//! MLT tractor — multi-track timeline with transitions.

use crate::{MltError, MltPtr};
use super::profile::Profile;
use super::playlist::Playlist;
use super::transition::Transition;

pub struct Tractor {
    pub(crate) inner: MltPtr<ffi::mlt_tractor_s>,
}

impl Tractor {
    pub fn new(_profile: &Profile) -> Result<Self, MltError> {
        #[cfg(feature = "system-mlt")]
        {
            let ptr = unsafe { ffi::mlt_tractor_new() };
            if ptr.is_null() {
                return Err(MltError::Generic("tractor creation failed"));
            }
            // Set the MLT profile on the tractor
            unsafe {
                let key = std::ffi::CStr::from_bytes_with_nul(b"mlt_profile\0").unwrap();
                ffi::mlt_properties_set_data(
                    ffi::mlt_tractor_properties(ptr),
                    key.as_ptr(),
                    _profile.as_ptr() as *mut _,
                    0, None, None,
                );
            }
            Ok(Self { inner: MltPtr { ptr } })
        }
        #[cfg(not(feature = "system-mlt"))]
        {
            let _ = _profile;
            Err(MltError::NotInitialized)
        }
    }

    /// Connect a playlist (track) to this tractor at the given index.
    pub fn connect(&self, playlist: &Playlist, track_index: i32) {
        #[cfg(feature = "system-mlt")]
        unsafe {
            ffi::mlt_tractor_connect(self.inner.ptr, playlist.as_service_ptr());
            // Set the track index explicitly
            ffi::mlt_tractor_set_track(self.inner.ptr, playlist.as_producer_ptr(), track_index);
        }
        #[cfg(not(feature = "system-mlt"))]
        let _ = (playlist, track_index);
    }

    /// Plant a transition between two tracks.
    pub fn plant_transition(&self, transition: &Transition, a_track: i32, b_track: i32) {
        #[cfg(feature = "system-mlt")]
        unsafe {
            let field = ffi::mlt_tractor_field(self.inner.ptr);
            ffi::mlt_field_plant_transition(field, transition.as_ptr(), a_track, b_track);
        }
        #[cfg(not(feature = "system-mlt"))]
        let _ = (transition, a_track, b_track);
    }

    pub fn track_count(&self) -> i32 {
        #[cfg(feature = "system-mlt")]
        unsafe {
            let key = std::ffi::CStr::from_bytes_with_nul(b"tractor.count\0").unwrap();
            ffi::mlt_properties_get_int(ffi::mlt_tractor_properties(self.inner.ptr), key.as_ptr())
        }
        #[cfg(not(feature = "system-mlt"))]
        0
    }

    pub fn as_service_ptr(&self) -> *mut ffi::mlt_service_s {
        #[cfg(feature = "system-mlt")]
        { self.inner.ptr as *mut ffi::mlt_service_s }
        #[cfg(not(feature = "system-mlt"))]
        std::ptr::null_mut()
    }
}

impl Drop for Tractor {
    fn drop(&mut self) {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_tractor_close(self.inner.ptr); }
    }
}

unsafe impl Send for Tractor {}
unsafe impl Sync for Tractor {}

#[cfg(not(feature = "system-mlt"))]
mod ffi {
    pub struct mlt_tractor_s { _private: [u8; 0] }
    pub struct mlt_service_s { _private: [u8; 0] }
}

#[cfg(feature = "system-mlt")]
mod ffi {
    pub use mlt_sys::{
        mlt_tractor_s, mlt_service_s,
        mlt_tractor_new, mlt_tractor_close,
        mlt_tractor_connect, mlt_tractor_set_track,
        mlt_tractor_field, mlt_tractor_properties,
        mlt_field_plant_transition,
        mlt_properties_set_data, mlt_properties_get_int,
    };
}
