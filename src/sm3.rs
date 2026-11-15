pub struct Instruction {
	pub index: usize,
	pub start: usize,
	pub len: usize,
	pub opcode: u32,
}

const OP_COMMENT: u32 = 0xfffe;
const OP_END: u32 = 0xffff;

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

fn sampler_type(tokens: &[u32], list: &[Instruction], register: u32) -> Option<u32> {
	list.iter()
		.filter(|i| i.opcode == 31 && i.len >= 3)
		.find(|i| tokens[i.start + 2] & 0x7ff == register && register_type(tokens[i.start + 2]) == 10)
		.map(|i| (tokens[i.start + 1] >> 27) & 0xf)
}

fn register_type(token: u32) -> u32 {
	((token >> 28) & 0x7) | ((token >> 8) & 0x18)
}

pub fn hint(tokens: &[u32], list: &[Instruction], at: &Instruction, vertex: bool) -> &'static str {
	let opcode = at.opcode;
	if opcode == 95 && at.len >= 4 {
		match sampler_type(tokens, list, tokens[at.start + 3] & 0x7ff) {
			Some(SAMPLER_VOLUME) => return "tex3Dlod: ToGL only translates the LOD form for 2D samplers",
			Some(SAMPLER_CUBE) => return "texCUBElod: ToGL only translates the LOD form for 2D samplers",
			_ => {}
		}
	}

	match opcode {
		93 => "tex2Dgrad/tex3Dgrad, or a sample fxc gave explicit gradients: ToGL has no texldd",
		27 | 29 => "a loop compiled to `loop aL` instead of `rep`, usually because the counter indexes an array or constant: ToGL only translates `rep`",
		44 => "an unconditional break: ToGL only translates the compare form `break_xx`",
		94 | 96 => "a predicate register (setp, break_pred, if_pred): ToGL has no predicates",
		25 | 26 | 28 | 30 => "a subroutine call fxc did not inline",
		20..=24 => "a matrix macro instruction from mul() with a matrix",
		31 if vertex => "a sampler declared in a vertex shader: ToGL has no vertex texture fetch",
		31 => "a pixel shader input with a semantic other than COLOR or TEXCOORD",
		_ => "an operand register type or modifier ToGL does not translate",
	}
}
