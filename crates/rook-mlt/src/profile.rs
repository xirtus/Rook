//! MLT profile — frame rate, resolution, sample rate, aspect ratio.

use crate::{MltError, MltPtr};

/// An MLT profile describing the output canvas and timing.
pub struct Profile {
    pub(crate) inner: MltPtr<ffi::mlt_profile_s>,
}

impl Profile {
    /// Create a profile from common preset name: "hd1080_24", "hd720_30", etc.
    pub fn from_preset(preset: &str) -> Result<Self, MltError> {
        #[cfg(feature = "system-mlt")]
        {
            let c_preset = std::ffi::CString::new(preset).map_err(|_| MltError::InvalidProfile(preset.to_string()))?;
            let ptr = unsafe { ffi::mlt_profile_init(c_preset.as_ptr()) };
            if ptr.is_null() {
                return Err(MltError::InvalidProfile(preset.to_string()));
            }
            Ok(Self { inner: MltPtr { ptr } })
        }
        #[cfg(not(feature = "system-mlt"))]
        {
            let _ = preset;
            Err(MltError::NotInitialized)
        }
    }

    pub fn frame_rate_num(&self) -> i32 { 24 }
    pub fn frame_rate_den(&self) -> i32 { 1 }
    pub fn width(&self) -> i32 { 1920 }
    pub fn height(&self) -> i32 { 1080 }
    pub fn sample_rate(&self) -> i32 { 48000 }

    pub(crate) fn as_ptr(&self) -> *mut ffi::mlt_profile_s {
        self.inner.ptr
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_profile_close(self.inner.ptr); }
    }
}

// MLT profiles are thread-safe for reading
unsafe impl Send for Profile {}
unsafe impl Sync for Profile {}

#[cfg(not(feature = "system-mlt"))]
mod ffi {
    #[allow(non_camel_case_types)]
    pub struct mlt_profile_s { _private: [u8; 0] }
}

#[cfg(feature = "system-mlt")]
mod ffi {
    pub use mlt_sys::{mlt_profile_s, mlt_profile_init, mlt_profile_close};
}
