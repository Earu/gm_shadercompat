pub struct TextureProblem {
	pub code: &'static str,
	pub message: String,
	pub help: &'static str,
}

const FORMAT_RGBA8888: i32 = 0;
const FORMAT_RGBA16161616: i32 = 25;
const FORMAT_RGBA32323232F: i32 = 29;
const FLAG_ONEBITALPHA: u32 = 0x1000;
const FLAG_EIGHTBITALPHA: u32 = 0x2000;

fn read<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], String> {
	data.get(offset..offset + N).and_then(|b| b.try_into().ok()).ok_or_else(|| "truncated VTF header".to_string())
}

pub fn check(data: &[u8]) -> Result<Vec<TextureProblem>, String> {
	if data.get(0..4) != Some(b"VTF\0") {
		return Err("not a VTF file".into());
	}
	let flags = u32::from_le_bytes(read(data, 20)?);
	let format = i32::from_le_bytes(read(data, 52)?);

	let mut problems = Vec::new();
	match format {
		FORMAT_RGBA16161616 => problems.push(TextureProblem {
			code: "vtf-rgba16",
			message: "ToGL loads RGBA16161616 VTFs as black".into(),
			help: "use RGBA16161616F, or ship the bytes as RGBA8888 and pack them into an RGBA16161616 render target in a shader",
		}),
		FORMAT_RGBA32323232F => problems.push(TextureProblem {
			code: "vtf-rgba32f",
			message: "ToGL loads RGBA32323232F VTFs with scrambled texels".into(),
			help: "use RGBA16161616F",
		}),
		FORMAT_RGBA8888 if flags & (FLAG_ONEBITALPHA | FLAG_EIGHTBITALPHA) == 0 => problems.push(TextureProblem {
			code: "vtf-alpha",
			message: "RGBA8888 VTF without an alpha flag loads with its alpha dropped".into(),
			help: "the material system picks BGR888 for it; set TEXTUREFLAGS_EIGHTBITALPHA (0x2000) if the alpha channel is used",
		}),
		_ => {}
	}

	Ok(problems)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn header(format: i32, flags: u32) -> Vec<u8> {
		let mut data = vec![0u8; 80];
		data[0..4].copy_from_slice(b"VTF\0");
		data[20..24].copy_from_slice(&flags.to_le_bytes());
		data[52..56].copy_from_slice(&format.to_le_bytes());
		data
	}

	fn codes(format: i32, flags: u32) -> Vec<&'static str> {
		check(&header(format, flags)).unwrap().iter().map(|p| p.code).collect()
	}

	#[test]
	fn formats() {
		assert_eq!(codes(25, 0), ["vtf-rgba16"]);
		assert_eq!(codes(29, 0), ["vtf-rgba32f"]);
		assert_eq!(codes(24, 0), Vec::<&str>::new());
		assert_eq!(codes(5, 0), Vec::<&str>::new());
	}

	#[test]
	fn alpha_flag() {
		assert_eq!(codes(0, 0x0300), ["vtf-alpha"]);
		assert_eq!(codes(0, 0x2308), Vec::<&str>::new());
		assert_eq!(codes(0, 0x1000), Vec::<&str>::new());
	}

	#[test]
	fn rejects_other_files() {
		assert!(check(b"nope").is_err());
		assert!(check(b"VTF\0").is_err());
	}
}
