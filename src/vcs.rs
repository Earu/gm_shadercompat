pub struct Combo {
	pub static_id: u32,
	pub dynamic_id: u32,
	pub tokens: Vec<u32>,
}

pub struct Vcs {
	pub centroid_mask: u32,
	pub combos: Vec<Combo>,
}

pub const BLOCK_END: u32 = 0xffff_ffff;
pub const BLOCK_RAW: u32 = 0x8000_0000;
const BLOCK_LZMA: u32 = 0x4000_0000;
pub const BLOCK_SIZE_MASK: u32 = 0x3fff_ffff;

pub fn u32_at(data: &[u8], offset: usize) -> Result<u32, String> {
	data.get(offset..offset + 4)
		.map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
		.ok_or_else(|| format!("truncated file at offset {offset}"))
}

fn decompress_lzma(block: &[u8]) -> Result<Vec<u8>, String> {
	if block.len() < 17 || &block[0..4] != b"LZMA" {
		return Err("block is missing its LZMA header".into());
	}

	let actual = u32_at(block, 4)? as u64;
	let compressed = u32_at(block, 8)? as usize;
	let body = block.get(17..17 + compressed).ok_or("LZMA block is truncated")?;

	let mut stream = Vec::with_capacity(13 + body.len());
	stream.extend_from_slice(&block[12..17]);
	stream.extend_from_slice(&actual.to_le_bytes());
	stream.extend_from_slice(body);

	let mut out = Vec::with_capacity(actual as usize);
	lzma_rs::lzma_decompress(&mut stream.as_slice(), &mut out).map_err(|e| format!("LZMA: {e:?}"))?;
	Ok(out)
}

pub fn decode_block(header: u32, block: &[u8]) -> Result<Vec<u8>, String> {
	if header & BLOCK_LZMA != 0 {
		decompress_lzma(block)
	} else if header & BLOCK_RAW != 0 {
		Ok(block.to_vec())
	} else {
		Err(format!("unsupported block encoding {header:#010x}"))
	}
}

fn read_combos(static_id: u32, data: &[u8], combos: &mut Vec<Combo>) -> Result<(), String> {
	let mut offset = 0;
	while offset + 8 <= data.len() {
		let dynamic_id = u32_at(data, offset)?;
		let size = u32_at(data, offset + 4)? as usize;
		let bytes = data.get(offset + 8..offset + 8 + size).ok_or("combo bytecode is truncated")?;
		let tokens = bytes.chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
		combos.push(Combo { static_id, dynamic_id, tokens });
		offset += 8 + size;
	}

	Ok(())
}

pub fn parse(data: &[u8]) -> Result<Vcs, String> {
	let version = u32_at(data, 0)?;
	if version != 6 {
		return Err(format!("unsupported vcs version {version}, only version 6 (ShaderCompile) is handled"));
	}

	let centroid_mask = u32_at(data, 16)?;
	let records = u32_at(data, 20)? as usize;

	let mut combos = Vec::new();
	for i in 0..records {
		let static_id = u32_at(data, 28 + i * 8)?;
		let mut offset = u32_at(data, 32 + i * 8)? as usize;
		if static_id == BLOCK_END {
			continue;
		}

		loop {
			let header = u32_at(data, offset)?;
			if header == BLOCK_END {
				break;
			}

			let size = (header & BLOCK_SIZE_MASK) as usize;
			let block = data.get(offset + 4..offset + 4 + size).ok_or("combo block is truncated")?;
			read_combos(static_id, &decode_block(header, block)?, &mut combos)?;
			offset += 4 + size;
		}
	}

	Ok(Vcs { centroid_mask, combos })
}
