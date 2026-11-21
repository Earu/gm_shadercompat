use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::compile::CompileFailure;
use crate::diag::{Diagnostic, Instruction, Level, Span, Use};
use crate::source::{display, find_name, Sources};
use crate::{dbug, glsl, sm3, togl, vcs, vtf};

const MAX_ERRORS_PER_COMBO: usize = 64;
const GENERIC_TOGL_ERROR: &str = "D3DToGL: GLSL translation error!";

pub enum Status {
	Ok(usize),
	Fail(usize),
	Error,
}

struct Finding {
	level: Level,
	code: String,
	message: String,
	help: Option<String>,
	names: Vec<String>,
	location: Option<(PathBuf, u32, Option<(usize, usize)>)>,
	instruction: Option<Instruction>,
	notes: Vec<String>,
}

impl Finding {
	fn plain(code: &str, message: String) -> Finding {
		Finding { level: Level::Error, code: code.into(), message, help: None, names: Vec::new(), location: None, instruction: None, notes: Vec::new() }
	}
}

type Key = (String, String, Option<(PathBuf, u32)>, Option<PathBuf>);

#[derive(Default)]
pub struct Checker {
	diagnostics: Vec<Diagnostic>,
	index: HashMap<Key, usize>,
	sources: Sources,
	glslang: Option<PathBuf>,
}

fn truncation_help(size: usize) -> String {
	match size {
		togl::SECTION_ALU => format!("the shader's ALU code is over ToGL's {} byte section buffer", togl::SECTION_ALU),
		togl::SECTION_DECLARATIONS => format!("the shader's attribute or parameter declarations are over ToGL's {} byte section buffer", togl::SECTION_DECLARATIONS),
		togl::OUTPUT_SIZE => format!("the shader is over ToGL's {} byte translation buffer", togl::OUTPUT_SIZE),
		0 => "ToGL cut a string short".into(),
		n => format!("ToGL cut a string short in a {n} byte internal buffer"),
	}
}

const GLSL_ERRORS_SHOWN: usize = 3;

