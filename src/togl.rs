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
	) -> i32;
}

const OPTION_USE_ENV_PARAMS: u32 = 0x0001;
const OPTION_FIXUP_Z: u32 = 0x0002;
const OPTION_FIXUP_Y: u32 = 0x0004;
const OPTION_USER_CLIP_PLANES: u32 = 0x0008;
const OPTION_BONE_UNIFORM_BUFFER: u32 = 0x0800;

pub struct Translation {
	pub glsl: String,
	pub error: Option<(String, usize)>,
	pub truncations: usize,
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
		)
	};

	let text = |buf: &[c_char]| unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().trim().to_string();
	Translation {
		glsl: text(&glsl),
		error: (status != 0).then(|| (text(&err), err_token)),
		truncations,
	}
}
