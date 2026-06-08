//! # rook-mlt — Safe Rust bindings for MLT (Media Lovin' Toolkit).
//!
//! MLT is the battle-tested C engine that powers Kdenlive, Shotcut, and
//! OpenShot.  This crate wraps its C API in safe, idiomatic Rust.
//!
//! ## Stub mode
//!
//! Without the `system-mlt` feature, this crate compiles as a type-only
//! stub — all structs and enums are defined but the underlying C FFI
//! calls are no-ops.  This lets `rook-engine` and `rook-ui` compile
//! and pass type-checks without requiring a system MLT installation.
//!
//! Enable real MLT: `cargo build --features system-mlt`
//!
//! ## Crate structure
//!
//! * `profile` — frame rate, resolution, aspect ratio
//! * `producer` — media sources (file, color, noise, text)
//! * `consumer` — sinks (SDL preview, avformat export)
//! * `filter` — effects and filters
//! * `transition` — compositing transitions
//! * `playlist` — ordered clip sequences (a single track)
//! * `tractor` — multi-track timeline with transitions
//! * `frame` — decoded audio/video frame access
//! * `properties` — MLT property system (key-value metadata)

pub mod consumer;
pub mod filter;
pub mod frame;
pub mod playlist;
pub mod producer;
pub mod profile;
pub mod properties;
pub mod tractor;
pub mod transition;

mod error;
pub use error::MltError;

use std::sync::OnceLock;

static MLT_INITIALIZED: OnceLock<()> = OnceLock::new();

/// Initialize the MLT factory.  Safe to call multiple times — only the
/// first call runs.
pub fn init() -> Result<(), MltError> {
    MLT_INITIALIZED.get_or_init(|| {
        #[cfg(feature = "system-mlt")]
        unsafe {
            let repo = mlt_sys::mlt_factory_init(std::ptr::null());
            tracing::info!("rook-mlt: MLT factory initialized (repository: {:p})", repo);
        }
        #[cfg(not(feature = "system-mlt"))]
        tracing::info!("rook-mlt: stub mode — MLT not initialized");
    });
    Ok(())
}

/// A reference-counted MLT service pointer.
///
/// Wrapping pattern: `mlt_*_init()` → store pointer, `mlt_*_close()` on Drop.
/// MLT objects are NOT `Send`/`Sync` by default (they use internal thread-local
/// state).  For objects safe to share, see module-level docs.
pub(crate) struct MltPtr<T> {
    pub(crate) ptr: *mut T,
}
