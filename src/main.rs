mod compile;
mod dbug;
mod diag;
mod sm3;
mod source;
mod togl;
mod vcs;

use std::collections::HashMap;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use diag::{Diagnostic, Instruction, Span, Style, Use};
use serde_json::json;
use source::{display, find_name, Sources};

const USAGE: &str = "usage: gm_shadercompat [--format human|json] [--color auto|always|never] [--wine <command>] [--glsl <dir>] <file.vcs|file.hlsl|dir>...";
const MAX_ERRORS_PER_COMBO: usize = 64;
const GENERIC_TOGL_ERROR: &str = "D3DToGL: GLSL translation error!";

#[derive(PartialEq)]
enum Format {
	Human,
	Json,
}

struct Options {
	wine: Option<String>,
	glsl_dir: Option<PathBuf>,
	format: Format,
	color: Option<bool>,
	inputs: Vec<PathBuf>,
}

fn parse_args() -> Result<Options, String> {
	let mut opts = Options {
		wine: (!cfg!(windows)).then(|| "wine".to_string()),
		glsl_dir: None,
		format: Format::Human,
		color: None,
		inputs: Vec::new(),
	};

	let mut args = std::env::args().skip(1);
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--wine" => opts.wine = Some(args.next().ok_or("--wine needs a command")?),
			"--glsl" => opts.glsl_dir = Some(PathBuf::from(args.next().ok_or("--glsl needs a directory")?)),
			"--format" => {
				opts.format = match args.next().as_deref() {
					Some("human") => Format::Human,
					Some("json") => Format::Json,
					_ => return Err("--format takes human or json".into()),
				}
			}
			"--color" => {
				opts.color = match args.next().as_deref() {
					Some("auto") => None,
					Some("always") => Some(true),
					Some("never") => Some(false),
					_ => return Err("--color takes auto, always or never".into()),
				}
			}
			"-h" | "--help" => return Err(String::new()),
			_ => opts.inputs.push(PathBuf::from(arg)),
		}
	}

	if opts.inputs.is_empty() {
		return Err(String::new());
	}
	Ok(opts)
}

fn is_shader(path: &Path) -> bool {
	matches!(path.extension().and_then(|e| e.to_str()), Some("vcs" | "hlsl"))
}

fn collect(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
	let mut files = Vec::new();
	for input in inputs {
		if input.is_dir() {
			let mut found: Vec<PathBuf> = fs::read_dir(input)
				.map_err(|e| format!("{}: {e}", input.display()))?
				.filter_map(|e| e.ok().map(|e| e.path()))
				.filter(|p| p.is_file() && is_shader(p))
				.collect();
			found.sort();
			files.extend(found);
		} else {
			files.push(input.clone());
		}
	}
	Ok(files.into_iter().map(|f| std::path::absolute(&f).unwrap_or(f)).collect())
}

struct Finding {
	code: String,
	message: String,
	help: Option<String>,
	names: Vec<String>,
	location: Option<(PathBuf, u32, Option<(usize, usize)>)>,
	instruction: Option<Instruction>,
	notes: Vec<String>,
}

type Key = (String, String, Option<(PathBuf, u32)>, Option<PathBuf>);

#[derive(Default)]
struct Report {
	diagnostics: Vec<Diagnostic>,
	index: HashMap<Key, usize>,
	sources: Sources,
}

impl Report {
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
}

