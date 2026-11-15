use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::dbug::host_path;

const COMPILER: &[u8] = include_bytes!("../bin/ShaderCompile.exe");
const D3DCOMPILER: &[u8] = include_bytes!("../bin/d3dcompiler_47.dll");

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

fn tool_dir() -> Result<PathBuf, String> {
	let dir = std::env::temp_dir().join(format!("gm_shadercompat-{}", env!("CARGO_PKG_VERSION")));
	fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

	for (name, bytes) in [("ShaderCompile.exe", COMPILER), ("d3dcompiler_47.dll", D3DCOMPILER)] {
		let path = dir.join(name);
		if fs::metadata(&path).map(|m| m.len() as usize == bytes.len()).unwrap_or(false) {
			continue;
		}
		fs::write(&path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
	}

	Ok(dir)
}

fn command(exe: &Path, wine: Option<&str>) -> Command {
	match wine {
		Some(wine) => {
			let mut cmd = Command::new(wine);
			cmd.arg(exe).env("WINEDLLOVERRIDES", "d3dcompiler_47=n").env("WINEDEBUG", "-all");
			cmd
		}
		None => Command::new(exe),
	}
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

pub fn compile(hlsl: &Path, wine: Option<&str>) -> Result<Vec<u8>, CompileFailure> {
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

	let exe = tool_dir()?.join("ShaderCompile.exe");
	let output = command(&exe, wine)
		.args(["/O", "3", "-debug-info", "-ver", ver, "-shaderpath"])
		.arg(&source_dir)
		.arg(&hlsl)
		.current_dir(&source_dir)
		.stdin(Stdio::null())
		.output()
		.map_err(|e| format!("cannot run the shader compiler: {e}"))?;

	let result = match fs::read(&vcs_path) {
		Ok(bytes) if output.status.success() => Ok(bytes),
		_ => {
			let log = String::from_utf8_lossy(&output.stdout);
			let messages = parse_log(&log, &source_dir).into_iter().filter(|m| !m.warning).collect();
			let log: Vec<&str> = log.split(['\r', '\n']).map(str::trim_end).filter(|l| !l.is_empty()).collect();
			Err(CompileFailure { messages, log: log.join("\n") })
		}
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
