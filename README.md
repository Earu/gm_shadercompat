# gm_shadercompat

Checks Garry's Mod shaders against ToGL, the DX9 to OpenGL layer of the native Linux and macOS builds. When ToGL cannot translate a shader, the game crashes as soon as a material using it is created (`D3DToGL: GLSL translation error!`).

## Usage

```
gm_shadercompat [--wine <command>] [--glsl <dir>] <file.vcs|file.hlsl|dir>...
```

- `.vcs`: checks every static and dynamic combo in the file.
- `.hlsl`: compiles it first with the bundled `ShaderCompile.exe` and `d3dcompiler_47.dll`, the same way gm_autoshader does (`/O 3`, `-ver 30` for names ending in `30`, `20b` otherwise). Includes resolve from the file's directory. Outside Windows it runs through `wine`, or the command given with `--wine`.
- A directory checks every `.vcs` and `.hlsl` directly inside it.
- `--glsl <dir>` writes the GLSL ToGL produced for each combo.

Exit code: 0 when everything passes, 1 when a shader fails, 2 when a file could not be read or compiled.

```
FAIL  shaders/fxc/borealis_fog_ps30.vcs
      instruction 110 `texldl`: tex3Dlod: ToGL only translates the LOD form for 2D samplers (D3DToGL: GLSL translation error!)
```

## What it checks

It runs ToGL's own translator (`togl/`) on the shader bytecode, with the options `CreatePixelShader` and `CreateVertexShader` pass. It only catches the crash: GLSL that gets rejected, or shaders that translate but render wrong, still need testing in game.

GMod's ToGL differs from Valve's public source. These GMod changes, found in `bin/linux64/libtogl_client.so`, are applied here:

- `dsx`/`dsy` translate to `dFdx`/`dFdy`.
- `sincos` only skips its two Taylor series operands for shader model 2.
- An `abs()` source with no swizzle keeps a valid register name when its LOD component is taken (`texldl`).
- The translation buffer holds 500000 bytes instead of 50000.
- Vertex shaders have 219 constant slots.

`Debugger()` is an unconditional breakpoint on Linux, so it counts as a failure.

Checked against GMod's own `libtogl_client.so` on 350k combos of GMod's stock shaders and about 800 hand-built probes covering every opcode, modifier, register type and declaration: identical GLSL and identical failures.

Things ToGL rejects include `tex3Dlod`/`texCUBElod`, `tex*grad` (`texldd`), loops compiled to `loop aL`, unconditional `break` and predicates.

## Building

Needs Rust and a C++ compiler for the vendored translator.

- Linux: `./build.sh` (g++ or clang++, or `CXX`)
- Windows: `.\build.ps1` (Visual Studio Build Tools with the "Desktop development with C++" workload, or MinGW-w64 with the GNU toolchain)

The binary lands in `target/release/`.

## License

MIT, see `LICENSE`. The third-party files below keep their own terms.

## Third-party files

- `togl/`: Valve's ToGL (github.com/ValveSoftware/ToGL at 7b64367), MIT, see `togl/LICENSE`. Patched as listed above.
- `bin/d3dcompiler_47.dll`: Microsoft's HLSL compiler, redistributable under the Windows SDK terms.
- `bin/ShaderCompile.exe`: the standalone shader compiler gm_autoshader ships.
