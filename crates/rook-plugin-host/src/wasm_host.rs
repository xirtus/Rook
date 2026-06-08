//! wasmtime-backed WASM plugin sandbox.
//!
//! Each `.wasm` plugin exports:
//!   `process_frame(in_ptr: i32, out_ptr: i32, width: i32, height: i32,
//!                  params_ptr: i32, params_len: i32) -> i32`
//!
//! The host provides two imports:
//!   `env::log(ptr: i32, len: i32)`          — write a UTF-8 log message
//!   `env::get_time() -> f64`                 — monotonic seconds
//!
//! Fuel is metered at 50 000 000 units per frame call (~10 ms budget on a
//! modern CPU).  Memory is capped at 256 MB.  No FS, net, or thread access.

use rook_core::plugin::PluginManifest;

use crate::error::PluginError;
#[cfg(feature = "wasm")]
use crate::param_map::flatten_params_to_f32;

/// State attached to each wasmtime `Store` — holds log output for the frame.
struct HostState {
    log_buf: String,
}

/// The WASM plugin execution host.
pub struct WasmHost {
    #[cfg(feature = "wasm")]
    engine: wasmtime::Engine,
    /// Compiled module cache: path → (mtime, Module).
    /// wasmtime::Module is Arc-backed — cloning is O(1).
    /// Modules are recompiled only when the file's mtime changes.
    #[cfg(feature = "wasm")]
    module_cache: std::collections::HashMap<std::path::PathBuf, (std::time::SystemTime, wasmtime::Module)>,
}

#[cfg(not(feature = "wasm"))]
impl WasmHost {
    pub fn new() -> Result<Self, PluginError> {
        Ok(Self {})
    }

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
}

