//! Effects & filters — adapted from verbreel-state.

use serde::{Deserialize, Serialize};

use crate::ids::{EffectId, PluginId};

/// The category of a filter / effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    // ── Colour ──────────────────────────────────────────────────────
    Brightness,
    Contrast,
    Saturation,
    HueRotate,
    Exposure,
    ColorBalance,
    ColorWheels,
    ColorCurves,
    Lut3D,
    // ── Blur / sharpen ──────────────────────────────────────────────
    GaussianBlur,
    Sharpen,
    Glow,
    // ── Keying ──────────────────────────────────────────────────────
    ChromaKey,
    LumaKey,
    // ── Distortion ──────────────────────────────────────────────────
    Transform,
    Distort,
    // ── Stylise ─────────────────────────────────────────────────────
    Vignette,
    FilmGrain,
    Noise,
    // ── Overlay ─────────────────────────────────────────────────────
    TextOverlay,
    Timecode,
    // ── Audio ───────────────────────────────────────────────────────
    Eq,
    Compressor,
    Limiter,
    NoiseGate,
    Reverb,
    Delay,
    PitchShift,
    // ── Other ───────────────────────────────────────────────────────
    Custom(String),
    /// A user-installed plugin — WASM or OFX — identified by its manifest id.
    Plugin(PluginId),
}

/// An effect instance on a clip or track, with its parameter values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectInstance {
    pub id: EffectId,
    pub kind: EffectKind,
    /// Whether this effect is currently active.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Effect parameters as a JSON object (keys depend on `kind`).
    #[serde(default)]
    pub params: serde_json::Value,
}

fn default_true() -> bool { true }

impl EffectInstance {
    pub fn new(kind: EffectKind) -> Self {
        Self {
            id: EffectId::next(),
            kind,
            enabled: true,
            params: serde_json::Value::Object(Default::default()),
        }
    }

    /// Get the effect's unique identifier.
    pub fn id(&self) -> EffectId {
        self.id
    }

    pub fn with_param(mut self, key: &str, value: impl Serialize) -> Self {
        if let serde_json::Value::Object(ref mut map) = self.params {
            map.insert(key.to_string(), serde_json::to_value(value).unwrap());
        }
        self
    }

    /// Set a single parameter value (for IPC/agent updates).
    pub fn set_param(&mut self, key: &str, value: serde_json::Value) {
        if let serde_json::Value::Object(ref mut map) = self.params {
            map.insert(key.to_string(), value);
        } else {
            let mut map = serde_json::Map::new();
            map.insert(key.to_string(), value);
            self.params = serde_json::Value::Object(map);
        }
    }
}

/// Legacy alias — used in clip model.
pub type Effect = EffectInstance;
