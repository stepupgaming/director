//! Typed transform longhands. `transform-origin` and `perspective-origin` stay
//! on the text path in `translate`, which reads the z offset Lightning CSS
//! drops.

use super::{absolute_px, convert_length, unsupported, Metrics, PaintStyle};
use crate::computed::Computed;
use lightningcss::properties::{
    transform::{BackfaceVisibility, Perspective, Transform, TransformList, TransformStyle},
    Property,
};

const TYPED: &[&str] = &[
    "backface-visibility",
    "perspective",
    "transform",
    "transform-style",
];

pub(super) fn apply(
    name: &str,
    entry: Computed<'_>,
    paint: &mut PaintStyle,
    m: Metrics,
) -> Option<Result<(), String>> {
    if !TYPED.contains(&name) {
        return None;
    }
    let s = entry.text;
    let result = match entry.value {
        Property::Transform(list, _) => check(list, m).map(|_| paint.transform = list.clone()),
        Property::Perspective(perspective, _) => match perspective {
            Perspective::None => {
                paint.perspective = None;
                Ok(())
            }
            Perspective::Length(length) => {
                distance(length, m).map(|d| paint.perspective = Some(d))
            }
        },
        // Lightning CSS 1.0.0-alpha.72 leaves `preserve-3d` unparsed.
        Property::Unparsed(_) if name == "transform-style" && s == "preserve-3d" => {
            paint.preserve_3d = true;
            Ok(())
        }
        Property::TransformStyle(style, _) => {
            paint.preserve_3d = *style == TransformStyle::Preserve3d;
            Ok(())
        }
        Property::BackfaceVisibility(visibility, _) => {
            paint.backface_hidden = *visibility == BackfaceVisibility::Hidden;
            Ok(())
        }
        _ => Err(unsupported(name, s)),
    };
    Some(result)
}

/// Every length in `list` must resolve; translations along z and perspective
/// distances must be absolute.
fn check(list: &TransformList, m: Metrics) -> Result<(), String> {
    for t in &list.0 {
        match t {
            Transform::Translate(x, y) => {
                convert_length(x, m)?;
                convert_length(y, m)?;
            }
            Transform::TranslateX(x) | Transform::TranslateY(x) => {
                convert_length(x, m)?;
            }
            Transform::TranslateZ(z) => {
                absolute_px(z, m)?;
            }
            Transform::Translate3d(x, y, z) => {
                convert_length(x, m)?;
                convert_length(y, m)?;
                absolute_px(z, m)?;
            }
            Transform::Perspective(z) => {
                distance(z, m)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn distance(length: &lightningcss::values::length::Length, m: Metrics) -> Result<f32, String> {
    let distance = absolute_px(length, m)?;
    if distance <= 0.0 {
        return Err("perspective distance must be positive".into());
    }
    Ok(distance)
}