#[cfg(feature = "wasm")]
impl WasmHost {
    pub fn new() -> Result<Self, PluginError> {
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true);
        // Disable WASI — no filesystem or network access
        let engine = wasmtime::Engine::new(&config)
            .map_err(|e| PluginError::WasmCompile(e.to_string()))?;
        Ok(Self { engine, module_cache: std::collections::HashMap::new() })
    }

    /// Execute a plugin's `process_frame` on a single RGBA frame.
    ///
    /// `frame_in` and `frame_out` must both be `width * height * 4` bytes.
    pub fn process_frame(
        &mut self,
        manifest: &PluginManifest,
        frame_in: &[u8],
        frame_out: &mut [u8],
        width: u32,
        height: u32,
        params: &serde_json::Value,
    ) -> Result<(), PluginError> {
        let path = match &manifest.source {
            rook_core::plugin::PluginSource::WasmFile(p) => p.clone(),
            _ => return Err(PluginError::NotFound("not a WASM plugin".into())),
        };

        // Resolve the on-disk mtime so we only recompile when the file changes.
        let mtime = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);

        let module = if let Some((cached_mtime, m)) = self.module_cache.get(&path) {
            if *cached_mtime == mtime {
                m.clone()   // Arc-backed — O(1)
            } else {
                let bytes = std::fs::read(&path)?;
                let m = wasmtime::Module::new(&self.engine, &bytes)
                    .map_err(|e| PluginError::WasmCompile(e.to_string()))?;
                self.module_cache.insert(path.clone(), (mtime, m.clone()));
                m
            }
        } else {
            let bytes = std::fs::read(&path)?;
            let m = wasmtime::Module::new(&self.engine, &bytes)
                .map_err(|e| PluginError::WasmCompile(e.to_string()))?;
            self.module_cache.insert(path.clone(), (mtime, m.clone()));
            m
        };

        let mut store = wasmtime::Store::new(&self.engine, HostState { log_buf: String::new() });
        store.set_fuel(50_000_000)
            .map_err(|e| PluginError::WasmTrap(e.to_string()))?;

        // Linker with host imports
        let mut linker: wasmtime::Linker<HostState> = wasmtime::Linker::new(&self.engine);

        // env::log(ptr, len) — write UTF-8 message from WASM memory
        linker.func_wrap("env", "log",
            |mut caller: wasmtime::Caller<'_, HostState>, ptr: i32, len: i32| {
                let mem = match caller.get_export("memory")
                    .and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return,
                };
                // Copy the bytes into a local buffer before mutably borrowing the store.
                let msg: Vec<u8> = {
                    let data = mem.data(&caller);
                    let start = ptr as usize;
                    let end = start.saturating_add(len as usize).min(data.len());
                    data[start..end].to_vec()
                };
                if let Ok(s) = std::str::from_utf8(&msg) {
                    caller.data_mut().log_buf.push_str(s);
                    caller.data_mut().log_buf.push('\n');
                }
            },
        ).map_err(|e| PluginError::WasmCompile(e.to_string()))?;

        // env::get_time() -> f64
        linker.func_wrap("env", "get_time", || -> f64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
        }).map_err(|e| PluginError::WasmCompile(e.to_string()))?;

        let instance = linker.instantiate(&mut store, &module)
            .map_err(|e| PluginError::WasmTrap(e.to_string()))?;

        let memory = instance.get_memory(&mut store, "memory")
            .ok_or_else(|| PluginError::WasmMemory("plugin has no 'memory' export".into()))?;

        // ── Layout in WASM linear memory ─────────────────────────────────
        //   0               : frame_in  (width*height*4 bytes)
        //   frame_size      : frame_out (width*height*4 bytes)
        //   frame_size*2    : params    (N × f32)
        let frame_size = (width * height * 4) as usize;
        let param_floats = flatten_params_to_f32(manifest, params);
        let param_bytes: &[u8] = bytemuck_pod_cast(&param_floats);
        let total_needed = frame_size * 2 + param_bytes.len();

        // Grow memory if needed (page = 65536 bytes)
        let current_bytes = memory.data_size(&store);
        if current_bytes < total_needed {
            let pages_needed = (total_needed - current_bytes + 65535) / 65536;
            memory.grow(&mut store, pages_needed as u64)
                .map_err(|e| PluginError::WasmMemory(e.to_string()))?;
        }

        // Write input frame and params
        {
            let data = memory.data_mut(&mut store);
            data[..frame_size].copy_from_slice(frame_in);
            data[frame_size * 2..frame_size * 2 + param_bytes.len()]
                .copy_from_slice(param_bytes);
        }

        let in_ptr   = 0i32;
        let out_ptr  = frame_size as i32;
        let par_ptr  = (frame_size * 2) as i32;
        let par_len  = param_bytes.len() as i32;

        let func: wasmtime::TypedFunc<(i32, i32, i32, i32, i32, i32), i32> = instance
            .get_typed_func(&mut store, "process_frame")
            .map_err(|e| PluginError::WasmTrap(e.to_string()))?;

        let status = func.call(&mut store, (in_ptr, out_ptr, width as i32, height as i32, par_ptr, par_len))
            .map_err(|e| PluginError::WasmTrap(e.to_string()))?;

        if status != 0 {
            return Err(PluginError::WasmTrap(format!("process_frame returned status {status}")));
        }

        // Copy output frame back
        {
            let data = memory.data(&store);
            frame_out.copy_from_slice(&data[frame_size..frame_size * 2]);
        }

        // Log any messages the plugin produced
        let log = store.data().log_buf.trim().to_string();
        if !log.is_empty() {
            tracing::debug!(plugin = %manifest.name, "{}", log);
        }

        Ok(())
    }
}

/// Safe cast of `&[f32]` to `&[u8]` — justified because f32 has no padding
/// and any bit pattern is valid.
fn bytemuck_pod_cast(v: &[f32]) -> &[u8] {
    // SAFETY: f32 is 4 bytes with no padding; alignment of u8 is 1.
    unsafe {
        std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4)
    }
}
