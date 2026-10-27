//! Typed effect longhands: box shadows, filters, backdrop blur, blending and
//! mask mode. `mask-image` stays on the text path in `translate`, which owns
//! gradient parsing.

use super::{
    absolute_px, compile_filters, css_color, unsupported, FilterFn, Metrics, PaintStyle, Shadow,
};
use crate::computed::Computed;
use director_core::{animation::Animated, types::Color};
use lightningcss::{
    properties::{
        box_shadow::BoxShadow,
        effects::{Filter, FilterList},
        masking::MaskMode,
        Property,
    },
    values::percentage::NumberOrPercentage,
};
use skia_safe::BlendMode as SkBlend;

const TYPED: &[&str] = &[
    "backdrop-filter",
    "box-shadow",
    "filter",
    "mask-mode",
    "mix-blend-mode",
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
        // Lightning CSS 1.0.0-alpha.72 leaves `none` unparsed.
        Property::Unparsed(_) if name == "box-shadow" && s == "none" => {
            paint.shadows.clear();
            Ok(())
        }
        Property::BoxShadow(list, _) => list
            .iter()
            .map(|shadow| box_shadow(shadow, m, paint.color))
            .collect::<Result<_, _>>()
            .map(|shadows| paint.shadows = shadows),
        Property::Filter(list, _) => filters(list, m).map(|list| {
            paint.filter = compile_filters(&list);
            paint.filters = list;
        }),
        Property::BackdropFilter(list, _) => match list {
            FilterList::None => Ok(()),
            FilterList::Filters(list) => match list.as_slice() {
                [Filter::Blur(radius)] => {
                    absolute_px(radius, m).map(|r| paint.box_node.backdrop_blur = Animated::new(r))
                }
                _ => Err("only backdrop blur is supported".into()),
            },
        },
        Property::MixBlendMode(mode) => blend(mode).map(|mode| paint.blend_mode = mode),
        Property::MaskMode(list) => match list.as_slice() {
            [mode] => {
                paint.mask_luminance = *mode == MaskMode::Luminance;
                Ok(())
            }
            _ => Err(unsupported(name, s)),
        },
        _ => Err(unsupported(name, s)),
    };
    Some(result)
}

fn box_shadow(shadow: &BoxShadow, m: Metrics, current: Color) -> Result<Shadow, String> {
    Ok(Shadow {
        inset: shadow.inset,
        color: css_color(&shadow.color, current)?,
        blur: absolute_px(&shadow.blur, m)? / 2.0,
        x: absolute_px(&shadow.x_offset, m)?,
        y: absolute_px(&shadow.y_offset, m)?,
        spread: absolute_px(&shadow.spread, m)?,
    })
}

/// Resolve a filter list in order. Zero blur and zero hue rotation are no-ops.
fn filters(list: &FilterList<'_>, m: Metrics) -> Result<Vec<FilterFn>, String> {
    let FilterList::Filters(list) = list else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for filter in list {
        out.push(match filter {
            Filter::Blur(radius) => {
                let radius = absolute_px(radius, m)?;
                if radius == 0.0 {
                    continue;
                }
                FilterFn::Blur(radius)
            }
            Filter::Brightness(amount) => FilterFn::Brightness(scalar(amount)),
            Filter::Contrast(amount) => FilterFn::Contrast(scalar(amount)),
            Filter::Saturate(amount) => FilterFn::Saturate(scalar(amount)),
            Filter::Grayscale(amount) => FilterFn::Grayscale(unit(amount)),
            Filter::HueRotate(angle) => {
                let degrees = angle.to_degrees();
                if degrees == 0.0 {
                    continue;
                }
                FilterFn::HueRotate(degrees)
            }
            Filter::Invert(amount) => FilterFn::Invert(unit(amount)),
            Filter::Sepia(amount) => FilterFn::Sepia(unit(amount)),
            Filter::Opacity(amount) => FilterFn::Opacity(unit(amount)),
            Filter::DropShadow(shadow) => FilterFn::DropShadow {
                x: absolute_px(&shadow.x_offset, m)?,
                y: absolute_px(&shadow.y_offset, m)?,
                blur: absolute_px(&shadow.blur, m)?,
                color: css_color(&shadow.color, Color::BLACK)?,
            },
            Filter::Url(_) => return Err("unsupported filter 'url'".into()),
        });
    }
    Ok(out)
}

fn scalar(amount: &NumberOrPercentage) -> f32 {
    match amount {
        NumberOrPercentage::Number(n) => *n,
        NumberOrPercentage::Percentage(p) => p.0,
    }
}

/// Amounts outside `0..=1` clamp.
fn unit(amount: &NumberOrPercentage) -> f32 {
    scalar(amount).clamp(0.0, 1.0)
}

fn blend(mode: &lightningcss::properties::effects::BlendMode) -> Result<SkBlend, String> {
    use lightningcss::properties::effects::BlendMode as Css;
    Ok(match mode {
        Css::Normal => SkBlend::SrcOver,
        Css::Multiply => SkBlend::Multiply,
        Css::Screen => SkBlend::Screen,
        Css::Overlay => SkBlend::Overlay,
        Css::Darken => SkBlend::Darken,
        Css::Lighten => SkBlend::Lighten,
        Css::Difference => SkBlend::Difference,
        Css::Exclusion => SkBlend::Exclusion,
        _ => return Err("unsupported blend mode".into()),
    })
}
