//! MLT playlist — an ordered sequence of clips (one track).

use crate::{MltError, MltPtr};
use super::profile::Profile;
use super::producer::Producer;

pub struct Playlist {
    pub(crate) inner: MltPtr<ffi::mlt_playlist_s>,
}

impl Playlist {
    pub fn new(_profile: &Profile) -> Result<Self, MltError> {
        #[cfg(feature = "system-mlt")]
        {
            let ptr = unsafe { ffi::mlt_playlist_init() };
            if ptr.is_null() {
                return Err(MltError::Generic("playlist creation failed"));
            }
            Ok(Self { inner: MltPtr { ptr } })
        }
        #[cfg(not(feature = "system-mlt"))]
        {
            let _ = _profile;
            Err(MltError::NotInitialized)
        }
    }

    /// Append a producer to the end of the playlist.
    pub fn append(&self, producer: &Producer, in_frame: i64, out_frame: i64) -> Result<(), MltError> {
        #[cfg(feature = "system-mlt")]
        {
            let result = unsafe {
                ffi::mlt_playlist_append_io(
                    self.inner.ptr,
                    producer.as_ptr(),
                    in_frame as i32,
                    out_frame as i32,
                )
            };
            if result < 0 {
                return Err(MltError::Generic("playlist append failed"));
            }
            Ok(())
        }
        #[cfg(not(feature = "system-mlt"))]
        {
            let _ = (producer, in_frame, out_frame);
            Err(MltError::NotInitialized)
        }
    }

    /// Insert a producer at a specific index.
    pub fn insert_at(&self, index: i32, producer: &Producer, in_frame: i64, out_frame: i64) -> Result<(), MltError> {
        #[cfg(feature = "system-mlt")]
        {
            let result = unsafe {
                ffi::mlt_playlist_insert(
                    self.inner.ptr,
                    producer.as_ptr(),
                    index,
                    in_frame as i32,
                    out_frame as i32,
                )
            };
            if result < 0 {
                return Err(MltError::Generic("playlist insert failed"));
            }
            Ok(())
        }
        #[cfg(not(feature = "system-mlt"))]
        {
            let _ = (index, producer, in_frame, out_frame);
            Err(MltError::NotInitialized)
        }
    }

    /// Remove the clip at the given index.
    pub fn remove_at(&self, index: i32) -> Result<(), MltError> {
        #[cfg(feature = "system-mlt")]
        {
            let result = unsafe { ffi::mlt_playlist_remove(self.inner.ptr, index) };
            if result != 0 {
                return Err(MltError::Generic("playlist remove failed"));
            }
            Ok(())
        }
        #[cfg(not(feature = "system-mlt"))]
        {
            let _ = index;
            Err(MltError::NotInitialized)
        }
    }

    /// Number of clips in the playlist.
    pub fn count(&self) -> i32 {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_playlist_count(self.inner.ptr) }
        #[cfg(not(feature = "system-mlt"))]
        0
    }

    /// Get the start frame of a clip.
    pub fn clip_start(&self, index: i32) -> i64 {
        #[cfg(feature = "system-mlt")]
        unsafe {
            let clip = ffi::mlt_playlist_get_clip(self.inner.ptr, index);
            if clip.is_null() { return 0; }
            let key = std::ffi::CStr::from_bytes_with_nul(b"start\0").unwrap();
            ffi::mlt_properties_get_position(ffi::mlt_producer_properties(clip), key.as_ptr()) as i64
        }
        #[cfg(not(feature = "system-mlt"))]
        { let _ = index; 0 }
    }

    /// Get the length (in frames) of a clip.
    pub fn clip_length(&self, index: i32) -> i64 {
        #[cfg(feature = "system-mlt")]
        unsafe {
            let clip = ffi::mlt_playlist_get_clip(self.inner.ptr, index);
            if clip.is_null() { return 0; }
            let key = std::ffi::CStr::from_bytes_with_nul(b"length\0").unwrap();
            ffi::mlt_properties_get_int(ffi::mlt_producer_properties(clip), key.as_ptr()) as i64
        }
        #[cfg(not(feature = "system-mlt"))]
        { let _ = index; 0 }
    }

    /// Get the MLT service pointer (for tractor connection).
    pub(crate) fn as_service_ptr(&self) -> *mut ffi::mlt_service_s {
        #[cfg(feature = "system-mlt")]
        { self.inner.ptr as *mut ffi::mlt_service_s }
        #[cfg(not(feature = "system-mlt"))]
        std::ptr::null_mut()
    }

    /// Get the MLT producer pointer (for tractor track assignment).
    pub(crate) fn as_producer_ptr(&self) -> *mut ffi::mlt_producer_s {
        #[cfg(feature = "system-mlt")]
        { self.inner.ptr as *mut ffi::mlt_producer_s }
        #[cfg(not(feature = "system-mlt"))]
        std::ptr::null_mut()
    }
}

impl Drop for Playlist {
    fn drop(&mut self) {
        #[cfg(feature = "system-mlt")]
        unsafe { ffi::mlt_playlist_close(self.inner.ptr); }
    }
}

unsafe impl Send for Playlist {}
unsafe impl Sync for Playlist {}

#[cfg(not(feature = "system-mlt"))]
pub(crate) mod ffi {
    pub struct mlt_playlist_s { _private: [u8; 0] }
    pub struct mlt_service_s { _private: [u8; 0] }
    pub struct mlt_producer_s { _private: [u8; 0] }
}

#[cfg(feature = "system-mlt")]
pub(crate) mod ffi {
    pub use mlt_sys::{
        mlt_playlist_s, mlt_service_s, mlt_producer_s,
        mlt_playlist_init, mlt_playlist_close,
        mlt_playlist_append_io, mlt_playlist_insert,
        mlt_playlist_remove, mlt_playlist_count,
        mlt_playlist_get_clip,
        mlt_producer_properties, mlt_properties_get_position,
        mlt_properties_get_int,
    };
}
