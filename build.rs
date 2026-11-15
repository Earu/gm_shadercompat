fn main() {
	println!("cargo:rerun-if-changed=shim");
	println!("cargo:rerun-if-changed=togl");

	let windows_gnu = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
		&& std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu");

	let mut build = cc::Build::new();
	build
		.cpp(true)
		.file("shim/bridge.cpp")
		.include("shim")
		.include("togl")
		.define("POSIX", None)
		.flag_if_supported("-std=c++14")
		.flag_if_supported("/EHsc")
		.flag_if_supported("/permissive")
		.flag_if_supported("-fpermissive")
		.flag_if_supported("-w");

	if windows_gnu {
		build.cpp_link_stdlib(None);
	}

	let compiler = build.get_compiler();
	build.compile("togl");

	if windows_gnu {
		let found = std::process::Command::new(compiler.path()).arg("-print-file-name=libstdc++.a").output().ok();
		let path = found.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
		if let Some(dir) = std::path::Path::new(&path).parent().filter(|d| d.is_absolute()) {
			println!("cargo:rustc-link-search=native={}", dir.display());
		}
		println!("cargo:rustc-link-lib=static=stdc++");
	}
}
