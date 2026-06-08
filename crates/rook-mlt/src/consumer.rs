//! MLT consumer — sinks: SDL preview, avformat export, audio-only.

use std::path::PathBuf;
use crate::{MltError, MltPtr};

use super::profile::Profile;

/// Destination for rendered output.
pub enum ConsumerKind {
    /// SDL2 preview window (video + audio).
    SdlPreview,
    /// SDL2 audio-only playback.
    SdlAudio,
    /// FFmpeg export to file.
    Avformat { path: PathBuf, format: String },
}

pub struct Consumer {
    pub(crate) inner: MltPtr<ffi::mlt_consumer_s>,
}

impl Consumer {
    pub fn new(profile: &Profile, kind: ConsumerKind) -> Result<Self, MltError> {
        #[cfg(feature = "system-mlt")]
        {
            let (service, arg) = match &kind {
                ConsumerKind::SdlPreview => ("sdl2", None::<String>),
                ConsumerKind::SdlAudio => ("sdl2_audio", None),
                ConsumerKind::Avformat { path, format } => {
                    let arg_str = format!("{}:{}", format, path.to_string_lossy());
                    ("avformat", Some(arg_str))
                }
            };

            let c_service = std::ffi::CString::new(service)
                .map_err(|_| MltError::Generic("invalid consumer service name"))?;
            let c_arg = arg.as_ref().map(|a| std::ffi::CString::new(a.as_str()).unwrap());
            let ptr = unsafe {
                ffi::mlt_factory_consumer(
                    profile.as_ptr(),
                    c_service.as_ptr(),
                    c_arg.as_ref().map(|a| a.as_ptr() as *const _).unwrap_or(std::ptr::null()),
                )
            };
            if ptr.is_null() {
                return Err(MltError::ConsumerCreationFailed);
            }

            // For export, set real_time=0 (render as fast as possible)
            if matches!(kind, ConsumerKind::Avformat { .. }) {
                unsafe {
                    let props = ffi::mlt_consumer_properties(ptr);
                    let key = std::ffi::CStr::from_bytes_with_nul(b"real_time\0").unwrap();
                    ffi::mlt_properties_set_int(props, key.as_ptr(), 0);
                }
            }

            Ok(Self { inner: MltPtr { ptr } })
        }
        #[cfg(not(feature = "system-mlt"))]
        {
            let _ = (profile, kind);
            Err(MltError::NotInitialized)
        }
    }

    /// Connect this consumer to a producer/tractor service.
    pub fn connect(&self, service: *mut ffi::mlt_service_s) -> Result<(), MltError> {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_consumer_connect(self.inner.ptr, service); }
        #[cfg(not(feature = "system-mlt"))]
        let _ = service;
        Ok(())
    }

    pub fn start(&self) -> Result<(), MltError> {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_consumer_start(self.inner.ptr); }
        Ok(())
    }

    pub fn stop(&self) {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_consumer_stop(self.inner.ptr); }
    }

    pub fn is_stopped(&self) -> bool {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_consumer_is_stopped(self.inner.ptr) == 1 }
        #[cfg(not(feature = "system-mlt"))]
        true
    }
}

impl Drop for Consumer {
    fn drop(&mut self) {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_consumer_close(self.inner.ptr); }
    }
}

unsafe impl Send for Consumer {}
unsafe impl Sync for Consumer {}

#[cfg(not(feature = "system-mlt"))]
mod ffi {
    #[allow(non_camel_case_types)]
    pub struct mlt_consumer_s { _private: [u8; 0] }
    pub type mlt_service_s = super::super::producer::ffi::mlt_service_s;
}

#[cfg(feature = "system-mlt")]
mod ffi {
    pub use mlt_sys::{
        mlt_consumer_s, mlt_service_s,
        mlt_consumer_connect, mlt_consumer_start,
        mlt_consumer_stop, mlt_consumer_is_stopped,
        mlt_consumer_close, mlt_consumer_properties,
        mlt_factory_consumer, mlt_properties_set_int,
    };
}
