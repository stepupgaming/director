//! Typed text longhands: font keywords, spacing, alignment, wrapping, case,
//! shadows, decoration style and color, and direction. `font-family`,
//! `font-variant-numeric`, line clamps, text stroke, `paint-order`, decoration
//! lines and thickness, and `text-underline-offset` stay on the text path in
//! `translate`.

use super::{
    absolute_px, convert_length, convert_length_with_basis, css_color, resolve, unsupported,
    ContainingBlock, Metrics, OverflowWrap, PaintStyle, TextOverflow, TextTransform,
};
use crate::computed::Computed;
use crate::decoration_marks::DecorationStyle;
use director_core::{element::TextShadow, types::Color};
use lightningcss::{
    properties::{
        font::{
            AbsoluteFontWeight, FontStretch, FontStretchKeyword, FontStyle, FontVariantCaps,
            FontWeight, LineHeight,
        },
        overflow::TextOverflow as CssTextOverflow,
        text::{
            Direction, OverflowWrap as CssOverflowWrap, Spacing, TextAlign, TextDecorationStyle,
            TextShadow as CssTextShadow, TextTransformCase, WhiteSpace, WordBreak,
        },
        Property,
    },
    values::color::CssColor,
};
use skia_safe::textlayout::TextAlign as SkAlign;

const TYPED: &[&str] = &[
    "direction",
    "font-stretch",
    "font-style",
    "font-variant-caps",
    "font-weight",
    "letter-spacing",
    "line-height",
    "overflow-wrap",
    "text-align",
    "text-decoration-color",
    "text-decoration-style",
    "text-indent",
    "text-overflow",
    "text-shadow",
    "text-transform",
    "white-space",
    "word-break",
    "word-wrap",
];

