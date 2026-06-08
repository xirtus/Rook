//! Bridges `EffectInstance.params` (JSON) ↔ plugin-native types.
//!
//! WASM:  flattens params into a contiguous `[f32; N]` in linear memory.
//! OFX:   serialises into key=value pairs for `OfxPropertySet`.

use rook_core::plugin::{PluginManifest, PluginParamKind};

use crate::error::PluginError;

/// Validate a JSON params object against a manifest's param definitions.
/// Returns `Err` if any required param is missing or out of range.
pub fn validate_params(
    manifest: &PluginManifest,
    params: &serde_json::Value,
) -> Result<(), PluginError> {
    let obj = params.as_object()
        .ok_or_else(|| PluginError::ParamValidation("params must be a JSON object".into()))?;

    for def in &manifest.params {
        let val = match obj.get(&def.id) {
            Some(v) => v,
            None => continue, // missing params use their defaults
        };
        match &def.kind {
            PluginParamKind::Float { min, max, .. } => {
                let f = val.as_f64()
                    .ok_or_else(|| PluginError::ParamValidation(
                        format!("param '{}' must be a float", def.id)
                    ))?;
                if f < *min || f > *max {
                    return Err(PluginError::ParamValidation(
                        format!("param '{}' = {f} out of range [{min}, {max}]", def.id)
                    ));
                }
            }
            PluginParamKind::Int { min, max, .. } => {
                let i = val.as_i64()
                    .ok_or_else(|| PluginError::ParamValidation(
                        format!("param '{}' must be an integer", def.id)
                    ))?;
                if i < *min || i > *max {
                    return Err(PluginError::ParamValidation(
                        format!("param '{}' = {i} out of range [{min}, {max}]", def.id)
                    ));
                }
            }
            PluginParamKind::Choice { choices, .. } => {
                let idx = val.as_u64()
                    .ok_or_else(|| PluginError::ParamValidation(
                        format!("param '{}' must be a choice index", def.id)
                    ))? as usize;
                if idx >= choices.len() {
                    return Err(PluginError::ParamValidation(
                        format!("param '{}' index {idx} out of range (max {})", def.id, choices.len() - 1)
                    ));
                }
            }
            _ => {} // Bool, Color, FilePath, Point — no range check needed
        }
    }
    Ok(())
}

/// Flatten manifest params + current JSON values into a `Vec<f32>` suitable
/// for writing into WASM linear memory.
///
/// Layout: params are serialised in manifest declaration order.
/// Each param occupies a fixed slot:
///   Float/Int/Bool  →  1 × f32
///   Color           →  4 × f32  (RGBA)
///   Point           →  2 × f32  (XY)
///   Choice          →  1 × f32  (index cast to f32)
///   FilePath        →  0 × f32  (handled via string table, not inline)
pub fn flatten_params_to_f32(
    manifest: &PluginManifest,
    params: &serde_json::Value,
) -> Vec<f32> {
    let obj = params.as_object();
    let mut out = Vec::new();

    for def in &manifest.params {
        let val = obj.and_then(|o| o.get(&def.id));

        match &def.kind {
            PluginParamKind::Float { default, .. } => {
                let f = val.and_then(|v| v.as_f64()).unwrap_or(*default);
                out.push(f as f32);
            }
            PluginParamKind::Int { default, .. } => {
                let i = val.and_then(|v| v.as_i64()).unwrap_or(*default);
                out.push(i as f32);
            }
            PluginParamKind::Bool { default } => {
                let b = val.and_then(|v| v.as_bool()).unwrap_or(*default);
                out.push(if b { 1.0 } else { 0.0 });
            }
            PluginParamKind::Color { default } => {
                if let Some(arr) = val.and_then(|v| v.as_array()) {
                    for (i, component) in arr.iter().enumerate().take(4) {
                        let f = component.as_f64().unwrap_or(default[i] as f64);
                        out.push(f as f32);
                    }
                } else {
                    out.extend_from_slice(default);
                }
            }
            PluginParamKind::Choice { default, .. } => {
                let idx = val.and_then(|v| v.as_u64()).unwrap_or(*default as u64);
                out.push(idx as f32);
            }
            PluginParamKind::Point { default } => {
                if let Some(arr) = val.and_then(|v| v.as_array()) {
                    for (i, component) in arr.iter().enumerate().take(2) {
                        let f = component.as_f64().unwrap_or(default[i]);
                        out.push(f as f32);
                    }
                } else {
                    out.push(default[0] as f32);
                    out.push(default[1] as f32);
                }
            }
            PluginParamKind::FilePath => {
                // not encoded in the f32 table
            }
        }
    }

    out
}

/// Serialise params as OFX-style key=value string pairs for property-set injection.
pub fn params_to_ofx_kv(
    manifest: &PluginManifest,
    params: &serde_json::Value,
) -> Vec<(String, String)> {
    let obj = params.as_object();
    let mut out = Vec::new();

    for def in &manifest.params {
        let val = obj.and_then(|o| o.get(&def.id));
        let s = match val {
            Some(v) => v.to_string(),
            None => def.kind.default_value().to_string(),
        };
        out.push((def.id.clone(), s));
    }

    out
}
