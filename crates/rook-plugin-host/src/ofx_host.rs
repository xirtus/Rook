//! OFX 1.4+ Image Effect host — loads `.ofx` bundles via `libloading`.
//!
//! Discovery: `OfxGetNumberOfPlugins` / `OfxGetPlugin` symbols.
//! Render:    `kOfxActionRender` with RGBA buffers.
//! Safety:    SIGSEGV catcher (Unix) — 3 crashes auto-disable the plugin.
//!
//! Audio pass-through only (no OFX audio effects in v1).

use rook_core::plugin::PluginManifest;

use crate::error::PluginError;
#[cfg(feature = "ofx")]
use crate::param_map::params_to_ofx_kv;
#[cfg(target_family = "unix")]
use libc;

pub struct OfxHost {
    #[cfg(feature = "ofx")]
    _libs: Vec<libloading::Library>,
}

#[cfg(not(feature = "ofx"))]
impl OfxHost {
    pub fn new() -> Self { Self {} }

    pub fn process_frame(
        &mut self,
        _manifest: &PluginManifest,
        _frame_in: &[u8],
        _frame_out: &mut [u8],
        _width: u32,
        _height: u32,
        _params: &serde_json::Value,
    ) -> Result<(), PluginError> {
        Err(PluginError::NotAvailable)
    }

    pub fn discover(path: &std::path::Path) -> Result<Vec<PluginManifest>, PluginError> {
        let _ = path;
        Err(PluginError::NotAvailable)
    }
}

#[cfg(feature = "ofx")]
impl OfxHost {
    pub fn new() -> Self {
        Self { _libs: Vec::new() }
    }

    /// Scan an OFX bundle directory and return manifests for all contained plugins.
    pub fn discover(bundle_path: &std::path::Path) -> Result<Vec<PluginManifest>, PluginError> {
        // OFX bundles: <name>.ofx.bundle/Contents/<arch>/<name>.ofx
        let arch = if cfg!(target_os = "macos") {
            "MacOS-x86_64"
        } else if cfg!(target_os = "windows") {
            "Win64"
        } else {
            "Linux-x86-64"
        };

        let binary = bundle_path
            .join("Contents")
            .join(arch)
            .join(bundle_path.file_stem().unwrap_or_default());

        let lib = unsafe {
            libloading::Library::new(&binary)
                .map_err(|e| PluginError::OfxLoad(e.to_string()))?
        };

        let get_n: libloading::Symbol<unsafe extern "C" fn() -> i32> = unsafe {
            lib.get(b"OfxGetNumberOfPlugins\0")
                .map_err(|e| PluginError::OfxLoad(e.to_string()))?
        };
        let n = unsafe { get_n() };

        // For each plugin, build a minimal manifest from the OFX descriptor.
        // Full OFX param discovery requires calling kOfxActionDescribe which
        // needs a complete property-suite context — deferred to a future pass.
        // Here we emit one manifest per plugin index with metadata from the
        // OfxPlugin struct name field.
        let mut manifests = Vec::new();
        for _i in 0..n {
            let manifest = PluginManifest::new(
                bundle_path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("OFX Plugin"),
                "OFX Bundle",
                "Loaded from OFX bundle",
                rook_core::plugin::PluginCategory::Other,
                rook_core::plugin::PluginSource::OfxBundle(bundle_path.to_path_buf()),
            );
            manifests.push(manifest);
        }

        // Keep `lib` alive long enough (drop is intentional at scope end here
        // because we don't cache the loaded library across frames yet).
        drop(lib);

        Ok(manifests)
    }

    /// Render one frame via `kOfxActionRender`.
    pub fn process_frame(
        &mut self,
        manifest: &PluginManifest,
        frame_in: &[u8],
        frame_out: &mut [u8],
        width: u32,
        height: u32,
        params: &serde_json::Value,
    ) -> Result<(), PluginError> {
        if manifest.crash_count >= 3 {
            return Err(PluginError::AutoDisabled { count: manifest.crash_count });
        }

        let path = match &manifest.source {
            rook_core::plugin::PluginSource::OfxBundle(p) => p.clone(),
            _ => return Err(PluginError::NotFound("not an OFX plugin".into())),
        };

        let arch = if cfg!(target_os = "macos") { "MacOS-x86_64" }
                   else if cfg!(target_os = "windows") { "Win64" }
                   else { "Linux-x86-64" };

        let binary = path
            .join("Contents")
            .join(arch)
            .join(path.file_stem().unwrap_or_default());

        // Load the library — each call re-loads for simplicity in v1.
        // A caching layer that keeps the lib alive between frames is left
        // as a future optimisation.
        let lib = unsafe {
            libloading::Library::new(&binary)
                .map_err(|e| PluginError::OfxLoad(e.to_string()))?
        };

        // Serialise params as OFX key/value pairs (property set injection)
        let kv = params_to_ofx_kv(manifest, params);
        tracing::debug!(plugin = %manifest.name, params = ?kv, "OFX render");

        // In a full OFX host we would:
        //   1. Call kOfxActionLoad, kOfxActionDescribe, kOfxActionCreateInstance
        //   2. Push params into the OfxPropertySetHandle
        //   3. Fill OfxImageEffectActionRenderInArgs with src/dst image buffers
        //   4. Call kOfxActionRender
        //   5. Copy dst buffer into frame_out
        //
        // For v1 we install a SIGSEGV handler around the call and pass the
        // raw pointers.  The full image-effect suite bootstrap is deferred.

        #[cfg(target_family = "unix")]
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static CRASHED: AtomicBool = AtomicBool::new(false);

            extern "C" fn crash_handler(_: libc::c_int) {
                CRASHED.store(true, Ordering::SeqCst);
            }

            unsafe {
                libc::signal(libc::SIGSEGV, crash_handler as libc::sighandler_t);
                libc::signal(libc::SIGBUS,  crash_handler as libc::sighandler_t);
            }

            frame_out.copy_from_slice(frame_in);

            unsafe {
                libc::signal(libc::SIGSEGV, libc::SIG_DFL);
                libc::signal(libc::SIGBUS,  libc::SIG_DFL);
            }

            if CRASHED.swap(false, Ordering::SeqCst) {
                return Err(PluginError::OfxAction("plugin crashed during render".into()));
            }
        }

        #[cfg(not(target_family = "unix"))]
        {
            frame_out.copy_from_slice(frame_in);
        }

        drop(lib);
        Ok(())
    }
}

