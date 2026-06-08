//! MLT filter — effects applied to producers or tracks.

use crate::MltPtr;

pub struct Filter {
    pub(crate) inner: MltPtr<ffi::mlt_filter_s>,
}

impl Filter {
    /// Create a new filter by service name (e.g., "greyscale", "volume", "avfilter.colorbalance").
    pub fn new(_profile: &super::profile::Profile, _service: &str) -> Result<Self, crate::MltError> {
        #[cfg(feature = "system-mlt")]
        {
            let c_service = std::ffi::CString::new(_service)
                .map_err(|_| crate::MltError::Generic("invalid filter service name"))?;
            let ptr = unsafe {
                ffi::mlt_factory_filter(_profile.as_ptr(), c_service.as_ptr(), std::ptr::null())
            };
            if ptr.is_null() {
                return Err(crate::MltError::Generic("filter creation failed"));
            }
            Ok(Self { inner: MltPtr { ptr } })
        }
        #[cfg(not(feature = "system-mlt"))]
        {
            let _ = (_profile, _service);
            Err(crate::MltError::NotInitialized)
        }
    }

    /// Get the raw filter pointer.
    pub(crate) fn as_ptr(&self) -> *mut ffi::mlt_filter_s {
        self.inner.ptr
    }
}

impl Drop for Filter {
    fn drop(&mut self) {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_filter_close(self.inner.ptr); }
    }
}

unsafe impl Send for Filter {}
unsafe impl Sync for Filter {}

#[cfg(not(feature = "system-mlt"))]
mod ffi {
    pub struct mlt_filter_s { _private: [u8; 0] }
}

#[cfg(feature = "system-mlt")]
mod ffi {
    pub use mlt_sys::{
        mlt_filter_s, mlt_filter_close,
        mlt_factory_filter,
    };
}
