use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::dbug::host_path;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Compiler {
	pub exe: PathBuf,
	pub wine: Option<String>,
}

pub struct Compiled {
	pub vcs: Vec<u8>,
	pub log: String,
}

pub struct FxcMessage {
	pub file: PathBuf,
	pub line: u32,
	pub column: u32,
	pub end_column: Option<u32>,
	pub warning: bool,
	pub code: String,
	pub message: String,
}

pub struct CompileFailure {
	pub messages: Vec<FxcMessage>,
	pub log: String,
}

impl From<String> for CompileFailure {
	fn from(log: String) -> Self {
		CompileFailure { messages: Vec::new(), log }
	}
}

impl From<&str> for CompileFailure {
	fn from(log: &str) -> Self {
		log.to_string().into()
	}
}

pub fn run_to_end(command: &mut Command) -> io::Result<(ExitStatus, Vec<u8>)> {
	let output = command.output()?;
	Ok((output.status, output.stdout))
}

fn command(compiler: &Compiler) -> Command {
	let mut cmd = match &compiler.wine {
		Some(wine) => {
			let mut cmd = Command::new(wine);
			cmd.arg(&compiler.exe).env("WINEDLLOVERRIDES", "d3dcompiler_47=n").env("WINEDEBUG", "-all");
			cmd
		}
		None => Command::new(&compiler.exe),
	};
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		cmd.creation_flags(CREATE_NO_WINDOW);
	}
	cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
	cmd
}

fn parse_message(line: &str, base: &Path) -> Option<FxcMessage> {
	let (index, marker, warning) = [("): error ", false), ("): warning ", true)]
		.iter()
		.find_map(|&(marker, warning)| line.find(marker).map(|i| (i, marker, warning)))?;
	let head = &line[..index];
	let rest = &line[index + marker.len()..];
	let open = head.rfind('(')?;
	let mut location = head[open + 1..].split(',');
	let line_number = location.next()?.trim().parse().ok()?;
	let mut columns = location.next().unwrap_or("0").split('-');
	let column = columns.next()?.trim().parse().unwrap_or(0);
	let end_column = columns.next().and_then(|c| c.trim().parse().ok());
	let (code, message) = rest.split_once(':').unwrap_or(("", rest));

	Some(FxcMessage {
		file: host_path(head[..open].trim(), Some(base)),
		line: line_number,
		column,
		end_column,
		warning,
		code: code.trim().to_string(),
		message: message.trim().to_string(),
	})
}

fn parse_log(log: &str, base: &Path) -> Vec<FxcMessage> {
	let mut out: Vec<FxcMessage> = Vec::new();
	for line in log.split(['\r', '\n']) {
		let Some(message) = parse_message(line.trim(), base) else { continue };
		let duplicate = out.iter().any(|m| {
			m.file == message.file && m.line == message.line && m.column == message.column && m.code == message.code && m.message == message.message
		});
		if !duplicate {
			out.push(message);
		}
	}
	out
}

fn readable(log: &[u8]) -> String {
	let log = String::from_utf8_lossy(log);
	let lines: Vec<&str> = log.split(['\r', '\n']).map(str::trim_end).filter(|l| !l.is_empty()).collect();
	lines.join("\n")
}

pub fn compile(
	hlsl: &Path,
	compiler: &Compiler,
	debug_info: bool,
	run: impl FnOnce(&mut Command) -> io::Result<(ExitStatus, Vec<u8>)>,
) -> Result<Compiled, CompileFailure> {
	let hlsl = std::path::absolute(hlsl).map_err(|e| format!("{}: {e}", hlsl.display()))?;
	if !hlsl.is_file() {
		return Err("file not found".into());
	}
	let source_dir = hlsl.parent().ok_or("shader has no parent directory")?.to_path_buf();
	let stem = hlsl.file_stem().ok_or("shader has no file name")?.to_string_lossy().to_string();
	let ver = if stem.ends_with("30") { "30" } else { "20b" };

	let out_root = source_dir.join("shaders");
	let out_dir = out_root.join("fxc");
	let created_root = !out_root.exists();
	let created_dir = !out_dir.exists();
	let vcs_path = out_dir.join(format!("{stem}.vcs"));
	let previous = fs::read(&vcs_path).ok();

	let mut cmd = command(compiler);
	cmd.args(["/O", "3"]);
	if debug_info {
		cmd.arg("-debug-info");
	}
	cmd.args(["-ver", ver, "-shaderpath"]).arg(&source_dir).arg(&hlsl).current_dir(&source_dir);

	let result = match run(&mut cmd) {
		Err(e) => Err(format!("cannot run the shader compiler: {e}").into()),
		Ok((status, log)) => match fs::read(&vcs_path) {
			Ok(vcs) if status.success() => Ok(Compiled { vcs, log: readable(&log) }),
			_ => {
				let text = String::from_utf8_lossy(&log);
				let messages = parse_log(&text, &source_dir).into_iter().filter(|m| !m.warning).collect();
				Err(CompileFailure { messages, log: readable(&log) })
			}
		},
	};

	match previous {
		Some(bytes) => {
			let _ = fs::write(&vcs_path, bytes);
		}
		None => {
			let _ = fs::remove_file(&vcs_path);
		}
	}
	if created_dir {
		let _ = fs::remove_dir(&out_dir);
	}
	if created_root {
		let _ = fs::remove_dir(&out_root);
	}

	result
}
