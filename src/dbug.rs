use std::collections::HashMap;
use std::path::{Path, PathBuf};

const FOURCC_DBUG: u32 = u32::from_le_bytes(*b"DBUG");
const NO_FILE: u16 = 0xffff;

pub struct DebugInfo {
	pub main: Option<PathBuf>,
	pub files: Vec<PathBuf>,
	lines: HashMap<usize, (usize, u32)>,
}

impl DebugInfo {
	pub fn location(&self, token: usize) -> Option<(&Path, u32)> {
		self.lines.get(&token).and_then(|&(file, line)| Some((self.files.get(file)?.as_path(), line)))
	}
}

fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
	data.get(offset..offset + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u16_at(data: &[u8], offset: usize) -> Option<u16> {
	data.get(offset..offset + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn cstr(data: &[u8], offset: usize) -> Option<String> {
	let rest = data.get(offset..)?;
	let end = rest.iter().position(|&b| b == 0)?;
	Some(String::from_utf8_lossy(&rest[..end]).into_owned())
}

pub fn host_path(name: &str, base: Option<&Path>) -> PathBuf {
	let bytes = name.as_bytes();
	let drive = bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/');
	if cfg!(windows) {
		let path = PathBuf::from(name);
		return match base {
			Some(base) if path.is_relative() => base.join(path),
			_ => path,
		};
	}

	if drive && bytes[0].eq_ignore_ascii_case(&b'z') {
		return PathBuf::from(name[2..].replace('\\', "/"));
	}
	let path = PathBuf::from(name.replace('\\', "/"));
	match base {
		Some(base) if !drive && path.is_relative() => base.join(path),
		_ => path,
	}
}

fn block(tokens: &[u32]) -> Option<Vec<u8>> {
	let mut pos = 1;
	while pos < tokens.len() {
		let token = tokens[pos];
		if token & 0xffff != 0xfffe {
			return None;
		}
		let len = ((token >> 16) & 0x7fff) as usize;
		if len > 0 && tokens.get(pos + 1) == Some(&FOURCC_DBUG) {
			let body = tokens.get(pos + 2..pos + 1 + len)?;
			return Some(body.iter().flat_map(|t| t.to_le_bytes()).collect());
		}
		pos += len + 1;
	}
	None
}

pub fn parse(tokens: &[u32], fallback_base: Option<&Path>) -> Option<DebugInfo> {
	let data = block(tokens)?;
	let file_count = u32_at(&data, 12)? as usize;
	let file_table = u32_at(&data, 16)? as usize;
	let instruction_count = u32_at(&data, 20)? as usize;
	let instruction_table = u32_at(&data, 24)? as usize;

	let mut names = Vec::with_capacity(file_count);
	for i in 0..file_count {
		names.push(cstr(&data, u32_at(&data, file_table + i * 4)? as usize)?);
	}

	let main = names.iter().map(|n| host_path(n, None)).find(|p| p.is_absolute());
	let base = main.as_ref().and_then(|m| m.parent()).or(fallback_base).map(Path::to_path_buf);
	let files = names.iter().map(|n| host_path(n, base.as_deref())).collect();

	let mut lines = HashMap::new();
	for i in 0..instruction_count {
		let entry = instruction_table + i * 8;
		let line = u16_at(&data, entry)? as u32;
		let file = u16_at(&data, entry + 2)?;
		let offset = u32_at(&data, entry + 4)? as usize;
		if file != NO_FILE && line > 0 {
			lines.insert(offset / 4, (file as usize, line));
		}
	}

	Some(DebugInfo { main, files, lines })
}
