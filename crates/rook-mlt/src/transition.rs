//! MLT transition — compositing between two tracks.

use crate::MltPtr;

pub struct Transition {
    pub(crate) inner: MltPtr<ffi::mlt_transition_s>,
}

impl Transition {
    /// Create a new transition by service name (e.g., "luma", "mix", "composite").
    pub fn new(_profile: &super::profile::Profile, _service: &str) -> Result<Self, crate::MltError> {
        #[cfg(feature = "system-mlt")]
        {
            let c_service = std::ffi::CString::new(_service)
                .map_err(|_| crate::MltError::Generic("invalid transition service name"))?;
            let ptr = unsafe {
                ffi::mlt_factory_transition(_profile.as_ptr(), c_service.as_ptr(), std::ptr::null())
            };
            if ptr.is_null() {
                return Err(crate::MltError::Generic("transition creation failed"));
            }
            Ok(Self { inner: MltPtr { ptr } })
        }
        #[cfg(not(feature = "system-mlt"))]
        {
            let _ = (_profile, _service);
            Err(crate::MltError::NotInitialized)
        }
    }

    /// Get the raw transition pointer.
    pub(crate) fn as_ptr(&self) -> *mut ffi::mlt_transition_s {
        self.inner.ptr
    }
}

impl Drop for Transition {
    fn drop(&mut self) {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_transition_close(self.inner.ptr); }
    }
}

unsafe impl Send for Transition {}
unsafe impl Sync for Transition {}

#[cfg(not(feature = "system-mlt"))]
mod ffi {
    pub struct mlt_transition_s { _private: [u8; 0] }
}

#[cfg(feature = "system-mlt")]
mod ffi {
    pub use mlt_sys::{
        mlt_transition_s, mlt_transition_close,
        mlt_factory_transition,
    };
}
