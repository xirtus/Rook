//! MLT frame — access decoded audio/video data.

use crate::MltPtr;

pub struct Frame {
    pub(crate) inner: MltPtr<ffi::mlt_frame_s>,
}

impl Frame {
    /// Get RGBA image data from this frame.
    pub fn get_image(&self) -> Option<ImageData> {
        #[cfg(feature = "system-mlt")]
        unsafe {
            let mut width: i32 = 0;
            let mut height: i32 = 0;
            let mut format: u32 = ffi::mlt_image_rgba;
            let mut image: *mut u8 = std::ptr::null_mut();
            let ret = ffi::mlt_frame_get_image(
                self.inner.ptr,
                &mut image,
                &mut format,
                &mut width,
                &mut height,
                0,
            );
            if ret != 0 || image.is_null() {
                return None;
            }
            let size = (width * height * 4) as usize;
            let data = std::slice::from_raw_parts(image, size).to_vec();
            Some(ImageData {
                width: width as u32,
                height: height as u32,
                data,
                stride: width * 4,
            })
        }
        #[cfg(not(feature = "system-mlt"))]
        None
    }

    /// Get PCM audio samples from this frame.
    pub fn get_audio(&self) -> Option<AudioData> {
        #[cfg(feature = "system-mlt")]
        unsafe {
            let mut freq: i32 = 0;
            let mut channels: i32 = 0;
            let mut samples: i32 = 0;
            let mut format: u32 = ffi::mlt_audio_f32;
            let mut data: *mut std::ffi::c_void = std::ptr::null_mut();
            let ret = ffi::mlt_frame_get_audio(
                self.inner.ptr,
                &mut data,
                &mut format,
                &mut freq,
                &mut channels,
                &mut samples,
            );
            if ret != 0 || data.is_null() {
                return None;
            }
            let sample_count = samples as usize * channels as usize;
            let f32_data = std::slice::from_raw_parts(data as *const f32, sample_count).to_vec();
            Some(AudioData {
                samples: f32_data,
                frequency: freq as u32,
                channels: channels as u8,
                count: samples as u32,
            })
        }
        #[cfg(not(feature = "system-mlt"))]
        None
    }

    pub fn position(&self) -> i64 {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_frame_get_position(self.inner.ptr) as i64 }
        #[cfg(not(feature = "system-mlt"))]
        0
    }
}

pub struct ImageData {
    pub width: u32,
    pub height: u32,
    /// RGBA pixel data (width × height × 4 bytes).
    pub data: Vec<u8>,
    /// Row stride in bytes.
    pub stride: i32,
}

pub struct AudioData {
    pub samples: Vec<f32>,
    pub frequency: u32,
    pub channels: u8,
    pub count: u32,
}

impl Drop for Frame {
    fn drop(&mut self) {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_frame_close(self.inner.ptr); }
    }
}

#[cfg(not(feature = "system-mlt"))]
mod ffi {
    pub struct mlt_frame_s { _private: [u8; 0] }
}

#[cfg(feature = "system-mlt")]
mod ffi {
    pub use mlt_sys::{
        mlt_frame_s, mlt_frame_close,
        mlt_frame_get_image, mlt_frame_get_audio,
        mlt_frame_get_position,
        mlt_image_format_mlt_image_rgba as mlt_image_rgba,
        mlt_audio_format_mlt_audio_f32le as mlt_audio_f32,
    };
}