pub(super) fn apply(
    name: &str,
    entry: Computed<'_>,
    paint: &mut PaintStyle,
    m: Metrics,
    block: ContainingBlock,
) -> Option<Result<(), String>> {
    if !TYPED.contains(&name) {
        return None;
    }
    let s = entry.text;
    let result = match entry.value {
        Property::FontWeight(weight) => {
            let weight = match weight {
                FontWeight::Absolute(AbsoluteFontWeight::Normal) => Some(400),
                FontWeight::Absolute(AbsoluteFontWeight::Bold) => Some(700),
                FontWeight::Absolute(AbsoluteFontWeight::Weight(n)) => {
                    Some(n.clamp(1.0, 1000.0) as u16)
                }
                FontWeight::Bolder | FontWeight::Lighter => None,
            };
            weight
                .map(|weight| paint.font_weight = weight)
                .ok_or_else(|| unsupported(name, s))
        }
        Property::FontStyle(style) => {
            let style = match style {
                FontStyle::Normal => Some("normal"),
                FontStyle::Italic => Some("italic"),
                FontStyle::Oblique(_) => None,
            };
            style
                .map(|style| paint.font_style = style.into())
                .ok_or_else(|| "oblique font style unsupported".into())
        }
        // The `font` shorthand expands to both, so `normal` must pass.
        Property::FontStretch(stretch) => match stretch {
            FontStretch::Keyword(FontStretchKeyword::Normal) => Ok(()),
            FontStretch::Percentage(p) if p.0 == 1.0 => Ok(()),
            _ => Err("font-stretch other than normal unsupported".into()),
        },
        Property::FontVariantCaps(FontVariantCaps::Normal) => Ok(()),
        Property::FontVariantCaps(_) => Err("font-variant-caps other than normal unsupported".into()),
        Property::LetterSpacing(spacing) => match spacing {
            Spacing::Normal => {
                paint.letter_spacing = 0.0;
                Ok(())
            }
            Spacing::Length(length) => absolute_px(length, m).map(|px| paint.letter_spacing = px),
        },
        Property::LineHeight(height) => match height {
            LineHeight::Normal => Ok(()),
            LineHeight::Number(n) => {
                paint.line_height = Some(*n);
                Ok(())
            }
            LineHeight::Length(length) => convert_length(length, m)
                .map(|l| paint.line_height = Some(resolve(l, m.font_size) / m.font_size)),
        },
        Property::TextAlign(align) => {
            let align = match align {
                TextAlign::Left | TextAlign::Start => Some(SkAlign::Left),
                TextAlign::Right | TextAlign::End => Some(SkAlign::Right),
                TextAlign::Center => Some(SkAlign::Center),
                TextAlign::Justify => Some(SkAlign::Justify),
                TextAlign::MatchParent | TextAlign::JustifyAll => None,
            };
            align
                .map(|align| paint.align = align)
                .ok_or_else(|| "unsupported text-align".into())
        }
        Property::TextIndent(indent) => {
            if indent.hanging || indent.each_line {
                Err("unsupported text-indent".into())
            } else {
                convert_length_with_basis(&indent.value, m, block.width)
                    .map(|l| paint.text_indent = l)
            }
        }
        Property::TextOverflow(overflow, _) => {
            paint.text_overflow = match overflow {
                CssTextOverflow::Clip => TextOverflow::Clip,
                CssTextOverflow::Ellipsis => TextOverflow::Ellipsis,
            };
            Ok(())
        }
        Property::WhiteSpace(mode) => {
            // pre-line skips normalize so hard newlines stay. The wrap path collapses spaces.
            paint.pre_line = *mode == WhiteSpace::PreLine;
            paint.no_wrap = matches!(mode, WhiteSpace::NoWrap | WhiteSpace::Pre);
            paint.preserve_space =
                matches!(mode, WhiteSpace::Pre | WhiteSpace::PreWrap | WhiteSpace::PreLine);
            if *mode == WhiteSpace::BreakSpaces {
                Err("unsupported whitespace mode".into())
            } else {
                Ok(())
            }
        }
        Property::OverflowWrap(wrap) | Property::WordWrap(wrap) => {
            paint.overflow_wrap = match wrap {
                CssOverflowWrap::Normal => OverflowWrap::Normal,
                CssOverflowWrap::BreakWord => OverflowWrap::BreakWord,
                CssOverflowWrap::Anywhere => OverflowWrap::Anywhere,
            };
            Ok(())
        }
        Property::WordBreak(mode) => {
            paint.break_word = *mode == WordBreak::BreakWord;
            paint.break_all = *mode == WordBreak::BreakAll;
            if *mode == WordBreak::KeepAll {
                Err("unsupported word-break".into())
            } else {
                Ok(())
            }
        }
        Property::TextTransform(transform) if transform.other.is_empty() => {
            paint.text_transform = match transform.case {
                TextTransformCase::None => TextTransform::None,
                TextTransformCase::Uppercase => TextTransform::Uppercase,
                TextTransformCase::Lowercase => TextTransform::Lowercase,
                TextTransformCase::Capitalize => TextTransform::Capitalize,
            };
            Ok(())
        }
        Property::TextTransform(_) => Err("unsupported text-transform".into()),
        // Lightning CSS 1.0.0-alpha.72 leaves `none` unparsed.
        Property::Unparsed(_) if name == "text-shadow" && s == "none" => {
            paint.text_shadows.clear();
            Ok(())
        }
        Property::TextShadow(list) => list
            .iter()
            .map(|shadow| text_shadow(shadow, m, paint.color))
            .collect::<Result<_, _>>()
            .map(|shadows| paint.text_shadows = shadows),
        Property::TextDecorationStyle(style, _) => {
            paint.decoration_style = match style {
                TextDecorationStyle::Solid => DecorationStyle::Solid,
                TextDecorationStyle::Double => DecorationStyle::Double,
                TextDecorationStyle::Dotted => DecorationStyle::Dotted,
                TextDecorationStyle::Dashed => DecorationStyle::Dashed,
                TextDecorationStyle::Wavy => DecorationStyle::Wavy,
            };
            Ok(())
        }
        Property::TextDecorationColor(CssColor::CurrentColor, _) => {
            paint.decoration_color = None;
            Ok(())
        }
        Property::TextDecorationColor(color, _) => {
            css_color(color, paint.color).map(|c| paint.decoration_color = Some(c))
        }
        Property::Direction(direction) => {
            paint.rtl = *direction == Direction::Rtl;
            Ok(())
        }
        _ => Err(unsupported(name, s)),
    };
    Some(result)
}

fn text_shadow(shadow: &CssTextShadow, m: Metrics, current: Color) -> Result<TextShadow, String> {
    if absolute_px(&shadow.spread, m)? != 0.0 {
        return Err("text-shadow spread is unsupported".into());
    }
    Ok(TextShadow {
        color: css_color(&shadow.color, current)?,
        blur: absolute_px(&shadow.blur, m)?.max(0.0),
        offset: (absolute_px(&shadow.x_offset, m)?, absolute_px(&shadow.y_offset, m)?),
    })
}
