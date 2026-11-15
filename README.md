# gm_shadercompat

Checks Garry's Mod shaders against ToGL, the DX9 to OpenGL layer of the native Linux and macOS builds. When ToGL cannot translate a shader, the game crashes as soon as a material using it is created (`D3DToGL: GLSL translation error!`).

## Usage

```
gm_shadercompat [--format human|json] [--color auto|always|never] [--wine <command>] [--glsl <dir>] <file.vcs|file.hlsl|dir>...
```

- `.hlsl`: compiles it first with the bundled `ShaderCompile.exe` and `d3dcompiler_47.dll`, the same way gm_autoshader does (`/O 3`, `-ver 30` for names ending in `30`, `20b` otherwise), plus debug info so problems point at source lines. Includes resolve from the file's directory. Outside Windows it runs through `wine`, or the command given with `--wine`.
- `.vcs`: checks every static and dynamic combo in the file. Release builds have no source lines, so problems point at the instruction.
- A directory checks every `.vcs` and `.hlsl` directly inside it.
- `--glsl <dir>` writes the GLSL ToGL produced for each combo.

Exit code: 0 when everything passes, 1 when a shader fails, 2 when a file could not be read or compiled.

Every problem in a combo is reported, not only the first one ToGL stops at. A line shared by several shaders, such as one in an include, is reported once:

```
error[tex3dlod]: `tex3Dlod` is not supported by ToGL
  --> shaders/include/cloud_volume.hlsl:77:12
   |
77 |     float c = tex3Dlod(Shape, float4(q, 0.37, 0.0)).r;
   |               ^^^^^^^^
   |
   = note: in function `coverAt`
   = note: used by shaders/borealis_cloudbake_ps30.hlsl, shaders/borealis_vclouds_ps30.hlsl
   = help: ToGL only translates the LOD form for 2D samplers (`tex2Dlod`)
```

fxc compile errors are reported the same way, with their `X` code.

### JSON

`--format json` prints one JSON object per line on stdout:

- `{"type":"diagnostic", "level", "code", "message", "span", "function", "shaders", "instruction", "help", "notes", "rendered"}`.
  - `span` is `{file, line, column, end_column, ranges, text}` or null. Columns are 1-based and `end_column` is exclusive.
  - `shaders` lists each affected shader as `{file, combos: [{static, dynamic}], total_combos}`.
  - `rendered` is the human text.
- `{"type":"shader", "file", "status": "ok"|"fail"|"error", "combos"}` for each input.
- `{"type":"summary", "ok", "failed", "errors"}` last.

## What it checks

It runs ToGL's own translator (`togl/`) on the shader bytecode, with the options `CreatePixelShader` and `CreateVertexShader` pass. It only catches the crash: GLSL that gets rejected, or shaders that translate but render wrong, still need testing in game.

GMod's ToGL differs from Valve's public source. These GMod changes, found in `bin/linux64/libtogl_client.so`, are applied here:

- `dsx`/`dsy` translate to `dFdx`/`dFdy`.
- `sincos` only skips its two Taylor series operands for shader model 2.
- An `abs()` source with no swizzle keeps a valid register name when its LOD component is taken (`texldl`).
- The translation buffer holds 500000 bytes instead of 50000.
- Vertex shaders have 219 constant slots.

`Debugger()` is an unconditional breakpoint on Linux, so it counts as a failure.

ToGL reads comment sizes with 12 bits, so a comment block over 4095 dwords (debug info, `/Zi`) makes it parse the comment as code. A `.vcs` with one fails with `comment-size`.

Checked against GMod's own `libtogl_client.so` on 350k combos of GMod's stock shaders and about 800 hand-built probes covering every opcode, modifier, register type and declaration: identical GLSL and identical failures.

Things ToGL rejects include `tex3Dlod`/`texCUBElod`, `tex*grad` (`texldd`), loops compiled to `loop aL`, unconditional `break` and predicates.

## Library

The crate is also a library (gm_autoshader uses it):

```toml
gm_shadercompat = { git = "https://github.com/Earu/gm_shadercompat" }
```

- `compile(hlsl, &Compiler { exe, wine }, debug_info, run_to_end)` compiles with your own `ShaderCompile.exe`. `run` spawns the command, so the caller can track or kill the process.
- `Checker::check_vcs(shader, bytes, glsl_dir)` runs ToGL on every combo. `Checker::add_compile_failure` turns a failed compile into diagnostics.
- `Diagnostic::render_as(style, "warning", root)` renders with another label and paths relative to `root`. `Diagnostic::json` gives the JSON form.

The library does not bundle the compiler; only the CLI embeds `bin/`.

## Building

Prebuilt Linux and Windows binaries are attached to each GitHub release, and to every CI run as artifacts.

Building needs Rust and a C++ compiler for the vendored translator.

- Linux: `./build.sh` (g++ or clang++, or `CXX`)
- Windows: `.\build.ps1` (Visual Studio Build Tools with the "Desktop development with C++" workload, or MinGW-w64 with the GNU toolchain)

The binary lands in `target/release/`.

## License

MIT, see `LICENSE`. The third-party files below keep their own terms.

## Third-party files

- `togl/`: Valve's ToGL (github.com/ValveSoftware/ToGL at 7b64367), MIT, see `togl/LICENSE`. Patched as listed above.
- `bin/d3dcompiler_47.dll`: Microsoft's HLSL compiler, redistributable under the Windows SDK terms.
- `bin/ShaderCompile.exe`: the standalone shader compiler gm_autoshader ships.