enum Status {
	Ok(usize),
	Fail(usize),
	Error,
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

fn source_base(shader: &Path) -> Option<PathBuf> {
	let shader = std::path::absolute(shader).ok()?;
	let dir = shader.parent()?;
	if shader.extension().and_then(|e| e.to_str()) == Some("hlsl") {
		return Some(dir.to_path_buf());
	}
	let fxc = dir.file_name()? == "fxc";
	let shaders = dir.parent()?;
	(fxc && shaders.file_name()? == "shaders").then(|| shaders.parent().map(Path::to_path_buf)).flatten()
}

fn check_combo(report: &mut Report, shader: &Path, combo: &vcs::Combo, total: usize, centroid: u32, glsl: Option<&Path>) -> Result<bool, String> {
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
			let finding = Finding {
				code: "comment-size".into(),
				message: format!("{len} dword comment block is longer than ToGL can skip"),
				help: Some(format!("ToGL reads comment sizes with 12 bits (at most {}), usually a debug info build (`/Zi`); ship it without", sm3::TOGL_MAX_COMMENT)),
				names: Vec::new(),
				location: None,
				instruction: None,
				notes: Vec::new(),
			};
			report.add(shader, Some(id), total, finding);
		}
		tokens[start..=start + len].fill(0);
	}

	let mut result = togl::translate(&tokens, vertex, centroid);

	if let Some(path) = glsl {
		fs::write(path, &result.glsl).map_err(|e| format!("{}: {e}", path.display()))?;
	}

	if result.error.is_none() {
		if result.truncations == 0 {
			return Ok(passed);
		}
		let finding = Finding {
			code: "truncated".into(),
			message: "ToGL ran out of buffer space and cut the GLSL short".into(),
			help: Some("the shader is too large for ToGL's 500000 byte translation buffer".into()),
			names: Vec::new(),
			location: None,
			instruction: None,
			notes: Vec::new(),
		};
		report.add(shader, Some(id), total, finding);
		return Ok(false);
	}

	for _ in 0..MAX_ERRORS_PER_COMBO {
		let Some((message, token)) = result.error.take() else { break };
		let Some(at) = token.checked_sub(1).and_then(|t| sm3::at_token(&list, t)) else {
			let finding = Finding {
				code: "togl".into(),
				message,
				help: None,
				names: Vec::new(),
				location: None,
				instruction: None,
				notes: Vec::new(),
			};
			report.add(shader, Some(id), total, finding);
			break;
		};
		if tokens[at.start] == 0 {
			break;
		}

		let problem = sm3::classify(&combo.tokens, &list, at, vertex, &message);
		let location = debug.as_ref().and_then(|d| d.location(at.start)).map(|(file, line)| (file.to_path_buf(), line, None)).or_else(|| {
			let main = debug.as_ref()?.main.clone()?;
			let line = report.sources.get(&main)?.find_line(&problem.names)?;
			Some((main, line, None))
		});
		let mut notes = Vec::new();
		if location.is_none() && debug.is_none() {
			notes.push("this vcs has no debug info; check the .hlsl to get source lines".to_string());
		}
		if message != GENERIC_TOGL_ERROR && problem.code != "debugger" {
			notes.push(format!("ToGL: {message}"));
		}
		report.add(
			shader,
			Some(id),
			total,
			Finding {
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

fn input_error(report: &mut Report, shader: &Path, code: &str, message: String, notes: Vec<String>) {
	let finding = Finding { code: code.into(), message, help: None, names: Vec::new(), location: None, instruction: None, notes };
	report.add(shader, None, 0, finding);
}

fn check(report: &mut Report, path: &Path, opts: &Options) -> Status {
	let bytes = match path.extension().and_then(|e| e.to_str()) {
		Some("hlsl") => match compile::compile(path, opts.wine.as_deref()) {
			Ok(bytes) => bytes,
			Err(failure) => {
				if failure.messages.is_empty() {
					let notes = if failure.log.is_empty() { Vec::new() } else { vec![failure.log] };
					input_error(report, path, "compile", "the shader failed to compile".into(), notes);
				}
				for m in failure.messages {
					let columns = (m.column > 0).then(|| (m.column as usize, m.end_column.map_or(m.column as usize + 1, |e| e as usize + 1)));
					let finding = Finding {
						code: m.code,
						message: m.message,
						help: None,
						names: Vec::new(),
						location: Some((m.file, m.line, columns)),
						instruction: None,
						notes: Vec::new(),
					};
					report.add(path, None, 0, finding);
				}
				return Status::Error;
			}
		},
		Some("vcs") => match fs::read(path) {
			Ok(bytes) => bytes,
			Err(e) => {
				input_error(report, path, "input", format!("cannot read the file: {e}"), Vec::new());
				return Status::Error;
			}
		},
		_ => {
			input_error(report, path, "input", "expected a .vcs or .hlsl file".into(), Vec::new());
			return Status::Error;
		}
	};

	let shader = match vcs::parse(&bytes) {
		Ok(shader) if !shader.combos.is_empty() => shader,
		Ok(_) => {
			input_error(report, path, "input", "no combos in the file".into(), Vec::new());
			return Status::Error;
		}
		Err(e) => {
			input_error(report, path, "input", e, Vec::new());
			return Status::Error;
		}
	};

	if let Some(dir) = &opts.glsl_dir {
		if let Err(e) = fs::create_dir_all(dir) {
			input_error(report, path, "input", format!("{}: {e}", dir.display()), Vec::new());
			return Status::Error;
		}
	}

	let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
	let total = shader.combos.len();
	let mut failed = 0;
	for combo in &shader.combos {
		let glsl = opts.glsl_dir.as_ref().map(|dir| {
			let name = if total > 1 { format!("{stem}_s{}_d{}.glsl", combo.static_id, combo.dynamic_id) } else { format!("{stem}.glsl") };
			dir.join(name)
		});
		match check_combo(report, path, combo, total, shader.centroid_mask, glsl.as_deref()) {
			Ok(true) => {}
			Ok(false) => failed += 1,
			Err(e) => {
				input_error(report, path, "input", e, Vec::new());
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

fn plural(n: usize, word: &str) -> String {
	if n == 1 {
		format!("{n} {word}")
	} else {
		format!("{n} {word}s")
	}
}

fn main() -> ExitCode {
	let opts = match parse_args() {
		Ok(opts) => opts,
		Err(e) => {
			if !e.is_empty() {
				eprintln!("{e}");
			}
			eprintln!("{USAGE}");
			return ExitCode::from(2);
		}
	};

	let files = match collect(&opts.inputs) {
		Ok(files) => files,
		Err(e) => {
			eprintln!("{e}");
			return ExitCode::from(2);
		}
	};

	let color = opts.color.unwrap_or_else(|| std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none());
	let style = Style { color };
	let progress = opts.format == Format::Human && std::io::stderr().is_terminal();

	let mut report = Report::default();
	let mut statuses = Vec::new();
	for (i, file) in files.iter().enumerate() {
		if progress {
			eprint!("\r\x1b[K    checking {}/{} {}", i + 1, files.len(), display(file));
			let _ = std::io::stderr().flush();
		}
		statuses.push(check(&mut report, file, &opts));
	}
	if progress {
		eprint!("\r\x1b[K");
	}

	let passed = statuses.iter().filter(|s| matches!(s, Status::Ok(_))).count();
	let failed = statuses.iter().filter(|s| matches!(s, Status::Fail(_))).count();
	let errored = statuses.iter().filter(|s| matches!(s, Status::Error)).count();

	let mut out = std::io::stdout().lock();
	match opts.format {
		Format::Json => {
			for d in &report.diagnostics {
				let _ = writeln!(out, "{}", d.json());
			}
			for (file, status) in files.iter().zip(&statuses) {
				let (name, combos) = match status {
					Status::Ok(n) => ("ok", Some(*n)),
					Status::Fail(n) => ("fail", Some(*n)),
					Status::Error => ("error", None),
				};
				let _ = writeln!(out, "{}", json!({ "type": "shader", "file": display(file), "status": name, "combos": combos }));
			}
			let _ = writeln!(out, "{}", json!({ "type": "summary", "ok": passed, "failed": failed, "errors": errored }));
		}
		Format::Human => {
			for d in &report.diagnostics {
				let _ = writeln!(out, "{}", d.render(style));
			}
			if failed > 0 {
				let _ = writeln!(out, "{}: ToGL translation failed for {} of {}", style.error(), failed, plural(files.len(), "shader"));
			}
			if errored > 0 {
				let _ = writeln!(out, "{}: {} could not be checked", style.error(), plural(errored, "shader"));
			}
			if failed == 0 && errored == 0 {
				let verb = if passed == 1 { "translates" } else { "translate" };
				let _ = writeln!(out, "{}: {} {verb}", style.ok(), plural(passed, "shader"));
			}
		}
	}

	if errored > 0 {
		ExitCode::from(2)
	} else if failed > 0 {
		ExitCode::from(1)
	} else {
		ExitCode::SUCCESS
	}
}
