//! Plugin manifest and parameter definitions — the data model shared between
//! rook-core (commands/project), rook-plugin-host (execution), and rook-ui (browser).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::PluginId;

// ── Plugin source ────────────────────────────────────────────────────────────

/// Where the plugin binary lives on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "path")]
pub enum PluginSource {
    WasmFile(PathBuf),
    OfxBundle(PathBuf),
}

impl PluginSource {
    pub fn path(&self) -> &PathBuf {
        match self {
            Self::WasmFile(p) | Self::OfxBundle(p) => p,
        }
    }

    pub fn is_wasm(&self) -> bool { matches!(self, Self::WasmFile(_)) }
    pub fn is_ofx(&self)  -> bool { matches!(self, Self::OfxBundle(_)) }
}

// ── Plugin category ──────────────────────────────────────────────────────────

/// High-level grouping used by the plugin browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCategory {
    ColorGrade,
    Keying,
    BlurSharpen,
    Stylize,
    Overlay,
    Audio,
    Other,
}

impl PluginCategory {
    pub fn label(&self) -> &str {
        match self {
            Self::ColorGrade  => "Color Grade",
            Self::Keying      => "Keying",
            Self::BlurSharpen => "Blur / Sharpen",
            Self::Stylize     => "Stylize",
            Self::Overlay     => "Overlay",
            Self::Audio       => "Audio",
            Self::Other       => "Other",
        }
    }
}

// ── Param kinds ──────────────────────────────────────────────────────────────

/// The type (and constraints) of a single plugin parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PluginParamKind {
    Float   { min: f64, max: f64, default: f64 },
    Int     { min: i64, max: i64, default: i64 },
    Bool    { default: bool },
    Color   { default: [f32; 4] },
    Choice  { choices: Vec<String>, default: usize },
    FilePath,
    Point   { default: [f64; 2] },
}

impl PluginParamKind {
    /// Return the JSON default value for this param kind.
    pub fn default_value(&self) -> serde_json::Value {
        match self {
            Self::Float  { default, .. } => serde_json::json!(*default),
            Self::Int    { default, .. } => serde_json::json!(*default),
            Self::Bool   { default }     => serde_json::json!(*default),
            Self::Color  { default }     => serde_json::json!(default),
            Self::Choice { default, .. } => serde_json::json!(*default),
            Self::FilePath               => serde_json::json!(""),
            Self::Point  { default }     => serde_json::json!(default),
        }
    }
}

// ── Param definition ─────────────────────────────────────────────────────────

/// Declaration of a single parameter exposed by a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginParamDef {
    /// Stable identifier used as the JSON key in `EffectInstance.params`.
    pub id: String,
    /// Human-readable name shown in the inspector.
    pub name: String,
    pub kind: PluginParamKind,
}

impl PluginParamDef {
    pub fn new(id: impl Into<String>, name: impl Into<String>, kind: PluginParamKind) -> Self {
        Self { id: id.into(), name: name.into(), kind }
    }
}

// ── Manifest ─────────────────────────────────────────────────────────────────

/// Everything the host needs to know about a plugin before loading it.
/// Embedded in WASM custom sections (`rook-manifest`) or a sidecar `.json`
/// next to an OFX bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: PluginId,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub category: PluginCategory,
    pub source: PluginSource,
    pub params: Vec<PluginParamDef>,
    /// Minimum Rook version this plugin requires (semver string).
    pub min_rook_version: String,
    /// Number of times this plugin has crashed (reset on reload).
    #[serde(default)]
    pub crash_count: u32,
    /// Whether this plugin has been auto-disabled after too many crashes.
    #[serde(default)]
    pub disabled: bool,
}

impl PluginManifest {
    pub fn new(
        name: impl Into<String>,
        author: impl Into<String>,
        description: impl Into<String>,
        category: PluginCategory,
        source: PluginSource,
    ) -> Self {
        Self {
            id: PluginId::next(),
            name: name.into(),
            version: "0.1.0".to_string(),
            author: author.into(),
            description: description.into(),
            category,
            source,
            params: Vec::new(),
            min_rook_version: "0.1.0".to_string(),
            crash_count: 0,
            disabled: false,
        }
    }

    pub fn with_param(mut self, param: PluginParamDef) -> Self {
        self.params.push(param);
        self
    }

    pub fn is_wasm(&self) -> bool { self.source.is_wasm() }
    pub fn is_ofx(&self)  -> bool { self.source.is_ofx() }

    /// Build a default `serde_json::Value::Object` of params for a new instance.
    pub fn default_params(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for param in &self.params {
            map.insert(param.id.clone(), param.kind.default_value());
        }
        serde_json::Value::Object(map)
    }
}
