use std::fs;
use std::path::PathBuf;

use gm_shadercompat::Compiler;

const COMPILER: &[u8] = include_bytes!("../bin/ShaderCompile.exe");
const D3DCOMPILER: &[u8] = include_bytes!("../bin/d3dcompiler_47.dll");

pub fn compiler(wine: Option<String>) -> Result<Compiler, String> {
	let dir = std::env::temp_dir().join(format!("gm_shadercompat-{}", env!("CARGO_PKG_VERSION")));
	fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

	for (name, bytes) in [("ShaderCompile.exe", COMPILER), ("d3dcompiler_47.dll", D3DCOMPILER)] {
		let path: PathBuf = dir.join(name);
		if fs::metadata(&path).map(|m| m.len() as usize == bytes.len()).unwrap_or(false) {
			continue;
		}
		fs::write(&path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
	}

	Ok(Compiler { exe: dir.join("ShaderCompile.exe"), wine })
}
