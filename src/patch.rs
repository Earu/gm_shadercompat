use crate::vcs::{decode_block, u32_at, BLOCK_END, BLOCK_RAW, BLOCK_SIZE_MASK};

const OP_MOV: u32 = 0x01;
const OP_DCL: u32 = 0x1f;
const OP_TEXLD: u32 = 0x42;
const OP_CMP: u32 = 0x58;
const OP_TEXLDL: u32 = 0x5f;
const OP_COMMENT: u32 = 0xfffe;
const OP_END: u32 = 0xffff;
const LITERALS: [u32; 4] = [0x1f, 0x2f, 0x30, 0x51];
const REG_TEMP: u32 = 0;
const REG_DEPTHOUT: u32 = 9;
const REG_SAMPLER: u32 = 10;
const SAMPLER_VOLUME: u32 = 4;
const MAX_TEMPS: u32 = 32;
const RESULT_MODIFIER: u32 = 0x00f0_0000;

fn register_type(token: u32) -> u32 {
	((token >> 28) & 0x7) | ((token >> 8) & 0x18)
}

pub fn patch_bytecode(tokens: &mut Vec<u32>) -> usize {
	let mut volumes = Vec::new();
	let mut depth_writes = Vec::new();
	let mut changed = 0;
	let mut highest: i64 = -1;
	let mut i = 1;
	while i < tokens.len() {
		let token = tokens[i];
		let op = token & 0xffff;
		if op == OP_END {
			break;
		}
		if op == OP_COMMENT {
			i += 1 + ((token >> 16) & 0x7fff) as usize;
			continue;
		}

		let length = ((token >> 24) & 0xf) as usize;
		let at = |k: usize| tokens.get(i + k).copied().unwrap_or(0);
		if op == OP_DCL && register_type(at(2)) == REG_SAMPLER && (at(1) >> 27) & 0xf == SAMPLER_VOLUME {
			volumes.push(at(2) & 0x7ff);
		}
		if !LITERALS.contains(&op) {
			for k in 1..=length {
				let operand = at(k);
				if register_type(operand) == REG_TEMP {
					highest = highest.max((operand & 0x7ff) as i64);
				}
			}
		}

		if op == OP_TEXLDL && volumes.contains(&(at(3) & 0x7ff)) {
			tokens[i] = (token & 0xffff_0000) | OP_TEXLD;
			changed += 1;
		} else if op == OP_CMP && length >= 2 && register_type(at(1)) == REG_DEPTHOUT && i + length < tokens.len() {
			depth_writes.push((i, length));
		}
		i += 1 + length;
	}

	let temp = (highest + 1) as u32;
	if !depth_writes.is_empty() && temp < MAX_TEMPS {
		for &(i, length) in depth_writes.iter().rev() {
			let dst = tokens[i + 1];
			let mut replacement = vec![0x8001_0000 | (dst & RESULT_MODIFIER) | temp];
			replacement.extend_from_slice(&tokens[i + 2..i + 1 + length]);
			replacement.extend_from_slice(&[0x0200_0000 | OP_MOV, dst & !RESULT_MODIFIER, 0x8000_0000 | temp]);
			tokens.splice(i + 1..i + 1 + length, replacement);
			changed += 1;
		}
	}

	changed
}

fn patch_combos(data: &[u8]) -> Result<(Vec<u8>, usize), String> {
	let mut out = Vec::with_capacity(data.len());
	let mut changed = 0;
	let mut offset = 0;
	while offset + 8 <= data.len() {
		let size = u32_at(data, offset + 4)? as usize;
		let bytes = data.get(offset + 8..offset + 8 + size).ok_or("combo bytecode is truncated")?;
		let mut tokens: Vec<u32> = bytes.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
		changed += patch_bytecode(&mut tokens);
		out.extend_from_slice(&data[offset..offset + 4]);
		out.extend_from_slice(&((tokens.len() * 4) as u32).to_le_bytes());
		for token in tokens {
			out.extend_from_slice(&token.to_le_bytes());
		}
		offset += 8 + size;
	}

	Ok((out, changed))
}

