use std::ffi::{c_char, CStr};

extern "C" {
	fn gmsc_translate(
		code: *const u32,
		options: u32,
		shadow_depth_sampler_mask: i32,
		centroid_mask: u32,
		glsl: *mut c_char,
		glsl_cap: usize,
		err: *mut c_char,
		err_cap: usize,
		err_token: *mut usize,
		truncations: *mut usize,
		truncated_size: *mut usize,
	) -> i32;
}

const OPTION_USE_ENV_PARAMS: u32 = 0x0001;
const OPTION_FIXUP_Z: u32 = 0x0002;
const OPTION_FIXUP_Y: u32 = 0x0004;
const OPTION_USER_CLIP_PLANES: u32 = 0x0008;
const OPTION_BONE_UNIFORM_BUFFER: u32 = 0x0800;

pub const SECTION_DECLARATIONS: usize = 20000;
pub const SECTION_ALU: usize = 120000;
pub const OUTPUT_SIZE: usize = 500000;

pub struct Translation {
	pub glsl: String,
	pub error: Option<(String, usize)>,
	pub truncations: usize,
	pub truncated_size: usize,
}

pub fn translate(tokens: &[u32], vertex: bool, centroid_mask: u32) -> Translation {
	let (options, shadow_mask) = if vertex {
		(OPTION_USE_ENV_PARAMS | OPTION_FIXUP_Z | OPTION_FIXUP_Y | OPTION_USER_CLIP_PLANES | OPTION_BONE_UNIFORM_BUFFER, -1)
	} else {
		(OPTION_USE_ENV_PARAMS, 0)
	};

	let mut padded = tokens.to_vec();
	padded.extend_from_slice(&[0x0000_ffff; 8]);

	let mut glsl = vec![0 as c_char; 1 << 20];
	let mut err = vec![0 as c_char; 1024];
	let mut err_token = 0usize;
	let mut truncations = 0usize;
	let mut truncated_size = 0usize;

	let status = unsafe {
		gmsc_translate(
			padded.as_ptr(),
			options,
			shadow_mask,
			centroid_mask,
			glsl.as_mut_ptr(),
			glsl.len(),
			err.as_mut_ptr(),
			err.len(),
			&mut err_token,
			&mut truncations,
			&mut truncated_size,
		)
	};

	let text = |buf: &[c_char]| unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().trim().to_string();
	Translation {
		glsl: text(&glsl),
		error: (status != 0).then(|| (text(&err), err_token)),
		truncations,
		truncated_size,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn movs(count: usize) -> Vec<u32> {
		let mut tokens = vec![0xffff_0300];
		for _ in 0..count {
			tokens.extend_from_slice(&[0x0200_0001, 0x800f_0000, 0xa0e4_0000]);
		}
		tokens.extend_from_slice(&[0x0200_0001, 0x800f_0800, 0x80e4_0000, 0x0000_ffff]);
		tokens
	}

	#[test]
	fn small_shader_fits() {
		let result = translate(&movs(100), false, 0);
		assert!(result.error.is_none());
		assert_eq!(result.truncations, 0);
	}

	#[test]
	fn alu_overflow_names_the_alu_section() {
		let result = translate(&movs(12000), false, 0);
		assert!(result.truncations > 0);
		assert_eq!(result.truncated_size, SECTION_ALU);
	}
}
