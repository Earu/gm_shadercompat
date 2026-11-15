use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const COMPILER: &[u8] = include_bytes!("../bin/ShaderCompile.exe");
const D3DCOMPILER: &[u8] = include_bytes!("../bin/d3dcompiler_47.dll");

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

pub fn compile(hlsl: &Path, wine: Option<&str>) -> Result<Vec<u8>, String> {
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
		.args(["/O", "3", "-ver", ver, "-shaderpath"])
		.arg(&source_dir)
		.arg(&hlsl)
		.stdin(Stdio::null())
		.output()
		.map_err(|e| format!("cannot run the shader compiler: {e}"))?;

	let result = match fs::read(&vcs_path) {
		Ok(bytes) if output.status.success() => Ok(bytes),
		_ => {
			let log = String::from_utf8_lossy(&output.stdout);
			let log: Vec<&str> = log.lines().map(str::trim_end).filter(|l| !l.is_empty()).collect();
			Err(format!("compile failed:\n{}", log.join("\n")))
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
