//! MLT properties — key-value metadata on any MLT object.

use crate::MltPtr;

pub struct Properties {
    pub(crate) inner: MltPtr<ffi::mlt_properties_s>,
}

impl Properties {
    /// Create a new standalone properties object.
    pub fn new() -> Self {
        #[cfg(feature = "system-mlt")]
        {
            let ptr = unsafe { ffi::mlt_properties_new() };
            Self { inner: MltPtr { ptr } }
        }
        #[cfg(not(feature = "system-mlt"))]
        {
            Self { inner: MltPtr { ptr: std::ptr::null_mut() } }
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        #[cfg(feature = "system-mlt")]
        {
            let c_key = std::ffi::CString::new(key).ok()?;
            let val = unsafe { ffi::mlt_properties_get(self.inner.ptr, c_key.as_ptr()) };
            if val.is_null() { return None; }
            let c_str = unsafe { std::ffi::CStr::from_ptr(val) };
            Some(c_str.to_string_lossy().into_owned())
        }
        #[cfg(not(feature = "system-mlt"))]
        { let _ = key; None }
    }

    pub fn set(&self, key: &str, value: &str) {
        #[cfg(feature = "system-mlt")]
        if let (Ok(c_key), Ok(c_val)) = (std::ffi::CString::new(key), std::ffi::CString::new(value)) {
            unsafe { ffi::mlt_properties_set(self.inner.ptr, c_key.as_ptr(), c_val.as_ptr()); }
        }
        #[cfg(not(feature = "system-mlt"))]
        let _ = (key, value);
    }

    pub fn get_int(&self, key: &str) -> Option<i32> {
        #[cfg(feature = "system-mlt")]
        {
            let c_key = std::ffi::CString::new(key).ok()?;
            Some(unsafe { ffi::mlt_properties_get_int(self.inner.ptr, c_key.as_ptr()) })
        }
        #[cfg(not(feature = "system-mlt"))]
        { let _ = key; None }
    }

    pub fn set_int(&self, key: &str, value: i32) {
        #[cfg(feature = "system-mlt")]
        if let Ok(c_key) = std::ffi::CString::new(key) {
            unsafe { ffi::mlt_properties_set_int(self.inner.ptr, c_key.as_ptr(), value); }
        }
        #[cfg(not(feature = "system-mlt"))]
        let _ = (key, value);
    }

    pub fn get_f64(&self, key: &str) -> Option<f64> {
        #[cfg(feature = "system-mlt")]
        {
            let c_key = std::ffi::CString::new(key).ok()?;
            Some(unsafe { ffi::mlt_properties_get_double(self.inner.ptr, c_key.as_ptr()) })
        }
        #[cfg(not(feature = "system-mlt"))]
        { let _ = key; None }
    }

    pub fn set_f64(&self, key: &str, value: f64) {
        #[cfg(feature = "system-mlt")]
        if let Ok(c_key) = std::ffi::CString::new(key) {
            unsafe { ffi::mlt_properties_set_double(self.inner.ptr, c_key.as_ptr(), value); }
        }
        #[cfg(not(feature = "system-mlt"))]
        let _ = (key, value);
    }

    /// Get the raw properties pointer.
    pub(crate) fn as_ptr(&self) -> *mut ffi::mlt_properties_s {
        self.inner.ptr
    }
}

impl Drop for Properties {
    fn drop(&mut self) {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_properties_close(self.inner.ptr); }
    }
}

#[cfg(not(feature = "system-mlt"))]
mod ffi {
    pub struct mlt_properties_s { _private: [u8; 0] }
}

#[cfg(feature = "system-mlt")]
mod ffi {
    pub use mlt_sys::{
        mlt_properties_s,
        mlt_properties_new, mlt_properties_close,
        mlt_properties_get, mlt_properties_set,
        mlt_properties_get_int, mlt_properties_set_int,
        mlt_properties_get_double, mlt_properties_set_double,
    };
}
