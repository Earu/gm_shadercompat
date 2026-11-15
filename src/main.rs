mod bundled;

use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gm_shadercompat::{compile, display, run_to_end, Checker, Compiler, Status, Style};
use serde_json::json;

const USAGE: &str = "usage: gm_shadercompat [--format human|json] [--color auto|always|never] [--wine <command>] [--glsl <dir>] <file.vcs|file.hlsl|dir>...";

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

fn check(checker: &mut Checker, compiler: &mut Option<Result<Compiler, String>>, path: &Path, opts: &Options) -> Status {
	let bytes = match path.extension().and_then(|e| e.to_str()) {
		Some("hlsl") => {
			let compiler = match compiler.get_or_insert_with(|| bundled::compiler(opts.wine.clone())) {
				Ok(compiler) => compiler,
				Err(e) => {
					checker.add_error(path, "input", e.clone(), Vec::new());
					return Status::Error;
				}
			};
			match compile(path, compiler, true, run_to_end) {
				Ok(compiled) => compiled.vcs,
				Err(failure) => {
					checker.add_compile_failure(path, failure);
					return Status::Error;
				}
			}
		}
		Some("vcs") => match fs::read(path) {
			Ok(bytes) => bytes,
			Err(e) => {
				checker.add_error(path, "input", format!("cannot read the file: {e}"), Vec::new());
				return Status::Error;
			}
		},
		_ => {
			checker.add_error(path, "input", "expected a .vcs or .hlsl file".into(), Vec::new());
			return Status::Error;
		}
	};

	checker.check_vcs(path, &bytes, opts.glsl_dir.as_deref())
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

	let mut checker = Checker::new();
	let mut compiler = None;
	let mut statuses = Vec::new();
	for (i, file) in files.iter().enumerate() {
		if progress {
			eprint!("\r\x1b[K    checking {}/{} {}", i + 1, files.len(), display(file));
			let _ = std::io::stderr().flush();
		}
		statuses.push(check(&mut checker, &mut compiler, file, &opts));
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
			for d in checker.diagnostics() {
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
			for d in checker.diagnostics() {
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
