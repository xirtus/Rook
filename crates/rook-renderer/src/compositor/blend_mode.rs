//! Blend modes for layer compositing.
//!
//! Vendored and adapted from koughen/Editor (MIT).
//! Maps to a shader code index used in `blend.wgsl`.

/// 17 Photoshop-standard blend modes for video compositing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlendMode {
    /// No blending — layer replaces anything below it.
    #[default]
    Normal,
    /// Darken: `min(base, layer)`.
    Darken,
    /// Multiply: `base * layer`.
    Multiply,
    /// Color Burn: darkens the base.
    ColorBurn,
    /// Lighten: `max(base, layer)`.
    Lighten,
    /// Screen: `1 - (1-base) * (1-layer)`.
    Screen,
    /// Plus Lighter / Linear Dodge (Add): `min(1, base + layer)`.
    PlusLighter,
    /// Color Dodge: brightens the base.
    ColorDodge,
    /// Overlay: combines Multiply and Screen.
    Overlay,
    /// Soft Light: softer version of Overlay.
    SoftLight,
    /// Hard Light: harder version of Overlay.
    HardLight,
    /// Difference: `abs(base - layer)`.
    Difference,
    /// Exclusion: lower-contrast Difference.
    Exclusion,
    /// Hue: uses layer hue, base saturation + luminosity.
    Hue,
    /// Saturation: uses layer saturation, base hue + luminosity.
    Saturation,
    /// Color: uses layer hue + saturation, base luminosity.
    Color,
    /// Luminosity: uses layer luminosity, base hue + saturation.
    Luminosity,
}

impl BlendMode {
    /// The shader code index used in `blend.wgsl`'s `blend_rgb` switch.
    #[inline]
    pub fn shader_code(self) -> u32 {
        match self {
            Self::Normal => 0,
            Self::Darken => 1,
            Self::Multiply => 2,
            Self::ColorBurn => 3,
            Self::Lighten => 4,
            Self::Screen => 5,
            Self::PlusLighter => 6,
            Self::ColorDodge => 7,
            Self::Overlay => 8,
            Self::SoftLight => 9,
            Self::HardLight => 10,
            Self::Difference => 11,
            Self::Exclusion => 12,
            Self::Hue => 13,
            Self::Saturation => 14,
            Self::Color => 15,
            Self::Luminosity => 16,
        }
    }

    /// Convert from Rook's blend mode index to the full 17-mode set.
    /// Index matches the order in rook-core::clip::BlendMode.
    pub fn from_rook_mode(mode: u8) -> Self {
        match mode {
            0 => Self::Normal,
            1 => Self::Darken,
            2 => Self::Multiply,
            3 => Self::ColorBurn,
            4 => Self::Lighten,
            5 => Self::Screen,
            6 => Self::PlusLighter,
            7 => Self::ColorDodge,
            8 => Self::Overlay,
            9 => Self::SoftLight,
            10 => Self::HardLight,
            11 => Self::Difference,
            12 => Self::Exclusion,
            13 => Self::Hue,
            14 => Self::Saturation,
            15 => Self::Color,
            16 => Self::Luminosity,
            _ => Self::Normal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_codes_are_unique_and_stable() {
        // Verify all variants produce a code
        let modes = [
            BlendMode::Normal,
            BlendMode::Darken,
            BlendMode::Multiply,
            BlendMode::ColorBurn,
            BlendMode::Lighten,
            BlendMode::Screen,
            BlendMode::PlusLighter,
            BlendMode::ColorDodge,
            BlendMode::Overlay,
            BlendMode::SoftLight,
            BlendMode::HardLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ];
        // Each mode must map to its expected code (index-based)
        for (i, mode) in modes.iter().enumerate() {
            assert_eq!(mode.shader_code(), i as u32);
        }
    }
}
