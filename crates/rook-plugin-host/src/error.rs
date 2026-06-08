use thiserror::Error;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin not found: {0}")]
    NotFound(String),

    #[error("plugin feature not compiled in (enable the 'wasm' or 'ofx' cargo feature)")]
    NotAvailable,

    #[error("WASM trap: {0}")]
    WasmTrap(String),

    #[error("WASM compile error: {0}")]
    WasmCompile(String),

    #[error("WASM memory error: {0}")]
    WasmMemory(String),

    #[error("OFX load error: {0}")]
    OfxLoad(String),

    #[error("OFX action error: {0}")]
    OfxAction(String),

    #[error("plugin crashed {count} time(s) and has been disabled")]
    AutoDisabled { count: u32 },

    #[error("param validation error: {0}")]
    ParamValidation(String),

    #[error("manifest parse error: {0}")]
    ManifestParse(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
