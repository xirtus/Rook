//! Rook plugin host — WASM sandbox + OFX loader.
//!
//! Feature flags:
//!   `wasm` — enable wasmtime-backed WASM execution
//!   `ofx`  — enable libloading OFX execution
//!   `full` — both (= `wasm` + `ofx`)

pub mod error;
pub mod ofx_host;
pub mod param_map;
pub mod wasm_host;

pub use error::PluginError;
pub use ofx_host::OfxHost;
pub use wasm_host::WasmHost;

use std::path::Path;

use rook_core::plugin::{PluginManifest, PluginSource};

// ── PluginHost — unified entry point ────────────────────────────────────────

/// The single object the engine holds.  Routes WASM plugins to `WasmHost`
/// and OFX plugins to `OfxHost`.
pub struct PluginHost {
    wasm: WasmHost,
    ofx: OfxHost,
    /// Manifests discovered from the plugin search path.
    discovered: Vec<PluginManifest>,
}

impl PluginHost {
    pub fn new() -> Self {
        let wasm = WasmHost::new()
            .unwrap_or_else(|e| {
                tracing::warn!("WasmHost init failed: {e}");
                // Return a stub that will return NotAvailable on all calls
                #[cfg(feature = "wasm")]
                { panic!("WasmHost::new failed: {e}"); }
                #[cfg(not(feature = "wasm"))]
                { WasmHost::new().unwrap() }
            });
        Self {
            wasm,
            ofx: OfxHost::new(),
            discovered: Vec::new(),
        }
    }

    /// Scan `~/.local/share/Rook/plugins/{wasm,ofx}/` and return all discovered
    /// plugin manifests.  Manifests are cached in `self.discovered`.
    pub fn refresh_cache(&mut self) -> &[PluginManifest] {
        self.discovered.clear();

        let base = match dirs::data_local_dir() {
            Some(d) => d.join("Rook").join("plugins"),
            None => return &self.discovered,
        };

        // ── WASM plugins ──────────────────────────────────────────────────
        let wasm_dir = base.join("wasm");
        if let Ok(entries) = std::fs::read_dir(&wasm_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
                    match load_wasm_manifest(&path) {
                        Ok(m) => self.discovered.push(m),
                        Err(e) => tracing::warn!(path = %path.display(), "WASM manifest load failed: {e}"),
                    }
                }
            }
        }

        // ── OFX plugins ──────────────────────────────────────────────────
        let ofx_dir = base.join("ofx");
        if let Ok(entries) = std::fs::read_dir(&ofx_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let ext = path.extension().and_then(|e| e.to_str());
                if ext == Some("bundle") || ext == Some("ofx") {
                    match OfxHost::discover(&path) {
                        Ok(manifests) => self.discovered.extend(manifests),
                        Err(e) => tracing::warn!(path = %path.display(), "OFX discover failed: {e}"),
                    }
                }
            }
        }

        tracing::info!(count = self.discovered.len(), "plugin cache refreshed");
        &self.discovered
    }

    pub fn discovered(&self) -> &[PluginManifest] { &self.discovered }

    /// Execute a plugin on a single RGBA frame.
    pub fn process_frame(
        &mut self,
        manifest: &PluginManifest,
        frame_in: &[u8],
        frame_out: &mut [u8],
        width: u32,
        height: u32,
        params: &serde_json::Value,
    ) -> Result<(), PluginError> {
        if manifest.disabled {
            return Err(PluginError::AutoDisabled { count: manifest.crash_count });
        }
        param_map::validate_params(manifest, params)?;

        match &manifest.source {
            PluginSource::WasmFile(_) =>
                self.wasm.process_frame(manifest, frame_in, frame_out, width, height, params),
            PluginSource::OfxBundle(_) =>
                self.ofx.process_frame(manifest, frame_in, frame_out, width, height, params),
        }
    }
}

impl Default for PluginHost {
    fn default() -> Self { Self::new() }
}

// ── Manifest loading ─────────────────────────────────────────────────────────

/// Load a manifest for a `.wasm` file.
/// First tries a sidecar `<name>.json`; if not found, returns a minimal stub.
fn load_wasm_manifest(wasm_path: &Path) -> Result<PluginManifest, PluginError> {
    // Look for sidecar: foo.wasm → foo.json
    let sidecar = wasm_path.with_extension("json");
    if sidecar.exists() {
        let text = std::fs::read_to_string(&sidecar)?;
        let mut manifest: PluginManifest = serde_json::from_str(&text)?;
        // Override source to match actual path
        manifest.source = PluginSource::WasmFile(wasm_path.to_path_buf());
        return Ok(manifest);
    }

    // No sidecar — emit a minimal manifest so the plugin still shows in the browser.
    let name = wasm_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("wasm-plugin")
        .to_string();

    Ok(PluginManifest::new(
        name,
        "Unknown",
        "WASM plugin (no sidecar manifest)",
        rook_core::plugin::PluginCategory::Other,
        PluginSource::WasmFile(wasm_path.to_path_buf()),
    ))
}
