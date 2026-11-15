mod compile;
mod sm3;
mod togl;
mod vcs;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: gm_shadercompat [--wine <command>] [--glsl <dir>] <file.vcs|file.hlsl|dir>...";

struct Options {
	wine: Option<String>,
	glsl_dir: Option<PathBuf>,
	inputs: Vec<PathBuf>,
}

fn parse_args() -> Result<Options, String> {
	let mut opts = Options {
		wine: (!cfg!(windows)).then(|| "wine".to_string()),
		glsl_dir: None,
		inputs: Vec::new(),
	};

	let mut args = std::env::args().skip(1);
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--wine" => opts.wine = Some(args.next().ok_or("--wine needs a command")?),
			"--glsl" => opts.glsl_dir = Some(PathBuf::from(args.next().ok_or("--glsl needs a directory")?)),
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
	Ok(files)
}

enum Outcome {
	Pass,
	Fail(Vec<String>),
}

fn check(path: &Path, opts: &Options) -> Result<Outcome, String> {
	let bytes = match path.extension().and_then(|e| e.to_str()) {
		Some("hlsl") => compile::compile(path, opts.wine.as_deref())?,
		Some("vcs") => fs::read(path).map_err(|e| e.to_string())?,
		_ => return Err("expected a .vcs or .hlsl file".into()),
	};

	let shader = vcs::parse(&bytes)?;
	if shader.combos.is_empty() {
		return Err("no combos in the file".into());
	}

	let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
	let multi = shader.combos.len() > 1;
	let mut problems = Vec::new();

	for combo in &shader.combos {
		let vertex = sm3::is_vertex_shader(&combo.tokens);
		let result = togl::translate(&combo.tokens, vertex, shader.centroid_mask);
		let label = if multi { format!("combo {}/{}: ", combo.static_id, combo.dynamic_id) } else { String::new() };

		if let Some((message, token)) = &result.error {
			let list = sm3::instructions(&combo.tokens);
			let at = token.checked_sub(1).and_then(|t| sm3::at_token(&list, t));
			match at {
				Some(i) => problems.push(format!(
					"{label}instruction {} `{}`: {} ({message})",
					i.index,
					sm3::name(i.opcode),
					sm3::hint(&combo.tokens, &list, i, vertex)
				)),
				None => problems.push(format!("{label}{message}")),
			}
		} else if result.truncations > 0 {
			problems.push(format!("{label}ToGL ran out of buffer space and cut the GLSL short"));
		}

		if let Some(dir) = &opts.glsl_dir {
			fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
			let name = if multi { format!("{stem}_s{}_d{}.glsl", combo.static_id, combo.dynamic_id) } else { format!("{stem}.glsl") };
			fs::write(dir.join(name), &result.glsl).map_err(|e| e.to_string())?;
		}
	}

	Ok(if problems.is_empty() { Outcome::Pass } else { Outcome::Fail(problems) })
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

	let (mut passed, mut failed, mut errored) = (0, 0, 0);
	for file in &files {
		let name = file.display();
		match check(file, &opts) {
			Ok(Outcome::Pass) => {
				passed += 1;
				println!("ok    {name}");
			}
			Ok(Outcome::Fail(problems)) => {
				failed += 1;
				println!("FAIL  {name}");
				for problem in problems.iter().take(8) {
					println!("      {problem}");
				}
				if problems.len() > 8 {
					println!("      and {} more", problems.len() - 8);
				}
			}
			Err(e) => {
				errored += 1;
				println!("ERROR {name}: {e}");
			}
		}
	}

	println!("\n{passed} ok, {failed} failed, {errored} errors");
	if errored > 0 {
		ExitCode::from(2)
	} else if failed > 0 {
		ExitCode::from(1)
	} else {
		ExitCode::SUCCESS
	}
}
