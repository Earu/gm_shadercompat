mod bundled;

use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gm_shadercompat::{compile, display, find_glslang, patch_vcs, run_to_end, Checker, Compiler, Status, Style};
use serde_json::json;

const USAGE: &str = "usage: gm_shadercompat [--format human|json] [--color auto|always|never] [--wine <command>] [--glsl <dir>] [--glslang <command> | --no-glslang] [--patch-togl] [--patch-out <dir>] [--verify <dir>] <file.vcs|file.hlsl|file.vtf|dir>...";

#[derive(PartialEq)]
enum Format {
	Human,
	Json,
}

enum Glslang {
	Find,
	Command(PathBuf),
	Off,
}

struct Options {
	wine: Option<String>,
	glsl_dir: Option<PathBuf>,
	glslang: Glslang,
	patch: bool,
	patch_dir: Option<PathBuf>,
	verify_dir: Option<PathBuf>,
	format: Format,
	color: Option<bool>,
	inputs: Vec<PathBuf>,
}

fn parse_args() -> Result<Options, String> {
	let mut opts = Options {
		wine: (!cfg!(windows)).then(|| "wine".to_string()),
		glsl_dir: None,
		glslang: Glslang::Find,
		patch: false,
		patch_dir: None,
		verify_dir: None,
		format: Format::Human,
		color: None,
		inputs: Vec::new(),
	};

	let mut args = std::env::args().skip(1);
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--wine" => opts.wine = Some(args.next().ok_or("--wine needs a command")?),
			"--glsl" => opts.glsl_dir = Some(PathBuf::from(args.next().ok_or("--glsl needs a directory")?)),
			"--glslang" => opts.glslang = Glslang::Command(PathBuf::from(args.next().ok_or("--glslang needs a command")?)),
			"--no-glslang" => opts.glslang = Glslang::Off,
			"--patch-togl" => opts.patch = true,
			"--patch-out" => {
				opts.patch = true;
				opts.patch_dir = Some(PathBuf::from(args.next().ok_or("--patch-out needs a directory")?));
			}
			"--verify" => opts.verify_dir = Some(PathBuf::from(args.next().ok_or("--verify needs a directory")?)),
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

fn extension(path: &Path) -> Option<&str> {
	path.extension().and_then(|e| e.to_str())
}

fn is_input(path: &Path) -> bool {
	matches!(extension(path), Some("vcs" | "hlsl" | "vtf"))
}

fn collect(inputs: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
	let mut files = Vec::new();
	for input in inputs {
		if input.is_dir() {
			let mut found: Vec<PathBuf> = fs::read_dir(input)
				.map_err(|e| format!("{}: {e}", input.display()))?
				.filter_map(|e| e.ok().map(|e| e.path()))
				.filter(|p| p.is_file() && is_input(p))
				.collect();
			found.sort();
			files.extend(found);
		} else {
			files.push(input.clone());
		}
	}
	Ok(files.into_iter().map(|f| std::path::absolute(&f).unwrap_or(f)).collect())
}

fn read(checker: &mut Checker, path: &Path) -> Option<Vec<u8>> {
	match fs::read(path) {
		Ok(bytes) => Some(bytes),
		Err(e) => {
			checker.add_error(path, "input", format!("cannot read the file: {e}"), Vec::new());
			None
		}
	}
}

fn patch(checker: &mut Checker, path: &Path, bytes: Vec<u8>, out: Option<&Path>) -> Option<Vec<u8>> {
	let patched = match patch_vcs(&bytes) {
		Ok(Some(patched)) => patched,
		Ok(None) => return Some(bytes),
		Err(e) => {
			checker.add_error(path, "input", e, Vec::new());
			return None;
		}
	};

	if let (Some(dir), Some(name)) = (out, path.file_name()) {
		let target = dir.join(name);
		if let Err(e) = fs::create_dir_all(dir).and_then(|_| fs::write(&target, &patched)) {
			checker.add_error(path, "input", format!("{}: {e}", target.display()), Vec::new());
			return None;
		}
	}
	Some(patched)
}

fn check(checker: &mut Checker, compiler: &mut Option<Result<Compiler, String>>, path: &Path, opts: &Options) -> Status {
	let bytes = match extension(path) {
		Some("hlsl") => {
			let compiler = match compiler.get_or_insert_with(|| bundled::compiler(opts.wine.clone())) {
				Ok(compiler) => compiler,
				Err(e) => {
					checker.add_error(path, "input", e.clone(), Vec::new());
					return Status::Error;
				}
			};
			if let Some(dir) = &opts.verify_dir {
				let release = match compile(path, compiler, false, run_to_end) {
					Ok(compiled) => compiled.vcs,
					Err(failure) => {
						checker.add_compile_failure(path, failure);
						return Status::Error;
					}
				};
				let stem = path.file_stem().unwrap_or_default().to_string_lossy();
				if !checker.check_build(path, &release, &dir.join(format!("{stem}.vcs"))) {
					return Status::Error;
				}
			}
			let compiled = match compile(path, compiler, true, run_to_end) {
				Ok(compiled) => compiled.vcs,
				Err(failure) => {
					checker.add_compile_failure(path, failure);
					return Status::Error;
				}
			};
			if opts.patch {
				match patch(checker, path, compiled, None) {
					Some(bytes) => bytes,
					None => return Status::Error,
				}
			} else {
				compiled
			}
		}
		Some("vcs") => {
			let Some(bytes) = read(checker, path) else { return Status::Error };
			if opts.patch {
				match patch(checker, path, bytes, opts.patch_dir.as_deref()) {
					Some(bytes) => bytes,
					None => return Status::Error,
				}
			} else {
				bytes
			}
		}
		Some("vtf") => {
			let Some(bytes) = read(checker, path) else { return Status::Error };
			return checker.check_vtf(path, &bytes);
		}
		_ => {
			checker.add_error(path, "input", "expected a .vcs, .hlsl or .vtf file".into(), Vec::new());
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
	let shaders = files.iter().any(|f| extension(f) != Some("vtf"));
	match &opts.glslang {
		Glslang::Command(command) => checker.set_glslang(Some(command.clone())),
		Glslang::Find => {
			checker.set_glslang(find_glslang());
			if checker.glslang().is_none() && shaders {
				eprintln!("warning: glslangValidator not found, the GLSL ToGL produces is not compiled (install glslang or pass --glslang <command>)");
			}
		}
		Glslang::Off => {}
	}

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
	let noun = if files.iter().any(|f| extension(f) == Some("vtf")) { "file" } else { "shader" };

	let mut out = std::io::stdout().lock();
	match opts.format {
		Format::Json => {
			for d in checker.diagnostics() {
				let _ = writeln!(out, "{}", d.json());
			}
			for (file, status) in files.iter().zip(&statuses) {
				let texture = extension(file) == Some("vtf");
				let (name, combos) = match status {
					Status::Ok(n) => ("ok", (!texture).then_some(*n)),
					Status::Fail(n) => ("fail", Some(*n)),
					Status::Error => ("error", None),
				};
				let kind = if texture { "texture" } else { "shader" };
				let _ = writeln!(out, "{}", json!({ "type": kind, "file": display(file), "status": name, "combos": combos }));
			}
			let _ = writeln!(out, "{}", json!({ "type": "summary", "ok": passed, "failed": failed, "errors": errored, "glsl_compiled": checker.glslang().is_some() }));
		}
		Format::Human => {
			for d in checker.diagnostics() {
				let _ = writeln!(out, "{}", d.render(style));
			}
			if failed > 0 {
				let _ = writeln!(out, "{}: ToGL translation failed for {} of {}", style.error(), failed, plural(files.len(), noun));
			}
			if errored > 0 {
				let _ = writeln!(out, "{}: {} could not be checked", style.error(), plural(errored, noun));
			}
			if failed == 0 && errored == 0 {
				let verb = if passed == 1 { "passes" } else { "pass" };
				let _ = writeln!(out, "{}: {} {verb}", style.ok(), plural(passed, noun));
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
