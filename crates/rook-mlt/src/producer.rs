//! MLT producer — media sources (files, colors, noise, text).

use std::path::Path;
use crate::{MltError, MltPtr};

use super::profile::Profile;

/// A media source: video file, audio file, image, generator (color/noise/text).
pub struct Producer {
    pub(crate) inner: MltPtr<ffi::mlt_producer_s>,
}

impl Producer {
    /// Open a media file (MLT auto-detects format via FFmpeg).
    pub fn from_file(profile: &Profile, path: &Path) -> Result<Self, MltError> {
        #[cfg(feature = "system-mlt")]
        {
            let c_path = std::ffi::CString::new(path.to_string_lossy().as_ref())
                .map_err(|_| MltError::Generic("path contains null byte"))?;
            let c_service = std::ffi::CString::new("abnormal")
                .map_err(|_| MltError::Generic("invalid service name"))?;
            let ptr = unsafe {
                ffi::mlt_factory_producer(profile.as_ptr(), c_service.as_ptr(), c_path.as_ptr() as *const _)
            };
            if ptr.is_null() {
                return Err(MltError::ProducerCreationFailed(path.to_path_buf()));
            }
            Ok(Self { inner: MltPtr { ptr } })
        }
        #[cfg(not(feature = "system-mlt"))]
        {
            let _ = (profile, path);
            Err(MltError::NotInitialized)
        }
    }

    /// Create a solid-color generator.
    pub fn color(_profile: &Profile, _color_hex: &str, _duration_frames: i64) -> Result<Self, MltError> {
        #[cfg(feature = "system-mlt")]
        {
            let c_color = std::ffi::CString::new(_color_hex).map_err(|_| MltError::Generic("invalid color"))?;
            let c_service = std::ffi::CString::new("color").map_err(|_| MltError::Generic("invalid service"))?;
            let ptr = unsafe {
                ffi::mlt_factory_producer(_profile.as_ptr(), c_service.as_ptr(), c_color.as_ptr() as *const _)
            };
            if ptr.is_null() {
                return Err(MltError::Generic("color producer creation failed"));
            }
            // Set length in frames
            unsafe {
                let props = ffi::mlt_producer_properties(ptr);
                let length_key = std::ffi::CString::new("length").unwrap();
                ffi::mlt_properties_set_int(props, length_key.as_ptr(), _duration_frames as i32);
            }
            Ok(Self { inner: MltPtr { ptr } })
        }
        #[cfg(not(feature = "system-mlt"))]
        Err(MltError::NotInitialized)
    }

    /// Create a text generator (uses MLT's `qtext` or `kdenlivetitle`).
    pub fn text(_profile: &Profile, _text: &str, _duration_frames: i64) -> Result<Self, MltError> {
        #[cfg(feature = "system-mlt")]
        {
            let c_text = std::ffi::CString::new(_text).map_err(|_| MltError::Generic("invalid text"))?;
            let c_service = std::ffi::CString::new("qtext").map_err(|_| MltError::Generic("invalid service"))?;
            let ptr = unsafe {
                ffi::mlt_factory_producer(_profile.as_ptr(), c_service.as_ptr(), c_text.as_ptr() as *const _)
            };
            if ptr.is_null() {
                return Err(MltError::Generic("text producer creation failed"));
            }
            unsafe {
                let props = ffi::mlt_producer_properties(ptr);
                let length_key = std::ffi::CString::new("length").unwrap();
                ffi::mlt_properties_set_int(props, length_key.as_ptr(), _duration_frames as i32);
            }
            Ok(Self { inner: MltPtr { ptr } })
        }
        #[cfg(not(feature = "system-mlt"))]
        Err(MltError::NotInitialized)
    }

    /// Total length in frames.
    pub fn length(&self) -> i64 {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_producer_get_playtime(self.inner.ptr) as i64 }
        #[cfg(not(feature = "system-mlt"))]
        0
    }

    /// Seek to frame.
    pub fn seek(&self, frame: i64) {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_producer_seek(self.inner.ptr, frame as mlt_sys::mlt_position); }
        #[cfg(not(feature = "system-mlt"))]
        let _ = frame;
    }

    /// Get current frame position.
    pub fn position(&self) -> i64 {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_producer_position(self.inner.ptr) as i64 }
        #[cfg(not(feature = "system-mlt"))]
        0
    }

    /// Attach a filter.
    pub fn attach(&self, filter: &super::filter::Filter) {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_producer_attach(self.inner.ptr, filter.inner.ptr); }
        #[cfg(not(feature = "system-mlt"))]
        let _ = filter;
    }

    /// The underlying MLT service pointer (for tractor/playlist connection).
    pub(crate) fn as_service_ptr(&self) -> *mut ffi::mlt_service_s {
        #[cfg(feature = "system-mlt")]
        { self.inner.ptr as *mut ffi::mlt_service_s }
        #[cfg(not(feature = "system-mlt"))]
        std::ptr::null_mut()
    }

    /// Get the raw MLT producer pointer (for playlist insert/append).
    pub(crate) fn as_ptr(&self) -> *mut ffi::mlt_producer_s {
        self.inner.ptr
    }

    pub(crate) fn into_raw(self) -> *mut ffi::mlt_producer_s {
        let ptr = self.inner.ptr;
        std::mem::forget(self);
        ptr
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_producer_close(self.inner.ptr); }
    }
}

unsafe impl Send for Producer {}
unsafe impl Sync for Producer {}

#[cfg(not(feature = "system-mlt"))]
pub(crate) mod ffi {
    #[allow(non_camel_case_types)]
    pub struct mlt_producer_s { _private: [u8; 0] }
    #[allow(non_camel_case_types)]
    pub struct mlt_service_s { _private: [u8; 0] }
}

#[cfg(feature = "system-mlt")]
pub(crate) mod ffi {
    pub use mlt_sys::{
        mlt_producer_s, mlt_service_s,
        mlt_factory_producer, mlt_producer_close,
        mlt_producer_get_playtime, mlt_producer_seek,
        mlt_producer_position, mlt_producer_attach,
        mlt_producer_properties, mlt_properties_set_int,
    };
}