pub fn patch_vcs(data: &[u8]) -> Result<Option<Vec<u8>>, String> {
	let version = u32_at(data, 0)?;
	if version != 6 {
		return Err(format!("unsupported vcs version {version}, only version 6 (ShaderCompile) is handled"));
	}

	let records = u32_at(data, 20)? as usize;
	let table_end = 28 + records * 8;
	let prefix_end = table_end + 4 + u32_at(data, table_end)? as usize * 8;
	let prefix = data.get(table_end..prefix_end).ok_or("truncated duplicate table")?;

	let mut ids = Vec::with_capacity(records);
	let mut bodies: Vec<Option<Vec<u8>>> = Vec::with_capacity(records);
	let mut changed = 0;
	for r in 0..records {
		let static_id = u32_at(data, 28 + r * 8)?;
		let mut offset = u32_at(data, 32 + r * 8)? as usize;
		ids.push(static_id);
		if static_id == BLOCK_END {
			bodies.push(None);
			continue;
		}

		let mut body = Vec::new();
		loop {
			let header = u32_at(data, offset)?;
			if header == BLOCK_END {
				break;
			}
			let size = (header & BLOCK_SIZE_MASK) as usize;
			let block = data.get(offset + 4..offset + 4 + size).ok_or("combo block is truncated")?;
			let (combos, count) = patch_combos(&decode_block(header, block)?)?;
			body.extend_from_slice(&(BLOCK_RAW | combos.len() as u32).to_le_bytes());
			body.extend_from_slice(&combos);
			changed += count;
			offset += 4 + size;
		}
		body.extend_from_slice(&BLOCK_END.to_le_bytes());
		bodies.push(Some(body));
	}

	if changed == 0 {
		return Ok(None);
	}

	let mut offsets = Vec::with_capacity(records);
	let mut position = prefix_end;
	for body in &bodies {
		offsets.push(position);
		if let Some(body) = body {
			position += body.len();
		}
	}

	let mut out = data[..28].to_vec();
	for ((id, at), body) in ids.iter().zip(&offsets).zip(&bodies) {
		let at = if body.is_some() { *at } else { position };
		out.extend_from_slice(&id.to_le_bytes());
		out.extend_from_slice(&(at as u32).to_le_bytes());
	}
	out.extend_from_slice(prefix);
	for body in bodies.into_iter().flatten() {
		out.extend_from_slice(&body);
	}

	Ok(Some(out))
}

#[cfg(test)]
mod tests {
	use super::*;

	const PS30: u32 = 0xffff_0300;
	const END: u32 = 0x0000_ffff;

	#[test]
	fn volume_texldl_becomes_texld() {
		let mut tokens = vec![PS30, 0x0200_001f, 0xa000_0000, 0xa00f_0800, 0x0300_005f, 0x800f_0000, 0x80e4_0001, 0xa0e4_0800, END];
		assert_eq!(patch_bytecode(&mut tokens), 1);
		assert_eq!(tokens[4], 0x0300_0042);
	}

	#[test]
	fn texldl_2d_is_left_alone() {
		let mut tokens = vec![PS30, 0x0200_001f, 0x9000_0000, 0xa00f_0800, 0x0300_005f, 0x800f_0000, 0x80e4_0001, 0xa0e4_0800, END];
		let before = tokens.clone();
		assert_eq!(patch_bytecode(&mut tokens), 0);
		assert_eq!(tokens, before);
	}

	#[test]
	fn depth_cmp_goes_through_a_spare_temp() {
		let mut tokens = vec![PS30, 0x0200_0001, 0x800f_0001, 0xa0e4_0000, 0x0400_0058, 0x9011_0800, 0x8000_0001, 0xa0e4_0000, 0xa0e4_0001, END];
		assert_eq!(patch_bytecode(&mut tokens), 1);
		assert_eq!(
			tokens,
			[PS30, 0x0200_0001, 0x800f_0001, 0xa0e4_0000, 0x0400_0058, 0x8011_0002, 0x8000_0001, 0xa0e4_0000, 0xa0e4_0001, 0x0200_0001, 0x9001_0800, 0x8000_0002, END]
		);
	}

	#[test]
	fn depth_cmp_without_a_spare_temp_is_left_alone() {
		let mut tokens = vec![PS30, 0x0200_0001, 0x800f_001f, 0xa0e4_0000, 0x0400_0058, 0x9001_0800, 0x8000_0001, 0xa0e4_0000, 0xa0e4_0001, END];
		let before = tokens.clone();
		assert_eq!(patch_bytecode(&mut tokens), 0);
		assert_eq!(tokens, before);
	}

	#[test]
	fn comments_are_skipped() {
		let mut tokens = vec![PS30, 0x0002_fffe, 0x0000_005f, 0x0000_0058, END];
		let before = tokens.clone();
		assert_eq!(patch_bytecode(&mut tokens), 0);
		assert_eq!(tokens, before);
	}
}
