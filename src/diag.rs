use std::path::PathBuf;

use serde_json::{json, Value};

use crate::source::display;

pub struct Span {
	pub file: PathBuf,
	pub line: u32,
	pub columns: Vec<(usize, usize)>,
	pub text: Option<String>,
}

pub struct Use {
	pub shader: PathBuf,
	pub combos: Vec<(u32, u32)>,
	pub total: usize,
}

pub struct Instruction {
	pub index: usize,
	pub opcode: &'static str,
}

pub struct Diagnostic {
	pub code: String,
	pub message: String,
	pub span: Option<Span>,
	pub function: Option<String>,
	pub uses: Vec<Use>,
	pub instruction: Option<Instruction>,
	pub help: Option<String>,
	pub notes: Vec<String>,
}

#[derive(Clone, Copy)]
pub struct Style {
	pub color: bool,
}

impl Style {
	fn paint(self, code: &str, text: &str) -> String {
		if self.color {
			format!("\x1b[{code}m{text}\x1b[0m")
		} else {
			text.to_string()
		}
	}

	pub fn error(self) -> String {
		self.red("error")
	}

	pub fn ok(self) -> String {
		self.green("ok")
	}

	fn red(self, text: &str) -> String {
		self.paint("1;31", text)
	}

	fn bold(self, text: &str) -> String {
		self.paint("1", text)
	}

	fn blue(self, text: &str) -> String {
		self.paint("1;34", text)
	}

	fn green(self, text: &str) -> String {
		self.paint("1;32", text)
	}
}

fn width(text: &str) -> usize {
	text.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

fn plural(n: usize, word: &str) -> String {
	if n == 1 {
		format!("{n} {word}")
	} else {
		format!("{n} {word}s")
	}
}

fn describe_use(u: &Use) -> String {
	let name = display(&u.shader);
	let count = u.combos.len();
	if u.total <= 1 {
		name
	} else if count == u.total {
		format!("{name} (all {} combos)", u.total)
	} else {
		format!("{name} ({count} of {})", plural(u.total, "combo"))
	}
}

impl Diagnostic {
	fn uses_note(&self) -> Option<String> {
		if self.uses.is_empty() {
			return None;
		}
		let only = &self.uses[0];
		let complete = only.total <= 1 || only.combos.len() == only.total;
		if self.uses.len() == 1 && complete {
			let named = match &self.span {
				Some(span) => span.file == only.shader,
				None => true,
			};
			if named {
				return None;
			}
		}

		const SHOWN: usize = 6;
		let mut parts: Vec<String> = self.uses.iter().take(SHOWN).map(describe_use).collect();
		if self.uses.len() > SHOWN {
			parts.push(format!("and {} more", self.uses.len() - SHOWN));
		}
		let prefix = if self.span.is_some() { "used by" } else { "in" };
		Some(format!("{prefix} {}", parts.join(", ")))
	}

	pub fn render(&self, style: Style) -> String {
		let mut out = String::new();
		let code = if self.code.is_empty() { String::new() } else { format!("[{}]", self.code) };
		out.push_str(&format!("{}: {}\n", style.red(&format!("error{code}")), style.bold(&self.message)));

		let gutter = self.span.as_ref().map_or(1, |s| s.line.to_string().len());
		let pad = " ".repeat(gutter);
		let bar = style.blue("|");

		match &self.span {
			Some(span) => {
				let column = span.columns.first().map_or(String::new(), |(start, _)| format!(":{start}"));
				out.push_str(&format!("{pad}{} {}:{}{column}\n", style.blue("-->"), display(&span.file), span.line));
				if let Some(text) = &span.text {
					let shown = text.replace('\t', "    ");
					out.push_str(&format!("{pad} {bar}\n"));
					out.push_str(&format!("{} {bar} {shown}\n", style.blue(&span.line.to_string())));
					let ranges = if span.columns.is_empty() {
						let first = text.chars().take_while(|c| c.is_whitespace()).count();
						vec![(first + 1, text.chars().count() + 1)]
					} else {
						span.columns.clone()
					};
					let mut marks = String::new();
					let mut cursor = 1;
					for (start, end) in ranges {
						if start < cursor {
							continue;
						}
						let gap: String = text.chars().skip(cursor - 1).take(start - cursor).collect();
						let marked: String = text.chars().skip(start - 1).take(end - start).collect();
						marks.push_str(&" ".repeat(width(&gap)));
						marks.push_str(&"^".repeat(width(&marked).max(1)));
						cursor = end.max(start + 1);
					}
					out.push_str(&format!("{pad} {bar} {}\n", style.red(&marks)));
					out.push_str(&format!("{pad} {bar}\n"));
				}
			}
			None => {
				if let Some(u) = self.uses.first() {
					let at = self.instruction.as_ref().map_or(String::new(), |i| format!(", instruction {} (`{}`)", i.index, i.opcode));
					out.push_str(&format!("{pad}{} {}{at}\n", style.blue("-->"), display(&u.shader)));
				}
			}
		}

		let mut notes = Vec::new();
		if let Some(function) = &self.function {
			notes.push(format!("in function `{function}`"));
		}
		notes.extend(self.uses_note());
		notes.extend(self.notes.iter().cloned());
		for note in notes {
			out.push_str(&format!("{pad} {} {}: {note}\n", style.blue("="), style.bold("note")));
		}
		if let Some(help) = &self.help {
			out.push_str(&format!("{pad} {} {}: {help}\n", style.blue("="), style.green("help")));
		}
		out
	}

	pub fn json(&self) -> Value {
		json!({
			"type": "diagnostic",
			"level": "error",
			"code": self.code,
			"message": self.message,
			"span": self.span.as_ref().map(|s| json!({
				"file": display(&s.file),
				"line": s.line,
				"column": s.columns.first().map(|c| c.0),
				"end_column": s.columns.first().map(|c| c.1),
				"ranges": s.columns.iter().map(|&(start, end)| json!([start, end])).collect::<Vec<_>>(),
				"text": s.text,
			})),
			"function": self.function,
			"shaders": self.uses.iter().map(|u| json!({
				"file": display(&u.shader),
				"combos": u.combos.iter().map(|&(s, d)| json!({ "static": s, "dynamic": d })).collect::<Vec<_>>(),
				"total_combos": u.total,
			})).collect::<Vec<_>>(),
			"instruction": self.instruction.as_ref().map(|i| json!({ "index": i.index, "opcode": i.opcode })),
			"help": self.help,
			"notes": self.notes,
			"rendered": self.render(Style { color: false }),
		})
	}
}
