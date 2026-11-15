pub struct Instruction {
	pub index: usize,
	pub start: usize,
	pub len: usize,
	pub opcode: u32,
}

const OP_COMMENT: u32 = 0xfffe;
const OP_END: u32 = 0xffff;
const PREDICATED: u32 = 0x1000_0000;

pub fn is_vertex_shader(tokens: &[u32]) -> bool {
	tokens.first().map_or(false, |t| t >> 16 == 0xfffe)
}

pub fn instructions(tokens: &[u32]) -> Vec<Instruction> {
	let mut out = Vec::new();
	let mut pos = 1;
	while pos < tokens.len() {
		let token = tokens[pos];
		let opcode = token & 0xffff;
		if opcode == OP_END {
			break;
		}

		let len = if opcode == OP_COMMENT { (token >> 16) & 0x7fff } else { (token >> 24) & 0x0f } as usize;
		if opcode != OP_COMMENT {
			out.push(Instruction { index: out.len(), start: pos, len: len + 1, opcode });
		}
		pos += len + 1;
	}

	out
}

pub const TOGL_MAX_COMMENT: usize = 0xfff;

pub fn long_comments(tokens: &[u32]) -> Vec<(usize, usize)> {
	let mut out = Vec::new();
	let mut pos = 1;
	while pos < tokens.len() {
		let token = tokens[pos];
		if token & 0xffff != OP_COMMENT {
			break;
		}
		let len = ((token >> 16) & 0x7fff) as usize;
		if len > TOGL_MAX_COMMENT {
			out.push((pos, len));
		}
		pos += len + 1;
	}
	out
}

pub fn at_token<'a>(list: &'a [Instruction], token: usize) -> Option<&'a Instruction> {
	list.iter().find(|i| token >= i.start && token < i.start + i.len)
}

pub fn name(opcode: u32) -> &'static str {
	match opcode {
		0 => "nop", 1 => "mov", 2 => "add", 3 => "sub", 4 => "mad", 5 => "mul", 6 => "rcp", 7 => "rsq",
		8 => "dp3", 9 => "dp4", 10 => "min", 11 => "max", 12 => "slt", 13 => "sge", 14 => "exp", 15 => "log",
		16 => "lit", 17 => "dst", 18 => "lrp", 19 => "frc", 20 => "m4x4", 21 => "m4x3", 22 => "m3x4", 23 => "m3x3",
		24 => "m3x2", 25 => "call", 26 => "callnz", 27 => "loop", 28 => "ret", 29 => "endloop", 30 => "label", 31 => "dcl",
		32 => "pow", 33 => "crs", 34 => "sgn", 35 => "abs", 36 => "nrm", 37 => "sincos", 38 => "rep", 39 => "endrep",
		40 => "if", 41 => "ifc", 42 => "else", 43 => "endif", 44 => "break", 45 => "breakc", 46 => "mova", 47 => "defb",
		48 => "defi", 64 => "texcoord", 65 => "texkill", 66 => "texld", 67 => "texbem", 68 => "texbeml", 69 => "texreg2ar",
		70 => "texreg2gb", 71 => "texm3x2pad", 72 => "texm3x2tex", 73 => "texm3x3pad", 74 => "texm3x3tex",
		76 => "texm3x3spec", 77 => "texm3x3vspec", 78 => "expp", 79 => "logp", 80 => "cnd", 81 => "def",
		82 => "texreg2rgb", 83 => "texdp3tex", 84 => "texm3x2depth", 85 => "texdp3", 86 => "texm3x3", 87 => "texdepth",
		88 => "cmp", 89 => "bem", 90 => "dp2add", 91 => "dsx", 92 => "dsy", 93 => "texldd", 94 => "setp", 95 => "texldl",
		96 => "breakp", 0xfffd => "phase",
		_ => "unknown",
	}
}

const SAMPLER_CUBE: u32 = 3;
const SAMPLER_VOLUME: u32 = 4;
const REG_INPUT: u32 = 1;
const REG_TEXTURE: u32 = 3;
const REG_OUTPUT: u32 = 6;
const REG_SAMPLER: u32 = 10;
const REG_PREDICATE: u32 = 19;

fn sampler_type(tokens: &[u32], list: &[Instruction], register: u32) -> Option<u32> {
	list.iter()
		.filter(|i| i.opcode == 31 && i.len >= 3)
		.find(|i| tokens[i.start + 2] & 0x7ff == register && register_type(tokens[i.start + 2]) == REG_SAMPLER)
		.map(|i| (tokens[i.start + 1] >> 27) & 0xf)
}

fn register_type(token: u32) -> u32 {
	((token >> 28) & 0x7) | ((token >> 8) & 0x18)
}

const REG_DEPTHOUT: u32 = 9;

pub fn depth_cmps<'a>(tokens: &[u32], list: &'a [Instruction]) -> Vec<&'a Instruction> {
	list.iter().filter(|i| i.opcode == 88 && i.len >= 2 && register_type(tokens[i.start + 1]) == REG_DEPTHOUT).collect()
}

fn usage_name(usage: u32) -> &'static str {
	match usage {
		0 => "POSITION", 1 => "BLENDWEIGHT", 2 => "BLENDINDICES", 3 => "NORMAL", 4 => "PSIZE", 5 => "TEXCOORD",
		6 => "TANGENT", 7 => "BINORMAL", 8 => "TESSFACTOR", 9 => "POSITIONT", 10 => "COLOR", 11 => "FOG",
		12 => "DEPTH", 13 => "SAMPLE",
		_ => "unknown",
	}
}

