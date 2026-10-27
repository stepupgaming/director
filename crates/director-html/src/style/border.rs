//! Typed border longhands: side widths, colors, line styles and corner radii.

use super::{absolute_px, convert_length, css_color, unsupported, BorderLine, Metrics, PaintStyle};
use crate::computed::Computed;
use lightningcss::properties::{
    border::{BorderSideWidth, LineStyle},
    Property,
};
use taffy::prelude::*;

const TYPED: &[&str] = &[
    "border-bottom-color",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "border-bottom-style",
    "border-bottom-width",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-top-color",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-top-style",
    "border-top-width",
];

/// Apply one border longhand. Colors are validated here and painted from the
/// computed text once the element's own `color` is known.
pub(super) fn apply(
    name: &str,
    entry: Computed<'_>,
    style: &mut Style,
    paint: &mut PaintStyle,
    m: Metrics,
) -> Option<Result<(), String>> {
    if !TYPED.contains(&name) {
        return None;
    }
    let s = entry.text;
    let result = match entry.value {
        Property::BorderTopWidth(w) => width(w, m).map(|w| style.border.top = w),
        Property::BorderRightWidth(w) => width(w, m).map(|w| style.border.right = w),
        Property::BorderBottomWidth(w) => width(w, m).map(|w| style.border.bottom = w),
        Property::BorderLeftWidth(w) => width(w, m).map(|w| style.border.left = w),
        Property::BorderTopColor(c)
        | Property::BorderRightColor(c)
        | Property::BorderBottomColor(c)
        | Property::BorderLeftColor(c) => css_color(c, paint.color).map(|_| ()),
        Property::BorderTopStyle(line) => {
            paint.border_lines[0] = line_of(line);
            Ok(())
        }
        Property::BorderRightStyle(line) => {
            paint.border_lines[1] = line_of(line);
            Ok(())
        }
        Property::BorderBottomStyle(line) => {
            paint.border_lines[2] = line_of(line);
            Ok(())
        }
        Property::BorderLeftStyle(line) => {
            paint.border_lines[3] = line_of(line);
            Ok(())
        }
        Property::BorderTopLeftRadius(size, _) => corner(size, m).map(|r| paint.corners[0] = r),
        Property::BorderTopRightRadius(size, _) => corner(size, m).map(|r| paint.corners[1] = r),
        Property::BorderBottomRightRadius(size, _) => corner(size, m).map(|r| paint.corners[2] = r),
        Property::BorderBottomLeftRadius(size, _) => corner(size, m).map(|r| paint.corners[3] = r),
        _ => Err(unsupported(name, s)),
    };
    Some(result)
}

fn width(value: &BorderSideWidth, m: Metrics) -> Result<LengthPercentage, String> {
    let px = match value {
        BorderSideWidth::Thin => 1.0,
        BorderSideWidth::Medium => 3.0,
        BorderSideWidth::Thick => 5.0,
        BorderSideWidth::Length(length) => absolute_px(length, m)?,
    };
    Ok(LengthPercentage::length(px))
}

fn line_of(style: &LineStyle) -> BorderLine {
    match style {
        LineStyle::None | LineStyle::Hidden | LineStyle::Solid => BorderLine::Solid,
        LineStyle::Dashed => BorderLine::Dashed,
        LineStyle::Dotted => BorderLine::Dotted,
        LineStyle::Double => BorderLine::Double,
        LineStyle::Groove => BorderLine::Groove,
        LineStyle::Ridge => BorderLine::Ridge,
        LineStyle::Inset => BorderLine::Inset,
        LineStyle::Outset => BorderLine::Outset,
    }
}

fn corner(
    size: &lightningcss::values::size::Size2D<lightningcss::values::length::LengthPercentage>,
    m: Metrics,
) -> Result<(LengthPercentage, LengthPercentage), String> {
    Ok((convert_length(&size.0, m)?, convert_length(&size.1, m)?))
}
