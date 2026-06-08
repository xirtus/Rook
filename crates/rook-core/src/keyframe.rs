//! Keyframes — animated parameter values over time.  From verbreel-state.

use serde::{Deserialize, Serialize};

use crate::ids::KeyframeId;

/// Easing function for keyframe interpolation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    Hold,       // step — no interpolation
    Bezier { x1: f32, y1: f32, x2: f32, y2: f32 },
}

impl Default for Easing {
    fn default() -> Self { Self::Linear }
}

/// Which property a keyframe controls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyframeProperty {
    PositionX,
    PositionY,
    ScaleX,
    ScaleY,
    Rotation,
    Opacity,
    Volume,
    /// Generic named parameter on a filter.
    FilterParam { filter_id: crate::ids::EffectId, param: String },
}

/// A single keyframe: property value at a given frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub id: KeyframeId,
    /// Frame on the clip's local timeline (0 = clip start).
    pub at_frame: i64,
    pub property: KeyframeProperty,
    pub value: f64,
    #[serde(default)]
    pub easing: Easing,
}

impl Keyframe {
    pub fn new(at_frame: i64, property: KeyframeProperty, value: f64) -> Self {
        Self {
            id: KeyframeId::next(),
            at_frame,
            property,
            value,
            easing: Easing::default(),
        }
    }
}

/// Interpolate between two keyframes at a given frame.
pub fn interpolate(kf_from: &Keyframe, kf_to: &Keyframe, at_frame: i64) -> f64 {
    if at_frame <= kf_from.at_frame { return kf_from.value; }
    if at_frame >= kf_to.at_frame { return kf_to.value; }
    let span = (kf_to.at_frame - kf_from.at_frame) as f64;
    if span <= 0.0 { return kf_from.value; }
    let t = (at_frame - kf_from.at_frame) as f64 / span;
    let t = apply_easing(t, kf_from.easing);
    kf_from.value + (kf_to.value - kf_from.value) * t
}

fn apply_easing(t: f64, easing: Easing) -> f64 {
    match easing {
        Easing::Linear => t,
        Easing::Ease => t * t * (3.0 - 2.0 * t),           // smoothstep
        Easing::EaseIn => t * t,
        Easing::EaseOut => t * (2.0 - t),
        Easing::EaseInOut => {
            if t < 0.5 { 2.0 * t * t } else { -1.0 + (4.0 - 2.0 * t) * t }
        }
        Easing::Hold => 0.0,
        Easing::Bezier { x1, y1, x2, y2 } => {
            // Cubic bezier: solve for y given t (approximate)
            cubic_bezier_t(t, x1 as f64, y1 as f64, x2 as f64, y2 as f64)
        }
    }
}

fn cubic_bezier_t(t: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    // Newton–Raphson to invert x(t) → t_x, then evaluate y(t_x).
    let mut guess = t;
    for _ in 0..8 {
        let x = cubic_bezier_val(guess, 0.0, x1, x2, 1.0);
        let dx = cubic_bezier_deriv(guess, 0.0, x1, x2, 1.0);
        if dx.abs() < 1e-7 { break; }
        guess -= (x - t) / dx;
        guess = guess.clamp(0.0, 1.0);
    }
    cubic_bezier_val(guess, 0.0, y1, y2, 1.0)
}

fn cubic_bezier_val(t: f64, p0: f64, p1: f64, p2: f64, p3: f64) -> f64 {
    let u = 1.0 - t;
    u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3
}

fn cubic_bezier_deriv(t: f64, p0: f64, p1: f64, p2: f64, p3: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * (p1 - p0) + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (p3 - p2)
}