const VERTEX_TEXTURE_HELP: &str = "screenspace_general only binds textures to the pixel shader and ToGL rejects vertex samplers; sample in the pixel shader";

pub struct Problem {
	pub code: &'static str,
	pub message: String,
	pub help: &'static str,
	pub names: Vec<String>,
}

fn problem(code: &'static str, message: impl Into<String>, help: &'static str, names: &[&str]) -> Problem {
	Problem { code, message: message.into(), help, names: names.iter().map(|n| n.to_string()).collect() }
}

pub fn classify(tokens: &[u32], list: &[Instruction], at: &Instruction, vertex: bool, togl_message: &str) -> Problem {
	let opcode = at.opcode;
	let op = name(opcode);

	if togl_message.contains("Debugger()") {
		return problem("debugger", format!("`{op}` hits a debugger breakpoint in ToGL"), "the game stops with SIGTRAP here", &[]);
	}

	let operands = tokens.get(at.start + 1..at.start + at.len).unwrap_or(&[]);
	let literal = matches!(opcode, 31 | 47 | 48 | 81);
	if tokens[at.start] & PREDICATED != 0 || (!literal && operands.iter().any(|&t| register_type(t) == REG_PREDICATE)) {
		return problem("predicate", format!("predicated `{op}` is not supported by ToGL"), "ToGL has no predicate register (`setp`, `breakp`, `if p0`)", &[]);
	}

	if vertex && matches!(opcode, 66 | 93 | 95) {
		return problem("vertex-texture", "texture fetch in a vertex shader", VERTEX_TEXTURE_HELP, &["tex2Dlod", "tex2D"]);
	}

	if opcode == 95 && at.len >= 4 {
		match sampler_type(tokens, list, tokens[at.start + 3] & 0x7ff) {
			Some(SAMPLER_VOLUME) => return problem("tex3dlod", "`tex3Dlod` is not supported by ToGL", "ToGL only translates the LOD form for 2D samplers (`tex2Dlod`); when the volume has one mip level, `gm_shadercompat --patch-togl` can rewrite it to `texld` in a separate ToGL copy, which samples the same texels", &["tex3Dlod"]),
			Some(SAMPLER_CUBE) => return problem("texcubelod", "`texCUBElod` is not supported by ToGL", "ToGL only translates the LOD form for 2D samplers (`tex2Dlod`)", &["texCUBElod"]),
			_ => {}
		}
	}

	if opcode == 31 && at.len >= 3 {
		let usage = tokens[at.start + 1];
		let dst = tokens[at.start + 2];
		match register_type(dst) {
			REG_SAMPLER if vertex => {
				let mut found = problem("vertex-texture", "sampler declared in a vertex shader", VERTEX_TEXTURE_HELP, &["sampler"]);
				found.names.insert(0, format!("s{}", dst & 0x7ff));
				return found;
			}
			REG_INPUT | REG_OUTPUT | REG_TEXTURE => {
				let semantic = usage_name(usage & 0x1f);
				let index = (usage >> 16) & 0xf;
				return Problem {
					code: "semantic",
					message: format!("`{semantic}{index}` is not supported by ToGL in this shader stage"),
					help: "pixel shader inputs must be COLOR or TEXCOORD; vertex shaders support a fixed set of semantics",
					names: vec![format!("{semantic}{index}"), semantic.to_string()],
				};
			}
			_ => {}
		}
	}

	match opcode {
		93 => problem("texldd", "explicit-gradient sampling (`texldd`) is not supported by ToGL", "`tex*grad` and `tex2D(s, uv, ddx, ddy)` compile to `texldd`; use `tex2Dlod` or `tex2D`", &["tex2Dgrad", "tex3Dgrad", "texCUBEgrad", "tex2D", "tex3D", "texCUBE"]),
		27 | 29 => problem("loop", "loop compiled to `loop aL`, which ToGL does not translate", "ToGL only translates `rep`; fxc emits `loop aL` when the counter is used as an index", &["for", "while", "do"]),
		44 => problem("break", "unconditional `break` is not supported by ToGL", "ToGL only translates the compare form `break_xx`", &["break"]),
		94 | 96 => problem("predicate", format!("`{op}` is not supported by ToGL"), "ToGL has no predicate register (`setp`, `breakp`, `if p0`)", &[]),
		25 | 26 | 28 | 30 => problem("call", "subroutine call that fxc did not inline", "ToGL has no `call`, `label` or `ret`", &[]),
		20..=24 => problem("matrix", format!("`{op}` is not supported by ToGL"), "fxc emits it for `mul()` with a matrix; ToGL needs it as `dp4`/`dp3`", &["mul"]),
		34 => problem("sign", "`sgn` (`sign()`) is not supported by ToGL", "replace `sign(x)` with comparisons", &["sign"]),
		80 | 89 => problem("legacy", format!("`{op}` is not supported by ToGL"), "a shader model 1 instruction ToGL never translates", &[]),
		31 => problem("declaration", "declaration ToGL does not translate", "ToGL only maps the register types the Source shaders use", &[]),
		_ => problem("operand", format!("operand of `{op}` is not supported by ToGL"), "ToGL has no GLSL for this register type or source modifier (`_bias`, `_x2`, `_dz`, ...)", &[]),
	}
}
