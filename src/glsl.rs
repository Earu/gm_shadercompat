use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct GlslError {
	pub line: Option<usize>,
	pub message: String,
}

pub fn find_glslang() -> Option<PathBuf> {
	let name = if cfg!(windows) { "glslangValidator.exe" } else { "glslangValidator" };
	std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(name)).find(|p| p.is_file())
}

fn parse_error(line: &str) -> Option<GlslError> {
	let rest = line.strip_prefix("ERROR: ")?;
	if rest.contains("compilation terminated") || rest.contains("compilation errors") {
		return None;
	}
	let mut parts = rest.splitn(3, ':');
	let (source, number, message) = (parts.next()?, parts.next()?, parts.next());
	match (source.trim().parse::<u32>(), number.trim().parse::<usize>(), message) {
		(Ok(_), Ok(n), Some(message)) => Some(GlslError { line: Some(n), message: message.split_whitespace().collect::<Vec<_>>().join(" ") }),
		_ => Some(GlslError { line: None, message: rest.split_whitespace().collect::<Vec<_>>().join(" ") }),
	}
}

pub fn validate(command: &Path, glsl: &str, vertex: bool) -> Result<Vec<GlslError>, String> {
	let mut child = Command::new(command)
		.args(["--stdin", "-S", if vertex { "vert" } else { "frag" }])
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|e| format!("{}: {e}", command.display()))?;

	let mut stdin = child.stdin.take().ok_or("glslang has no stdin")?;
	stdin.write_all(glsl.as_bytes()).map_err(|e| format!("{}: {e}", command.display()))?;
	drop(stdin);

	let output = child.wait_with_output().map_err(|e| format!("{}: {e}", command.display()))?;
	if output.status.success() {
		return Ok(Vec::new());
	}

	let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
	let errors: Vec<GlslError> = text.lines().filter_map(parse_error).collect();
	if errors.is_empty() {
		return Err(format!("{} failed: {}", command.display(), text.trim()));
	}
	Ok(errors)
}
