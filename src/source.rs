use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

struct Function {
	name: String,
	start: u32,
	end: u32,
}

pub struct Source {
	lines: Vec<String>,
	functions: Vec<Function>,
}

impl Source {
	pub fn line(&self, line: u32) -> Option<&str> {
		self.lines.get(line.checked_sub(1)? as usize).map(String::as_str)
	}

	pub fn find_line(&self, names: &[String]) -> Option<u32> {
		names.iter().find_map(|name| {
			let one = std::slice::from_ref(name);
			self.lines.iter().position(|l| !find_name(l, one).is_empty()).map(|i| i as u32 + 1)
		})
	}

	pub fn function_at(&self, line: u32) -> Option<&str> {
		self.functions.iter().find(|f| line >= f.start && line <= f.end).map(|f| f.name.as_str())
	}
}

fn is_ident(c: char) -> bool {
	c.is_ascii_alphanumeric() || c == '_'
}

fn functions(text: &str) -> Vec<Function> {
	let chars: Vec<char> = text.chars().collect();
	let mut out = Vec::new();
	let mut line = 1u32;
	let mut depth = 0i32;
	let mut parens = 0i32;
	let mut candidate: Option<(String, u32)> = None;
	let mut after_params = false;
	let mut open: Option<(String, u32)> = None;
	let mut last_ident: Option<(String, u32)> = None;
	let mut at_line_start = true;
	let mut i = 0;

	while i < chars.len() {
		let c = chars[i];
		if c == '\n' {
			line += 1;
			at_line_start = true;
			i += 1;
			continue;
		}
		if at_line_start && c == '#' {
			while i < chars.len() && chars[i] != '\n' {
				if chars[i] == '\\' && chars.get(i + 1) == Some(&'\n') {
					line += 1;
					i += 1;
				}
				i += 1;
			}
			continue;
		}
		if !c.is_whitespace() {
			at_line_start = false;
		}
		if c == '/' && chars.get(i + 1) == Some(&'/') {
			while i < chars.len() && chars[i] != '\n' {
				i += 1;
			}
			continue;
		}
		if c == '/' && chars.get(i + 1) == Some(&'*') {
			i += 2;
			while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
				if chars[i] == '\n' {
					line += 1;
				}
				i += 1;
			}
			i += 2;
			continue;
		}
		if c == '"' {
			i += 1;
			while i < chars.len() && chars[i] != '"' && chars[i] != '\n' {
				i += 1;
			}
			i += 1;
			continue;
		}

		if is_ident(c) {
			let start = i;
			while i < chars.len() && is_ident(chars[i]) {
				i += 1;
			}
			if depth == 0 && parens == 0 && !after_params {
				last_ident = Some((chars[start..i].iter().collect(), line));
			}
			continue;
		}

		if depth == 0 {
			match c {
				'(' => {
					if parens == 0 && !after_params {
						candidate = last_ident.take();
					}
					parens += 1;
				}
				')' => {
					parens -= 1;
					if parens == 0 && candidate.is_some() {
						after_params = true;
					}
				}
				';' if parens == 0 => {
					candidate = None;
					after_params = false;
					last_ident = None;
				}
				'{' => {
					if after_params {
						open = candidate.take();
					}
					after_params = false;
					last_ident = None;
					depth += 1;
				}
				_ => {}
			}
		} else if c == '{' {
			depth += 1;
		} else if c == '}' {
			depth -= 1;
			if depth == 0 {
				if let Some((name, start)) = open.take() {
					out.push(Function { name, start, end: line });
				}
			}
		}
		i += 1;
	}

	out
}

#[derive(Default)]
pub struct Sources {
	cache: HashMap<PathBuf, Option<Source>>,
}

impl Sources {
	pub fn get(&mut self, path: &Path) -> Option<&Source> {
		self.cache
			.entry(path.to_path_buf())
			.or_insert_with(|| {
				let bytes = fs::read(path).ok()?;
				let text = String::from_utf8_lossy(&bytes).into_owned();
				Some(Source { lines: text.lines().map(|l| l.trim_end_matches('\r').to_string()).collect(), functions: functions(&text) })
			})
			.as_ref()
	}
}

pub fn find_name(line: &str, names: &[String]) -> Vec<(usize, usize)> {
	let chars: Vec<char> = line.chars().collect();
	for name in names {
		let target: Vec<char> = name.chars().collect();
		if target.is_empty() || target.len() > chars.len() {
			continue;
		}
		let mut found = Vec::new();
		for start in 0..=chars.len() - target.len() {
			let end = start + target.len();
			if chars[start..end] != target[..] {
				continue;
			}
			let before = start == 0 || !is_ident(chars[start - 1]);
			let after = end == chars.len() || !is_ident(chars[end]);
			if before && after {
				found.push((start + 1, end + 1));
			}
		}
		if !found.is_empty() {
			return found;
		}
	}
	Vec::new()
}

pub fn display(path: &Path) -> String {
	display_in(path, None)
}

pub fn display_in(path: &Path, root: Option<&Path>) -> String {
	let root = root.map(Path::to_path_buf).or_else(|| std::env::current_dir().ok());
	let shown = root.as_deref().and_then(|root| path.strip_prefix(root).ok()).unwrap_or(path);
	shown.display().to_string()
}