fn absolute(path: &Path) -> PathBuf {
	std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn source_base(shader: &Path) -> Option<PathBuf> {
	let dir = shader.parent()?;
	if shader.extension().and_then(|e| e.to_str()) == Some("hlsl") {
		return Some(dir.to_path_buf());
	}
	let fxc = dir.file_name()? == "fxc";
	let shaders = dir.parent()?;
	(fxc && shaders.file_name()? == "shaders").then(|| shaders.parent().map(Path::to_path_buf)).flatten()
}

fn closing_loop(list: &[sm3::Instruction], from: usize) -> Option<usize> {
	let mut depth = 0;
	for i in &list[from..] {
		match i.opcode {
			27 => depth += 1,
			29 => {
				depth -= 1;
				if depth == 0 {
					return Some(i.index);
				}
			}
			_ => {}
		}
	}
	None
}

impl Checker {
	pub fn new() -> Checker {
		Checker::default()
	}

	pub fn diagnostics(&self) -> &[Diagnostic] {
		&self.diagnostics
	}

	pub fn into_diagnostics(self) -> Vec<Diagnostic> {
		self.diagnostics
	}

	pub fn set_glslang(&mut self, command: Option<PathBuf>) {
		self.glslang = command;
	}

	pub fn glslang(&self) -> Option<&Path> {
		self.glslang.as_deref()
	}

	pub fn check_vtf(&mut self, texture: &Path, bytes: &[u8]) -> Status {
		let texture = absolute(texture);
		match vtf::check(bytes) {
			Ok(problems) => {
				for p in problems {
					let mut finding = Finding::plain(p.code, p.message);
					finding.level = Level::Warning;
					finding.help = Some(p.help.into());
					self.add(&texture, None, 0, finding);
				}
				Status::Ok(0)
			}
			Err(e) => {
				self.add_error(&texture, "input", e, Vec::new());
				Status::Error
			}
		}
	}

	fn add(&mut self, shader: &Path, combo: Option<(u32, u32)>, total: usize, finding: Finding) {
		let line_key = finding.location.as_ref().map(|(file, line, _)| (file.clone(), *line));
		let shader_key = line_key.is_none().then(|| shader.to_path_buf());
		let key = (finding.code.clone(), finding.message.clone(), line_key, shader_key);

		let at = match self.index.get(&key) {
			Some(&at) => at,
			None => {
				let mut function = None;
				let span = finding.location.map(|(file, line, columns)| {
					let source = self.sources.get(&file);
					let text = source.and_then(|s| s.line(line)).map(str::to_string);
					function = source.and_then(|s| s.function_at(line)).map(str::to_string);
					let columns = match columns {
						Some(range) => vec![range],
						None => text.as_deref().map(|t| find_name(t, &finding.names)).unwrap_or_default(),
					};
					Span { file, line, columns, text }
				});
				self.diagnostics.push(Diagnostic {
					level: finding.level,
					code: finding.code,
					message: finding.message,
					span,
					function,
					uses: Vec::new(),
					instruction: finding.instruction,
					help: finding.help,
					notes: finding.notes,
				});
				self.index.insert(key, self.diagnostics.len() - 1);
				self.diagnostics.len() - 1
			}
		};

		let uses = &mut self.diagnostics[at].uses;
		let entry = match uses.iter().position(|u| u.shader == shader) {
			Some(i) => &mut uses[i],
			None => {
				uses.push(Use { shader: shader.to_path_buf(), combos: Vec::new(), total });
				uses.last_mut().unwrap()
			}
		};
		if let Some(combo) = combo {
			if !entry.combos.contains(&combo) {
				entry.combos.push(combo);
			}
		}
	}

	pub fn add_error(&mut self, shader: &Path, code: &str, message: String, notes: Vec<String>) {
		let mut finding = Finding::plain(code, message);
		finding.notes = notes;
		self.add(&absolute(shader), None, 0, finding);
	}

	pub fn check_build(&mut self, shader: &Path, built: &[u8], committed: &Path) -> bool {
		let shader = absolute(shader);
		let bytes = match fs::read(committed) {
			Ok(bytes) => bytes,
			Err(e) => {
				self.add_error(&shader, "missing-vcs", format!("cannot read {}: {e}", display(committed)), Vec::new());
				return false;
			}
		};
		match vcs::compare(&bytes, built) {
			Ok(None) => true,
			Ok(Some(m)) => {
				let mut finding = Finding::plain("stale-vcs", format!("{} does not match this source, {} of {} combos differ", display(committed), m.differing, m.total));
				finding.help = Some("rebuild the .vcs from this source and commit both".into());
				self.add(&shader, None, 0, finding);
				false
			}
			Err(e) => {
				self.add_error(&shader, "input", format!("{}: {e}", display(committed)), Vec::new());
				false
			}
		}
	}

	pub fn add_compile_failure(&mut self, shader: &Path, failure: CompileFailure) {
		if failure.messages.is_empty() {
			let notes = if failure.log.is_empty() { Vec::new() } else { vec![failure.log] };
			self.add_error(shader, "compile", "the shader failed to compile".into(), notes);
			return;
		}
		let shader = absolute(shader);
		for m in failure.messages {
			let columns = (m.column > 0).then(|| (m.column as usize, m.end_column.map_or(m.column as usize + 1, |e| e as usize + 1)));
			let mut finding = Finding::plain(&m.code, m.message);
			finding.location = Some((m.file, m.line, columns));
			self.add(&shader, None, 0, finding);
		}
	}

	pub fn check_vcs(&mut self, shader: &Path, bytes: &[u8], glsl_dir: Option<&Path>) -> Status {
		let shader = absolute(shader);
		let parsed = match vcs::parse(bytes) {
			Ok(parsed) if !parsed.combos.is_empty() => parsed,
			Ok(_) => {
				self.add_error(&shader, "input", "no combos in the file".into(), Vec::new());
				return Status::Error;
			}
			Err(e) => {
				self.add_error(&shader, "input", e, Vec::new());
				return Status::Error;
			}
		};

		if let Some(dir) = glsl_dir {
			if let Err(e) = fs::create_dir_all(dir) {
				self.add_error(&shader, "input", format!("{}: {e}", dir.display()), Vec::new());
				return Status::Error;
			}
		}

		let stem = shader.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
		let total = parsed.combos.len();
		let mut failed = 0;
		for combo in &parsed.combos {
			let glsl = glsl_dir.map(|dir| {
				let name = if total > 1 { format!("{stem}_s{}_d{}.glsl", combo.static_id, combo.dynamic_id) } else { format!("{stem}.glsl") };
				dir.join(name)
			});
			match self.check_combo(&shader, combo, total, parsed.centroid_mask, glsl.as_deref()) {
				Ok(true) => {}
				Ok(false) => failed += 1,
				Err(e) => {
					self.add_error(&shader, "input", e, Vec::new());
					return Status::Error;
				}
			}
		}

		if failed == 0 {
			Status::Ok(total)
		} else {
			Status::Fail(total)
		}
	}

	fn check_glsl(&mut self, shader: &Path, id: (u32, u32), total: usize, command: &Path, text: &str, vertex: bool) -> Result<bool, String> {
		let errors = glsl::validate(command, text, vertex)?;
		let Some(first) = errors.first() else { return Ok(true) };

		let mut finding = Finding::plain("glsl", format!("ToGL's GLSL does not compile: {}", first.message));
		finding.notes = errors
			.iter()
			.take(GLSL_ERRORS_SHOWN)
			.filter_map(|e| {
				let n = e.line?;
				let line = text.lines().nth(n.checked_sub(1)?)?.trim();
				Some(format!("GLSL line {n}: `{line}`"))
			})
			.collect();
		finding.help = Some("a driver that rejects it stops the game with SIGTRAP at the first draw (`CGLMShaderPair::RefreshProgramPair`)".into());
		self.add(shader, Some(id), total, finding);
		Ok(false)
	}

	fn check_combo(&mut self, shader: &Path, combo: &vcs::Combo, total: usize, centroid: u32, glsl: Option<&Path>) -> Result<bool, String> {
		let vertex = sm3::is_vertex_shader(&combo.tokens);
		let debug = dbug::parse(&combo.tokens, source_base(shader).as_deref());
		let list = sm3::instructions(&combo.tokens);
		let mut tokens = combo.tokens.clone();
		let id = (combo.static_id, combo.dynamic_id);
		let from_hlsl = shader.extension().and_then(|e| e.to_str()) == Some("hlsl");
		let mut passed = true;

		for (start, len) in sm3::long_comments(&tokens) {
			if !from_hlsl {
				passed = false;
				let mut finding = Finding::plain("comment-size", format!("{len} dword comment block is longer than ToGL can skip"));
				finding.help = Some(format!("ToGL reads comment sizes with 12 bits (at most {}), usually a debug info build (`/Zi`); ship it without", sm3::TOGL_MAX_COMMENT));
				self.add(shader, Some(id), total, finding);
			}
			tokens[start..=start + len].fill(0);
		}

		let depth_cmps = sm3::depth_cmps(&combo.tokens, &list);
		for at in &depth_cmps {
			passed = false;
			let mut finding = Finding::plain("cmp-depth", "`cmp` writes `oDepth` directly, which ToGL turns into invalid GLSL".into());
			finding.help = Some("ToGL copies the destination of every `cmp` into a vec4 temp, so this becomes `atomic_temp_var = gl_FragDepth`, which does not compile; `gm_shadercompat --patch-togl` can rewrite it in a separate ToGL copy to compute into a spare temp and `mov` that to `oDepth`".into());
			finding.location = debug.as_ref().and_then(|d| d.location(at.start)).map(|(file, line)| (file.to_path_buf(), line, None));
			if debug.is_none() {
				finding.notes.push("this vcs has no debug info; check the .hlsl to get source lines".to_string());
			}
			finding.instruction = Some(Instruction { index: at.index, opcode: "cmp" });
			self.add(shader, Some(id), total, finding);
		}

		let mut result = togl::translate(&tokens, vertex, centroid);

		if let Some(path) = glsl {
			fs::write(path, &result.glsl).map_err(|e| format!("{}: {e}", path.display()))?;
		}

		if result.error.is_none() {
			if result.truncations > 0 {
				let mut finding = Finding::plain("truncated", "ToGL ran out of buffer space and cut the GLSL short".into());
				finding.help = Some(truncation_help(result.truncated_size));
				self.add(shader, Some(id), total, finding);
				return Ok(false);
			}
			if let (true, Some(command)) = (depth_cmps.is_empty(), self.glslang.clone()) {
				if !self.check_glsl(shader, id, total, &command, &result.glsl, vertex)? {
					return Ok(false);
				}
			}
			return Ok(passed);
		}

		for _ in 0..MAX_ERRORS_PER_COMBO {
			let Some((message, token)) = result.error.take() else { break };
			let Some(at) = token.checked_sub(1).and_then(|t| sm3::at_token(&list, t)) else {
				self.add(shader, Some(id), total, Finding::plain("togl", message));
				break;
			};
			if tokens[at.start] == 0 {
				break;
			}

			let problem = sm3::classify(&combo.tokens, &list, at, vertex, &message);
			let location = debug.as_ref().and_then(|d| d.location(at.start)).map(|(file, line)| (file.to_path_buf(), line, None)).or_else(|| {
				let main = debug.as_ref()?.main.clone()?;
				let line = self.sources.get(&main)?.find_line(&problem.names)?;
				Some((main, line, None))
			});
			let mut notes = Vec::new();
			if location.is_none() && debug.is_none() {
				notes.push("this vcs has no debug info; check the .hlsl to get source lines".to_string());
			}
			if message != GENERIC_TOGL_ERROR && problem.code != "debugger" {
				notes.push(format!("ToGL: {message}"));
			}
			self.add(
				shader,
				Some(id),
				total,
				Finding {
					level: Level::Error,
					code: problem.code.into(),
					message: problem.message,
					help: Some(problem.help.into()),
					names: problem.names,
					location,
					instruction: Some(Instruction { index: at.index, opcode: sm3::name(at.opcode) }),
					notes,
				},
			);

			tokens[at.start..at.start + at.len].fill(0);
			if at.opcode == 27 {
				if let Some(end) = closing_loop(&list, at.index) {
					let end = &list[end];
					tokens[end.start..end.start + end.len].fill(0);
				}
			}
			if at.opcode == 31 {
				break;
			}
			result = togl::translate(&tokens, vertex, centroid);
		}

		Ok(false)
	}
}
