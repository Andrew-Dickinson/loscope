#![allow(clippy::all)]
#![allow(warnings)]

use image::RgbaImage;
use openjpeg_sys::*;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::ptr;

struct StreamState {
    cursor: Cursor<Vec<u8>>,
}

unsafe extern "C" fn stream_read(
    buf: *mut std::os::raw::c_void,
    nb_bytes: OPJ_SIZE_T,
    user_data: *mut std::os::raw::c_void,
) -> OPJ_SIZE_T {
    let state = &mut *(user_data as *mut StreamState);
    let dst = std::slice::from_raw_parts_mut(buf as *mut u8, nb_bytes);
    match state.cursor.read(dst) {
        Ok(0) => OPJ_SIZE_T::MAX,
        Ok(n) => n,
        Err(_) => OPJ_SIZE_T::MAX,
    }
}

unsafe extern "C" fn stream_skip(
    nb_bytes: OPJ_OFF_T,
    user_data: *mut std::os::raw::c_void,
) -> OPJ_OFF_T {
    let state = &mut *(user_data as *mut StreamState);
    match state.cursor.seek(SeekFrom::Current(nb_bytes)) {
        Ok(_) => nb_bytes,
        Err(_) => -1,
    }
}

unsafe extern "C" fn stream_seek(
    nb_bytes: OPJ_OFF_T,
    user_data: *mut std::os::raw::c_void,
) -> OPJ_BOOL {
    let state = &mut *(user_data as *mut StreamState);
    match state.cursor.seek(SeekFrom::Start(nb_bytes as u64)) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}

unsafe extern "C" fn stream_free(user_data: *mut std::os::raw::c_void) {
    drop(Box::from_raw(user_data as *mut StreamState));
}

unsafe extern "C" fn error_callback(
    msg: *const std::os::raw::c_char,
    _data: *mut std::os::raw::c_void,
) {
    let s = std::ffi::CStr::from_ptr(msg).to_string_lossy();
    eprintln!("openjpeg error: {}", s);
}

unsafe extern "C" fn warning_callback(
    msg: *const std::os::raw::c_char,
    _data: *mut std::os::raw::c_void,
) {
    let s = std::ffi::CStr::from_ptr(msg).to_string_lossy();
    eprintln!("openjpeg warning: {}", s);
}

pub fn decode_jp2_region(
    data: Vec<u8>,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
) -> Result<RgbaImage, String> {
    unsafe {
        // Create codec
        let codec = opj_create_decompress(CODEC_FORMAT::OPJ_CODEC_JP2);
        if codec.is_null() {
            return Err("Failed to create codec".into());
        }
        scopeguard::defer! { opj_destroy_codec(codec); }

        opj_set_error_handler(codec, Some(error_callback), ptr::null_mut());
        opj_set_warning_handler(codec, Some(warning_callback), ptr::null_mut());

        // Set decode parameters
        let mut params: opj_dparameters_t = std::mem::zeroed();
        opj_set_default_decoder_parameters(&mut params);
        params.cp_reduce = 0;
        params.cp_layer = 30;

        if opj_setup_decoder(codec, &mut params) != 1 {
            return Err("Failed to setup decoder".into());
        }

        // Create stream
        let data_len = data.len();
        let state = Box::new(StreamState {
            cursor: Cursor::new(data),
        });
        let state_ptr = Box::into_raw(state) as *mut std::os::raw::c_void;

        let stream = opj_stream_default_create(1);
        if stream.is_null() {
            drop(Box::from_raw(state_ptr as *mut StreamState));
            return Err("Failed to create stream".into());
        }
        scopeguard::defer! { opj_stream_destroy(stream); }

        opj_stream_set_read_function(stream, Some(stream_read));
        opj_stream_set_skip_function(stream, Some(stream_skip));
        opj_stream_set_seek_function(stream, Some(stream_seek));
        opj_stream_set_user_data(stream, state_ptr, Some(stream_free));
        opj_stream_set_user_data_length(stream, data_len as OPJ_UINT64);

        // Read header
        let mut image: *mut opj_image_t = ptr::null_mut();
        if opj_read_header(stream, codec, &mut image) != 1 {
            return Err("Failed to read header".into());
        }
        scopeguard::defer! { opj_image_destroy(image); }

        // Set decode area AFTER read_header, BEFORE decode
        if opj_set_decode_area(codec, image, x0, y0, x1, y1) != 1 {
            return Err("Failed to set decode area".into());
        }

        // Decode
        if opj_decode(codec, stream, image) != 1 {
            return Err("Failed to decode".into());
        }
        if opj_end_decompress(codec, stream) != 1 {
            return Err("Failed to end decompress".into());
        }

        // Extract image data
        let img = &*image;
        let w = (img.x1 - img.x0) as u32;
        let h = (img.y1 - img.y0) as u32;
        let num_pixels = (w * h) as usize;
        let num_comps = img.numcomps as usize;

        if num_comps != 4 {
            return Err(format!("Expected 4 components (RGBA), got {}", num_comps));
        }

        let comps = std::slice::from_raw_parts(img.comps, num_comps);
        let data_r = std::slice::from_raw_parts(comps[0].data, num_pixels);
        let data_g = std::slice::from_raw_parts(comps[1].data, num_pixels);
        let data_b = std::slice::from_raw_parts(comps[2].data, num_pixels);
        let data_a = std::slice::from_raw_parts(comps[3].data, num_pixels);

        // These four buffers were allocated by the C OpenJPEG library's own malloc, not Rust's
        // allocator -- invisible to any Rust-side accounting (including a global-allocator
        // hook), which is exactly why this crate doesn't use one. We know their exact size here
        // (num_pixels * 4 bytes each, OPJ_INT32-per-sample), so charge it manually.
        crate::analysis::memory_paranoid::check(
            "decode_jp2_region::c_component_buffers",
            (num_pixels as u64) * 4 * (std::mem::size_of::<OPJ_INT32>() as u64),
        );

        let mut rgba = vec![0u8; num_pixels * 4];
        crate::analysis::memory_paranoid::check("decode_jp2_region::rgba", rgba.len() as u64);
        for i in 0..num_pixels {
            rgba[i * 4] = data_r[i].clamp(0, 255) as u8;
            rgba[i * 4 + 1] = data_g[i].clamp(0, 255) as u8;
            rgba[i * 4 + 2] = data_b[i].clamp(0, 255) as u8;
            rgba[i * 4 + 3] = data_a[i].clamp(0, 255) as u8;
        }

        RgbaImage::from_raw(w, h, rgba).ok_or_else(|| "Failed to construct RgbaImage".into())
    }
}
