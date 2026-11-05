//! `--grade`, `--grade-mix` and `--grade-intensity`: a colour grade from one
//! or two LUTs, each a preset name or a `url()` of a `.cube` file.

use crate::computed::ComputedStyle;
use director_core::grade::{parse_cube, preset, Grade, Lut3d, PRESETS};
use std::sync::Arc;

/// The grade the custom properties declare, or `None` without `--grade`.
pub(crate) fn grade(values: &ComputedStyle, assets: &crate::assets::AssetStore) -> Result<Option<Grade>, String> {
    let Some(text) = values.get("--grade").map(|s| s.trim()).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if text.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let parts = crate::css::split(text, false);
    let luts = match parts.as_slice() {
        [one] => (lut(one, assets)?, None),
        [from, to] => (lut(from, assets)?, Some(lut(to, assets)?)),
        _ => return Err(format!("--grade: expected one or two LUTs, got '{text}'")),
    };
    let amount = |name: &str, default: f32| -> Result<f32, String> {
        values.get(name).map_or(Ok(default), |s| amount(s).map_err(|e| format!("{name}: {e}")))
    };
    Ok(Some(Grade {
        from: luts.0,
        to: luts.1,
        mix: amount("--grade-mix", 0.0)?,
        intensity: amount("--grade-intensity", 1.0)?,
    }))
}

/// A number or percent, clamped to `0..=1`.
pub(crate) fn amount(s: &str) -> Result<f32, String> {
    let s = s.trim();
    let (n, scale) = match s.strip_suffix('%') {
        Some(n) => (n, 0.01),
        None => (s, 1.0),
    };
    let value: f32 = n.trim().parse().map_err(|_| format!("'{s}' is not a number or percent"))?;
    if !value.is_finite() {
        return Err(format!("'{s}' is not finite"));
    }
    Ok((value * scale).clamp(0.0, 1.0))
}

fn lut(part: &str, assets: &crate::assets::AssetStore) -> Result<Arc<Lut3d>, String> {
    let lower = part.to_ascii_lowercase();
    if lower.starts_with("url(") {
        let url = crate::compiler::url(part).map_err(|e| e.to_string())?;
        let bytes = assets.read(&url).map_err(|e| format!("--grade: {e}"))?;
        return parse_cube(&bytes).map(Arc::new).map_err(|e| format!("--grade: {url}: {e}"));
    }
    let name = part.trim_matches(|c| c == '"' || c == '\'');
    preset(name).ok_or_else(|| format!("--grade: unknown preset '{name}'; use one of {} or url(\"file.cube\")", PRESETS.join(", ")))
}
