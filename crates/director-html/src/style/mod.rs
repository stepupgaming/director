use crate::background_box::AxisRepeat;
use crate::box_edges::{parse_box_edge, BoxEdge};
use crate::computed::ComputedStyle;
use crate::css::{split, Values};
use director_core::{
    animation::Animated,
    node::BoxNode,
    types::{Color, ObjectPosition},
};
use lightningcss::{
    properties::{
        background::BackgroundSize,
        transform::{Matrix3d, Transform, TransformList},
    },
    traits::{Parse, ToCss},
    values::{
        angle::Angle,
        color::CssColor,
        length::{Length, LengthPercentage as CssLength, LengthPercentageOrAuto},
        position::{
            HorizontalPosition, HorizontalPositionKeyword, PositionComponent, VerticalPosition,
            VerticalPositionKeyword,
        },
    },
};
use skia_safe::{Matrix, M44};
use taffy::prelude::*;
use taffy::style::Overflow;

mod layout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderLine {
    Solid,
    Dashed,
    Dotted,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

pub enum CanvasTransform {
    Affine(Matrix),
    Projective(M44),
}

pub trait LengthParts {
    fn value(self) -> f32;
    fn is_percent(&self) -> bool;
}
impl LengthParts for LengthPercentage {
    fn value(self) -> f32 {
        self.into_raw().value()
    }
    fn is_percent(&self) -> bool {
        self.into_raw().tag() == taffy::style::CompactLength::PERCENT_TAG
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowWrap {
    Normal,
    BreakWord,
    Anywhere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOverflow {
    Clip,
    Ellipsis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextTransform {
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

#[derive(Debug, Clone)]
pub enum BackgroundLayer {
    Gradient(crate::gradient::NativeGradient),
    Image(skia_safe::Image),
}

#[derive(Debug, Clone, Copy)]
pub struct Metrics {
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
}

#[derive(Debug, Clone)]
pub struct PaintStyle {
    pub box_node: BoxNode,
    pub opacity: f32,
    pub visible: bool,
    pub transform: TransformList,
    pub origin: (LengthPercentage, LengthPercentage),
    /// Absolute z of `transform-origin`. Percent z stays unsupported.
    pub origin_z: f32,
    pub color: Color,
    pub font_size: f32,
    pub font_family: String,
    pub font_weight: u16,
    pub font_style: String,
    pub letter_spacing: f32,
    /// First-line `text-indent`. A percent is a fraction of the text box width.
    pub text_indent: LengthPercentage,
    /// `font-variant-numeric: tabular-nums` enables OpenType `tnum`.
    pub tabular_nums: bool,
    pub line_height: Option<f32>,
    pub align: skia_safe::textlayout::TextAlign,
    pub preserve_space: bool,
    pub no_wrap: bool,
    /// `white-space: pre-line`. Spaces and tabs collapse; hard newlines stay.
    pub pre_line: bool,
    pub overflow_wrap: OverflowWrap,
    /// `word-break: break-word`. Wrapping matches `overflow-wrap: break-word`.
    pub break_word: bool,
    /// `word-break: break-all`. An overflowing token breaks between characters.
    pub break_all: bool,
    pub text_overflow: TextOverflow,
    /// `line-clamp` and `-webkit-line-clamp`. `None` is `none`.
    pub line_clamp: Option<u32>,
    pub text_transform: TextTransform,
    pub rtl: bool,
    pub z_index: i32,
    pub layers: Vec<BackgroundLayer>,
    /// Empty lists use the initial value. A shorter list cycles by source index.
    pub background_position_x: Vec<AxisPos>,
    pub background_position_y: Vec<AxisPos>,
    pub background_size: Vec<BackgroundFit>,
    pub background_repeat: Vec<(AxisRepeat, AxisRepeat)>,
    pub background_origin: BackgroundOrigin,
    pub background_clip: BoxEdge,
    pub mask: Option<crate::gradient::NativeGradient>,
    pub mask_luminance: bool,
    pub decoration: TextLines,
    pub decoration_style: crate::decoration_marks::DecorationStyle,
    pub decoration_thickness: crate::decoration_value::DecorationThickness,
    /// `None` means `currentColor`.
    pub decoration_color: Option<Color>,
    /// Extra underline shift in px. `auto` and a missing offset are `0`.
    pub text_underline_offset: f32,
    pub object_position: ObjectPosition,
    pub corners: [(LengthPercentage, LengthPercentage); 4],
    pub border_colors: [Color; 4],
    pub border_lines: [BorderLine; 4],
    pub perspective: Option<f32>,
    pub perspective_origin: (LengthPercentage, LengthPercentage),
    pub backface_hidden: bool,
    pub preserve_3d: bool,
    /// `filter` functions in source order. `filter` is their compiled chain.
    pub filters: Vec<FilterFn>,
    pub filter: Option<skia_safe::ImageFilter>,
    pub blend_mode: skia_safe::BlendMode,
    pub shadows: Vec<Shadow>,
    /// CSS `text-shadow` list in source order: the first shadow paints on top. Empty is
    /// `none`. A non-zero spread stays an error.
    pub text_shadows: Vec<director_core::element::TextShadow>,
    /// `-webkit-text-stroke` width in px. `0` or `none` paints no stroke.
    pub text_stroke_width: f32,
    /// Stroke color. `None` is currentColor.
    pub text_stroke_color: Option<Color>,
    /// Text `paint-order`. `true` paints the stroke over the fill, the CSS `normal` order.
    pub stroke_over_fill: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AxisPos {
    pub percent: f32,
    pub length: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackgroundFit {
    Auto,
    Cover,
    Contain,
    Lengths(Option<LengthPercentage>, Option<LengthPercentage>),
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundOrigin {
    PaddingBox,
    BorderBox,
    ContentBox,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TextLines {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
}

#[derive(Debug, Clone)]
pub struct Shadow {
    pub inset: bool,
    pub color: Color,
    pub x: f32,
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
}

pub fn number(s: &str) -> Result<f32, String> {
    let n = s
        .parse::<f32>()
        .map_err(|_| format!("expected number, got '{s}'"))?;
    if n.is_finite() {
        Ok(n)
    } else {
        Err(format!("non-finite number '{s}'"))
    }
}

pub fn color(s: &str, current: Color) -> Result<Color, String> {
    let parsed = CssColor::parse_string(s).map_err(|e| format!("color '{s}': {e:?}"))?;
    css_color(&parsed, current)
}

pub(crate) fn css_color(c: &CssColor, current: Color) -> Result<Color, String> {
    if matches!(c, CssColor::CurrentColor) {
        return Ok(current);
    }
    match c
        .to_rgb()
        .map_err(|_| "color space cannot be resolved".to_string())?
    {
        CssColor::RGBA(c) => Ok(Color {
            r: c.red_f32(),
            g: c.green_f32(),
            b: c.blue_f32(),
            a: c.alpha_f32(),
        }),
        _ => Err("unresolved color".into()),
    }
}

pub fn length(s: &str, m: Metrics) -> Result<LengthPercentage, String> {
    let l = CssLength::parse_string(s).map_err(|e| format!("length '{s}': {e:?}"))?;
    convert_length(&l, m)
}

pub(crate) fn convert_length(l: &CssLength, m: Metrics) -> Result<LengthPercentage, String> {
    // No percent basis. A pure percent stays a percent, and mixed calc still fails.
    // The composition size is not the containing block on this path.
    match crate::calc_len::resolve_length(l, m, None)? {
        crate::calc_len::ResolvedLength::Length(px) => Ok(LengthPercentage::length(px)),
        crate::calc_len::ResolvedLength::Percent(fraction) => {
            Ok(LengthPercentage::percent(fraction))
        }
    }
}

fn length_on(s: &str, m: Metrics, basis: Option<f32>) -> Result<LengthPercentage, String> {
    let l = CssLength::parse_string(s).map_err(|e| format!("length '{s}': {e:?}"))?;
    convert_length_with_basis(&l, m, basis)
}

// `basis` is the containing-block axis. A pure percent stays a percent.
fn convert_length_with_basis(
    l: &CssLength,
    m: Metrics,
    basis: Option<f32>,
) -> Result<LengthPercentage, String> {
    let basis = basis.and_then(axis_basis);
    match crate::calc_len::resolve_length(l, m, basis)? {
        crate::calc_len::ResolvedLength::Length(px) => Ok(LengthPercentage::length(px)),
        crate::calc_len::ResolvedLength::Percent(fraction) => {
            Ok(LengthPercentage::percent(fraction))
        }
    }
}

pub(crate) fn axis_basis(value: f32) -> Option<f32> {
    if value.is_finite() && value > 0.0 {
        Some(value)
    } else {
        None
    }
}

/// Definite containing-block axes in px. `None` is indefinite.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ContainingBlock {
    pub width: Option<f32>,
    pub height: Option<f32>,
}

impl ContainingBlock {
    pub(crate) fn from_axes(width: f32, height: f32) -> Self {
        Self {
            width: axis_basis(width),
            height: axis_basis(height),
        }
    }
}

fn finite_px(value: f32) -> Option<f32> {
    value.is_finite().then_some(value)
}

fn dimension_px(value: Dimension, basis: Option<f32>) -> Option<f32> {
    if value.is_auto() {
        return None;
    }
    match value.tag() {
        taffy::style::CompactLength::PERCENT_TAG => {
            basis.and_then(axis_basis).map(|size| size * value.value())
        }
        taffy::style::CompactLength::LENGTH_TAG => finite_px(value.value()),
        _ => None,
    }
}

fn length_px(value: LengthPercentage, basis: Option<f32>) -> Option<f32> {
    if value.is_percent() {
        basis.and_then(axis_basis).map(|size| size * value.value())
    } else {
        finite_px(value.value())
    }
}

fn margin_px(value: LengthPercentageAuto, basis: Option<f32>) -> Option<f32> {
    if value.is_auto() {
        return Some(0.0);
    }
    let raw = value.into_raw();
    match raw.tag() {
        taffy::style::CompactLength::PERCENT_TAG => {
            basis.and_then(axis_basis).map(|size| size * raw.value())
        }
        taffy::style::CompactLength::LENGTH_TAG => finite_px(raw.value()),
        _ => None,
    }
}

fn border_px(value: LengthPercentage) -> f32 {
    if value.is_percent() {
        0.0
    } else {
        value.value().max(0.0)
    }
}

fn horizontal_padding(style: &Style, width: Option<f32>) -> Option<f32> {
    Some(length_px(style.padding.left, width)? + length_px(style.padding.right, width)?)
}

fn vertical_padding(style: &Style, width: Option<f32>) -> Option<f32> {
    Some(length_px(style.padding.top, width)? + length_px(style.padding.bottom, width)?)
}

fn clamp_axis(used: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    let mut value = used;
    if let Some(max) = max {
        value = value.min(max);
    }
    if let Some(min) = min {
        value = value.max(min);
    }
    value
}

/// Content-box size children use as their containing block.
/// Width `auto` stretches only when `stretch_width` is set. Height `auto` stays indefinite.
pub(crate) fn content_box(
    style: &Style,
    parent: ContainingBlock,
    stretch_width: bool,
) -> ContainingBlock {
    ContainingBlock {
        width: used_content_width(style, parent, stretch_width),
        height: used_content_height(style, parent),
    }
}

fn used_content_width(style: &Style, parent: ContainingBlock, stretch: bool) -> Option<f32> {
    let specified = dimension_px(style.size.width, parent.width);
    let min = dimension_px(style.min_size.width, parent.width);
    let max = dimension_px(style.max_size.width, parent.width);
    let border = border_px(style.border.left) + border_px(style.border.right);
    if let Some(specified) = specified {
        let used = clamp_axis(specified, min, max);
        let content = match style.box_sizing {
            BoxSizing::ContentBox => used,
            BoxSizing::BorderBox => {
                let pad = horizontal_padding(style, parent.width)?;
                used - pad - border
            }
        };
        return axis_basis(content);
    }
    if !stretch {
        return None;
    }
    let left = margin_px(style.margin.left, parent.width)?;
    let right = margin_px(style.margin.right, parent.width)?;
    let pad = horizontal_padding(style, parent.width)?;
    let width = parent.width?;
    let content = match style.box_sizing {
        BoxSizing::BorderBox => {
            let border_box = clamp_axis(width - left - right, min, max);
            border_box - pad - border
        }
        BoxSizing::ContentBox => clamp_axis(width - left - right - pad - border, min, max),
    };
    axis_basis(content)
}

fn used_content_height(style: &Style, parent: ContainingBlock) -> Option<f32> {
    let specified = dimension_px(style.size.height, parent.height)?;
    let min = dimension_px(style.min_size.height, parent.height);
    let max = dimension_px(style.max_size.height, parent.height);
    let used = clamp_axis(specified, min, max);
    let content = match style.box_sizing {
        BoxSizing::ContentBox => used,
        BoxSizing::BorderBox => {
            let pad = vertical_padding(style, parent.width)?;
            let border = border_px(style.border.top) + border_px(style.border.bottom);
            used - pad - border
        }
    };
    axis_basis(content)
}

/// Width stretches for a block-level static, relative, or sticky box with `width: auto`.
/// A row flex item and a grid item stay indefinite. A column flex item stretches on the cross axis.
pub(crate) fn width_stretches(values: &Values, parent: &Values) -> bool {
    if values.get("width").is_some_and(|value| value != "auto") {
        return false;
    }
    let position = values
        .get("position")
        .map(String::as_str)
        .unwrap_or("static");
    if !matches!(position, "static" | "relative" | "sticky") {
        return false;
    }
    let display = values.get("display").map(String::as_str).unwrap_or("block");
    if matches!(display, "none" | "inline" | "inline-flex" | "inline-grid") {
        return false;
    }
    let parent_display = parent.get("display").map(String::as_str).unwrap_or("block");
    if matches!(parent_display, "grid" | "inline-grid") {
        return false;
    }
    if matches!(parent_display, "flex" | "inline-flex") {
        let direction = parent
            .get("flex-direction")
            .map(String::as_str)
            .unwrap_or("row");
        if !matches!(direction, "column" | "column-reverse") {
            return false;
        }
        if values
            .get("margin-left")
            .is_some_and(|value| value == "auto")
            || values
                .get("margin-right")
                .is_some_and(|value| value == "auto")
        {
            return false;
        }
        let align = values
            .get("align-self")
            .map(String::as_str)
            .filter(|value| *value != "auto")
            .or_else(|| parent.get("align-items").map(String::as_str))
            .unwrap_or("stretch");
        return matches!(align, "stretch" | "normal");
    }
    true
}

pub(crate) fn resolve_gaps(
    style: &mut Style,
    values: &Values,
    m: Metrics,
    content: ContainingBlock,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if let Some(value) = values.get("column-gap") {
        match gap_length(value, m, content.width) {
            Ok(length) => style.gap.width = length,
            Err(error) => errors.push(format!("column-gap: {value}: {error}")),
        }
    }
    if let Some(value) = values.get("row-gap") {
        match gap_length(value, m, content.height) {
            Ok(length) => style.gap.height = length,
            Err(error) => errors.push(format!("row-gap: {value}: {error}")),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn gap_length(value: &str, m: Metrics, basis: Option<f32>) -> Result<LengthPercentage, String> {
    if value == "normal" {
        Ok(LengthPercentage::length(0.0))
    } else {
        length_on(value, m, basis)
    }
}

pub fn soft_wrap(paint: &PaintStyle) -> bool {
    paint.break_word || paint.overflow_wrap != OverflowWrap::Normal
}

fn split_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut whitespace = false;
    let mut started = false;
    for ch in text.chars() {
        if ch == '\n' {
            if started {
                tokens.push(std::mem::take(&mut current));
                started = false;
            }
            tokens.push("\n".into());
            continue;
        }
        let is_space = ch.is_whitespace();
        if started && is_space != whitespace {
            tokens.push(std::mem::take(&mut current));
        }
        whitespace = is_space;
        started = true;
        current.push(ch);
    }
    if started {
        tokens.push(current);
    }
    tokens
}

/// Collapse spaces and tabs. Hard newlines stay, including blank lines.
fn collapse_pre_line(text: &str) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    let mut at_break = true;
    for c in text.chars() {
        if c == '\n' {
            pending_space = false;
            at_break = true;
            out.push('\n');
        } else if c.is_whitespace() {
            if !at_break {
                pending_space = true;
            }
        } else {
            if pending_space {
                out.push(' ');
            }
            pending_space = false;
            at_break = false;
            out.push(c);
        }
    }
    out
}

/// Fill the current line, then later lines, breaking between characters.
fn place_break_all(
    out: &mut String,
    token: &str,
    max_width: f32,
    letter_spacing: f32,
    line_width: &mut f32,
    line_empty: &mut bool,
    width_of: &mut impl FnMut(&str) -> f32,
) {
    for ch in token.chars() {
        let piece = ch.to_string();
        let alone = width_of(&piece);
        let mut char_width = alone;
        if !*line_empty {
            char_width += letter_spacing;
        }
        if !*line_empty && *line_width + char_width > max_width {
            out.push('\n');
            *line_width = 0.0;
            *line_empty = true;
            char_width = alone;
        }
        out.push(ch);
        *line_width += char_width;
        *line_empty = false;
    }
}

/// Greedy line break. `soft` splits a token wider than `max_width`. A fitting token stays whole.
/// `break_all` also splits when the token overflows the space left on the line.
/// `pre_line` collapses spaces and tabs and keeps newlines.
#[allow(clippy::too_many_arguments)]
pub fn append_wrapped(
    out: &mut String,
    text: &str,
    max_width: f32,
    soft: bool,
    letter_spacing: f32,
    line_width: &mut f32,
    line_empty: &mut bool,
    break_all: bool,
    pre_line: bool,
    mut width_of: impl FnMut(&str) -> f32,
) {
    let collapsed;
    let text = if pre_line {
        collapsed = collapse_pre_line(text);
        collapsed.as_str()
    } else {
        text
    };
    if text.is_empty() {
        return;
    }
    if !max_width.is_finite() || max_width <= 0.0 {
        out.push_str(text);
        *line_empty = false;
        return;
    }
    for token in split_tokens(text) {
        if token == "\n" {
            out.push('\n');
            *line_width = 0.0;
            *line_empty = true;
            continue;
        }
        let natural = width_of(&token);
        let gap = if *line_empty { 0.0 } else { letter_spacing };
        if !*line_empty && *line_width + natural + gap <= max_width {
            out.push_str(&token);
            *line_width += natural + gap;
            *line_empty = false;
            continue;
        }
        let whitespace = token.chars().all(|c| c.is_whitespace());
        if break_all && !whitespace && (!*line_empty || natural > max_width) {
            place_break_all(
                out,
                &token,
                max_width,
                letter_spacing,
                line_width,
                line_empty,
                &mut width_of,
            );
            continue;
        }
        if !*line_empty {
            out.push('\n');
            *line_width = 0.0;
            *line_empty = true;
        }
        if whitespace || !soft || natural <= max_width {
            out.push_str(&token);
            *line_width = natural;
            *line_empty = token.is_empty();
            continue;
        }
        let mut chunk = String::new();
        let mut chunk_width = 0.0;
        for ch in token.chars() {
            let piece = ch.to_string();
            let mut char_width = width_of(&piece);
            if !chunk.is_empty() {
                char_width += letter_spacing;
            }
            if !chunk.is_empty() && chunk_width + char_width > max_width {
                out.push_str(&chunk);
                out.push('\n');
                chunk.clear();
                chunk_width = 0.0;
                char_width = width_of(&piece);
            }
            chunk.push(ch);
            chunk_width += char_width;
        }
        out.push_str(&chunk);
        *line_width = chunk_width;
        *line_empty = chunk.is_empty();
    }
}

pub fn transform_text(text: &str, transform: TextTransform) -> String {
    match transform {
        TextTransform::None => text.to_string(),
        TextTransform::Uppercase => text.to_uppercase(),
        TextTransform::Lowercase => text.to_lowercase(),
        TextTransform::Capitalize => {
            let mut out = String::with_capacity(text.len());
            let mut boundary = true;
            for ch in text.chars() {
                if ch.is_whitespace() {
                    boundary = true;
                    out.push(ch);
                } else if boundary {
                    boundary = false;
                    out.extend(ch.to_uppercase());
                } else {
                    out.push(ch);
                }
            }
            out
        }
    }
}

pub fn resolve(l: LengthPercentage, basis: f32) -> f32 {
    if l.is_percent() {
        l.value() * basis
    } else {
        l.value()
    }
}
pub fn px(s: &str, m: Metrics) -> Result<f32, String> {
    let l = length(s, m)?;
    if l.is_percent() {
        Err(format!("percentage not supported here: {s}"))
    } else {
        Ok(l.value())
    }
}
fn length_dimension(v: f32) -> Dimension {
    Dimension::length(v)
}

fn grid_track(s: &str, m: Metrics) -> Result<TrackSizingFunction, String> {
    match s {
        "auto" => Ok(auto()),
        "min-content" => Ok(min_content()),
        "max-content" => Ok(max_content()),
        _ if s.ends_with("fr") => Ok(fr(number(s.trim_end_matches("fr"))?)),
        _ if s.starts_with("minmax(") => {
            let args = split(&s[7..s.len() - 1], true);
            if args.len() != 2 {
                return Err("minmax requires two arguments".into());
            }
            let a = grid_track(&args[0], m)?;
            let b = grid_track(&args[1], m)?;
            Ok(minmax(a.min, b.max))
        }
        _ => {
            let v = length(s, m)?;
            Ok(if v.is_percent() {
                percent(v.value())
            } else {
                taffy::prelude::length(v.value())
            })
        }
    }
}
fn grid(s: &str, m: Metrics, columns: bool) -> Result<Vec<GridTemplateComponent<String>>, String> {
    crate::grid_template::grid_template_axis(s, m, columns)
}
fn placement(s: &str, columns: bool) -> Result<GridPlacement, String> {
    let parts = split(s.trim(), false);
    if parts.len() == 1 && parts[0] == "auto" {
        return Ok(auto());
    }
    if parts.first().is_some_and(|part| part == "span") {
        return span_placement(&parts[1..], columns);
    }
    line_placement(&parts, columns)
}

fn line_ident(token: &str) -> bool {
    token != "span" && token != "auto" && crate::grid_template::is_custom_ident(token)
}

fn line_placement(parts: &[String], columns: bool) -> Result<GridPlacement, String> {
    match parts {
        [token] => {
            if let Ok(index) = token.parse::<i16>() {
                return Ok(line(index));
            }
            if line_ident(token) {
                return named_line(columns, token, 1);
            }
        }
        [index, name] if line_ident(name) => {
            if let Ok(nth) = index.parse::<i16>() {
                return named_line(columns, name, nth);
            }
        }
        [name, index] if line_ident(name) => {
            if let Ok(nth) = index.parse::<i16>() {
                return named_line(columns, name, nth);
            }
        }
        _ => {}
    }
    Err("named grid lines require extraction".into())
}

fn span_placement(parts: &[String], columns: bool) -> Result<GridPlacement, String> {
    match parts {
        [token] => {
            if let Ok(count) = token.parse::<u16>() {
                if count == 0 {
                    return Err("named grid lines require extraction".into());
                }
                return Ok(span(count));
            }
            if line_ident(token) {
                return named_span(columns, token, 1);
            }
        }
        [index, name] if line_ident(name) => {
            if let Ok(count) = index.parse::<u16>() {
                return named_span(columns, name, count);
            }
        }
        [name, index] if line_ident(name) => {
            if let Ok(count) = index.parse::<u16>() {
                return named_span(columns, name, count);
            }
        }
        _ => {}
    }
    Err("named grid lines require extraction".into())
}

fn named_line(columns: bool, name: &str, nth: i16) -> Result<GridPlacement, String> {
    match crate::grid_template::resolve_named(columns, name, nth)? {
        crate::grid_template::NamedPlacement::Line(index) => Ok(line(index)),
        crate::grid_template::NamedPlacement::Named(name, nth) => {
            Ok(GridPlacement::NamedLine(name, nth))
        }
    }
}

fn named_span(columns: bool, name: &str, count: u16) -> Result<GridPlacement, String> {
    if count == 0 {
        return Err("named grid lines require extraction".into());
    }
    // The span is relative to the other edge. Taffy applies it after the name exists.
    let nth =
        i16::try_from(count).map_err(|_| "named grid lines require extraction".to_string())?;
    crate::grid_template::resolve_named(columns, name, nth)?;
    Ok(GridPlacement::NamedSpan(name.to_string(), count))
}

pub fn translate(
    values: &Values,
    mut m: Metrics,
    block: ContainingBlock,
    assets: &crate::assets::AssetStore,
) -> Result<(Style, PaintStyle, crate::grid_template::GridScope), Vec<String>> {
    let grid_scope =
        crate::grid_template::begin_element(assets as *const crate::assets::AssetStore as usize);
    let mut errors = Vec::new();
    let mut style = Style {
        display: Display::Block,
        box_sizing: BoxSizing::ContentBox,
        ..Style::default()
    };
    let mut paint = PaintStyle {
        box_node: BoxNode::new(),
        opacity: 1.0,
        visible: true,
        transform: TransformList::default(),
        origin: (percent(0.5_f32), percent(0.5_f32)),
        origin_z: 0.0,
        color: Color::BLACK,
        font_size: 16.0,
        font_family: "Roboto".into(),
        font_weight: 400,
        font_style: "normal".into(),
        letter_spacing: 0.0,
        text_indent: zero(),
        tabular_nums: false,
        line_height: None,
        align: skia_safe::textlayout::TextAlign::Left,
        preserve_space: false,
        no_wrap: false,
        pre_line: false,
        overflow_wrap: OverflowWrap::Normal,
        break_word: false,
        break_all: false,
        text_overflow: TextOverflow::Clip,
        line_clamp: None,
        text_transform: TextTransform::None,
        rtl: false,
        z_index: 0,
        layers: Vec::new(),
        background_position_x: Vec::new(),
        background_position_y: Vec::new(),
        background_size: Vec::new(),
        background_repeat: Vec::new(),
        background_origin: BackgroundOrigin::PaddingBox,
        background_clip: BoxEdge::BorderBox,
        mask: None,
        mask_luminance: false,
        decoration: TextLines::default(),
        decoration_style: crate::decoration_marks::DecorationStyle::Solid,
        decoration_thickness: crate::decoration_value::DecorationThickness::Auto,
        decoration_color: None,
        text_underline_offset: 0.0,
        object_position: ObjectPosition::default(),
        corners: [(zero(), zero()); 4],
        border_colors: [Color::BLACK; 4],
        border_lines: [BorderLine::Solid; 4],
        perspective: None,
        perspective_origin: (percent(0.5_f32), percent(0.5_f32)),
        backface_hidden: false,
        preserve_3d: false,
        filters: Vec::new(),
        filter: Default::default(),
        blend_mode: skia_safe::BlendMode::SrcOver,
        shadows: Vec::new(),
        text_shadows: Vec::new(),
        text_stroke_width: 0.0,
        text_stroke_color: None,
        stroke_over_fill: true,
    };
    if let Some(s) = values.get("font-size") {
        match px(s, m) {
            Ok(n) if n > 0.0 => {
                m.font_size = n;
                paint.font_size = n;
            }
            _ => errors.push(format!("font-size: unsupported '{s}'")),
        }
    }
    if let Some(s) = values.get("color") {
        match color(s, Color::BLACK) {
            Ok(c) => paint.color = c,
            Err(e) => errors.push(e),
        }
    }
    let computed = ComputedStyle::parse(values);
    for (name, value) in values {
        let s = value.as_str();
        let result: Result<(), String> = (|| {
            if let Some(entry) = computed.get(name) {
                let Some(info) = crate::property::info(name) else {
                    return Err(format!("unsupported property '{name}'"));
                };
                if info.group == crate::property::Group::Layout {
                    if let Some(result) = layout::apply(name, entry, &mut style, m, block) {
                        return result;
                    }
                }
            }
            match name.as_str() {
                n if n.starts_with("--") => {}
                "color" | "font-size" => {}
                // Resolved after the content box is known. Percent gap uses this element's size.
                "column-gap" | "row-gap" => {}
                "grid-template-columns" => {
                    let (tracks, names) =
                        crate::grid_template::install_tracks(true, grid(s, m, true)?);
                    style.grid_template_columns = tracks;
                    style.grid_template_column_names = names;
                }
                "grid-template-rows" => {
                    let (tracks, names) =
                        crate::grid_template::install_tracks(false, grid(s, m, false)?);
                    style.grid_template_rows = tracks;
                    style.grid_template_row_names = names;
                }
                "grid-auto-columns" => {
                    style.grid_auto_columns = split(s, false)
                        .iter()
                        .map(|s| grid_track(s, m))
                        .collect::<Result<_, _>>()?
                }
                "grid-auto-rows" => {
                    style.grid_auto_rows = split(s, false)
                        .iter()
                        .map(|s| grid_track(s, m))
                        .collect::<Result<_, _>>()?
                }
                "grid-column-start" => style.grid_column.start = placement(s, true)?,
                "grid-column-end" => style.grid_column.end = placement(s, true)?,
                "grid-row-start" => style.grid_row.start = placement(s, false)?,
                "grid-row-end" => style.grid_row.end = placement(s, false)?,
                "background-color" => {
                    paint.box_node.bg_color = Some(Animated::new(color(s, paint.color)?))
                }
                "background-image" => {
                    if s != "none" {
                        paint.layers = background_layers(s, m, paint.color, assets)?;
                    }
                }
                "background-position-x" => {
                    paint.background_position_x = axis_position_list(s, m, horizontal_position)?;
                }
                "background-position-y" => {
                    paint.background_position_y = axis_position_list(s, m, vertical_position)?;
                }
                "background-position" => {
                    let layers = background_position_layers(s, m)?;
                    paint.background_position_x = layers.iter().map(|(x, _)| *x).collect();
                    paint.background_position_y = layers.iter().map(|(_, y)| *y).collect();
                }
                "background-size" => paint.background_size = background_size_list(s, m)?,
                "background-repeat" => paint.background_repeat = background_repeat_list(s)?,
                "background-attachment" => {
                    if s.trim() != "scroll" {
                        return Err(format!("unsupported background-attachment '{s}'"));
                    }
                }
                "background-origin" => {
                    paint.background_origin = match s.trim() {
                        "padding-box" => BackgroundOrigin::PaddingBox,
                        "border-box" => BackgroundOrigin::BorderBox,
                        "content-box" => BackgroundOrigin::ContentBox,
                        _ => return Err(format!("unsupported background-origin '{s}'")),
                    }
                }
                "background-clip" => {
                    paint.background_clip = parse_box_edge(s)
                        .map_err(|_| format!("unsupported background-clip '{s}'"))?;
                }
                "opacity" => paint.opacity = number(s)?.clamp(0.0, 1.0),
                "visibility" => {
                    paint.visible = match s {
                        "visible" => true,
                        "hidden" => false,
                        _ => return Err("visibility collapse unsupported".into()),
                    }
                }
                "overflow-x" | "overflow-y" => {
                    let overflow = match s {
                        "visible" => Overflow::Visible,
                        "hidden" | "clip" => Overflow::Hidden,
                        _ => return Err("scrolling overflow unsupported".into()),
                    };
                    if name == "overflow-x" {
                        style.overflow.x = overflow
                    } else {
                        style.overflow.y = overflow
                    };
                    if overflow == Overflow::Hidden {
                        paint.box_node.overflow = "hidden".into();
                    }
                }
                n if n.starts_with("border-") && n.ends_with("-width") => {
                    let w = if s == "medium" {
                        3.0
                    } else if s == "thin" {
                        1.0
                    } else if s == "thick" {
                        5.0
                    } else {
                        px(s, m)?
                    };
                    let v = LengthPercentage::length(w);
                    match n {
                        "border-top-width" => style.border.top = v,
                        "border-right-width" => style.border.right = v,
                        "border-bottom-width" => style.border.bottom = v,
                        "border-left-width" => style.border.left = v,
                        _ => return Err("unsupported border width".into()),
                    }
                }
                n if n.starts_with("border-") && n.ends_with("-color") => {
                    color(s, paint.color)?;
                }
                n if n.starts_with("border-") && n.ends_with("-style") => {
                    let line = match s {
                        "none" | "hidden" | "solid" => BorderLine::Solid,
                        "dashed" => BorderLine::Dashed,
                        "dotted" => BorderLine::Dotted,
                        "double" => BorderLine::Double,
                        "groove" => BorderLine::Groove,
                        "ridge" => BorderLine::Ridge,
                        "inset" => BorderLine::Inset,
                        "outset" => BorderLine::Outset,
                        _ => return Err("only solid/dashed/dotted borders supported".into()),
                    };
                    let index = match n {
                        "border-top-style" => 0,
                        "border-right-style" => 1,
                        "border-bottom-style" => 2,
                        "border-left-style" => 3,
                        _ => return Err("unsupported border style".into()),
                    };
                    paint.border_lines[index] = line;
                }
                n if n.starts_with("border-") && n.ends_with("-radius") => {
                    let radii = split(s, false);
                    if radii.is_empty() || radii.len() > 2 {
                        return Err("invalid corner radius".into());
                    };
                    let index = match n {
                        "border-top-left-radius" => 0,
                        "border-top-right-radius" => 1,
                        "border-bottom-right-radius" => 2,
                        "border-bottom-left-radius" => 3,
                        _ => return Err("unknown border radius".into()),
                    };
                    paint.corners[index] = (
                        length(&radii[0], m)?,
                        length(radii.get(1).unwrap_or(&radii[0]), m)?,
                    );
                }
                "box-shadow" => {
                    if s != "none" {
                        paint.shadows = shadows(s, m, paint.color)?;
                    }
                }
                "filter" => {
                    if s != "none" {
                        paint.filters = filters(s, m)?;
                        paint.filter = compile_filters(&paint.filters);
                    }
                }
                "mix-blend-mode" => {
                    paint.blend_mode = match s {
                        "normal" => skia_safe::BlendMode::SrcOver,
                        "multiply" => skia_safe::BlendMode::Multiply,
                        "screen" => skia_safe::BlendMode::Screen,
                        "overlay" => skia_safe::BlendMode::Overlay,
                        "darken" => skia_safe::BlendMode::Darken,
                        "lighten" => skia_safe::BlendMode::Lighten,
                        "difference" => skia_safe::BlendMode::Difference,
                        "exclusion" => skia_safe::BlendMode::Exclusion,
                        _ => return Err("unsupported blend mode".into()),
                    }
                }
                "backdrop-filter" => {
                    if s != "none" {
                        let value = s
                            .strip_prefix("blur(")
                            .and_then(|s| s.strip_suffix(')'))
                            .ok_or("only backdrop blur is supported")?;
                        paint.box_node.backdrop_blur = Animated::new(px(value, m)?);
                    }
                }
                "mask-image" => {
                    if s != "none" {
                        paint.mask =
                            Some(crate::gradient::NativeGradient::parse(s, m, paint.color)?);
                    }
                }
                "mask-mode" => {
                    paint.mask_luminance = match s.trim() {
                        "alpha" | "match-source" => false,
                        "luminance" => true,
                        _ => return Err(format!("unsupported mask-mode '{s}'")),
                    }
                }
                "transform" => {
                    paint.transform =
                        TransformList::parse_string(s).map_err(|e| format!("transform: {e:?}"))?;
                    for t in &paint.transform.0 {
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
                                let distance = absolute_px(z, m)?;
                                if distance <= 0.0 {
                                    return Err("perspective distance must be positive".into());
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "perspective" => {
                    paint.perspective = if s == "none" {
                        None
                    } else {
                        let distance = px(s, m)?;
                        if distance <= 0.0 {
                            return Err("perspective distance must be positive".into());
                        }
                        Some(distance)
                    };
                }
                "perspective-origin" => {
                    paint.perspective_origin = origin_pair(s, m)?;
                }
                "transform-style" => {
                    paint.preserve_3d = match s {
                        "flat" => false,
                        "preserve-3d" => true,
                        _ => return Err("unsupported transform-style".into()),
                    };
                }
                "backface-visibility" => {
                    paint.backface_hidden = match s {
                        "visible" => false,
                        "hidden" => true,
                        _ => return Err("unsupported backface-visibility".into()),
                    };
                }
                "transform-origin" => {
                    let (origin, z) = origin_with_z(s, m)?;
                    paint.origin = origin;
                    paint.origin_z = z;
                }
                "font-family" => {
                    paint.font_family = split(s, true)
                        .first()
                        .ok_or("empty font-family")?
                        .trim_matches(['\'', '"'])
                        .to_string()
                }
                "font-weight" => {
                    paint.font_weight = match s {
                        "normal" => 400,
                        "bold" => 700,
                        _ => number(s)?.clamp(1.0, 1000.0) as u16,
                    }
                }
                "font-style" => {
                    if matches!(s, "normal" | "italic") {
                        paint.font_style = s.into()
                    } else {
                        return Err("oblique font style unsupported".into());
                    }
                }
                // The `font` shorthand expands to both, so `normal` must pass.
                "font-stretch" => {
                    if !matches!(s.trim(), "normal" | "100%") {
                        return Err("font-stretch other than normal unsupported".into());
                    }
                }
                "font-variant-caps" => {
                    if s.trim() != "normal" {
                        return Err("font-variant-caps other than normal unsupported".into());
                    }
                }
                "font-variant-numeric" => {
                    paint.tabular_nums = match s.trim() {
                        "normal" => false,
                        "tabular-nums" => true,
                        _ => return Err("unsupported font-variant-numeric".into()),
                    }
                }
                "letter-spacing" => {
                    paint.letter_spacing = if s == "normal" { 0.0 } else { px(s, m)? }
                }
                "line-height" => {
                    if s != "normal" {
                        paint.line_height = Some(number(s).or_else(|_| {
                            length(s, m).map(|l| resolve(l, m.font_size) / m.font_size)
                        })?)
                    }
                }
                "text-align" => {
                    paint.align = match s {
                        "left" | "start" => skia_safe::textlayout::TextAlign::Left,
                        "right" | "end" => skia_safe::textlayout::TextAlign::Right,
                        "center" => skia_safe::textlayout::TextAlign::Center,
                        "justify" => skia_safe::textlayout::TextAlign::Justify,
                        _ => return Err("unsupported text-align".into()),
                    }
                }
                "text-indent" => {
                    let parsed = lightningcss::properties::text::TextIndent::parse_string(s)
                        .map_err(|_| "unsupported text-indent".to_string())?;
                    if parsed.hanging || parsed.each_line {
                        return Err("unsupported text-indent".into());
                    }
                    paint.text_indent = convert_length_with_basis(&parsed.value, m, block.width)?;
                }
                "text-overflow" => {
                    paint.text_overflow = match s.trim() {
                        "clip" => TextOverflow::Clip,
                        "ellipsis" => TextOverflow::Ellipsis,
                        _ => return Err("unsupported text-overflow".into()),
                    }
                }
                "line-clamp" | "-webkit-line-clamp" => {
                    paint.line_clamp = parse_line_clamp(s)?;
                }
                "white-space" => {
                    // pre-line skips normalize so hard newlines stay. The wrap path collapses spaces.
                    paint.pre_line = s == "pre-line";
                    paint.preserve_space = match s {
                        "normal" | "nowrap" => false,
                        "pre" | "pre-wrap" | "pre-line" => true,
                        _ => return Err("unsupported whitespace mode".into()),
                    };
                    paint.no_wrap = matches!(s, "nowrap" | "pre");
                }
                "overflow-wrap" | "word-wrap" => {
                    paint.overflow_wrap = match s.trim() {
                        "normal" => OverflowWrap::Normal,
                        "break-word" => OverflowWrap::BreakWord,
                        "anywhere" => OverflowWrap::Anywhere,
                        _ => return Err("unsupported overflow-wrap".into()),
                    }
                }
                "word-break" => {
                    let mode = s.trim();
                    paint.break_word = mode == "break-word";
                    paint.break_all = mode == "break-all";
                    if !matches!(mode, "normal" | "break-word" | "break-all") {
                        return Err("unsupported word-break".into());
                    }
                }
                "text-transform" => {
                    paint.text_transform = match s {
                        "none" => TextTransform::None,
                        "uppercase" => TextTransform::Uppercase,
                        "lowercase" => TextTransform::Lowercase,
                        "capitalize" => TextTransform::Capitalize,
                        _ => return Err("unsupported text-transform".into()),
                    }
                }
                "text-decoration" => {
                    let parsed = crate::decoration_value::parse_shorthand(s, m)?;
                    paint.decoration.underline = parsed.underline;
                    paint.decoration.overline = parsed.overline;
                    paint.decoration.line_through = parsed.line_through;
                    paint.decoration_style = parsed.style;
                    paint.decoration_thickness = parsed.thickness;
                }
                "text-decoration-line" => {
                    let parsed = crate::decoration_value::parse_lines(s)?;
                    paint.decoration.underline = parsed.underline;
                    paint.decoration.overline = parsed.overline;
                    paint.decoration.line_through = parsed.line_through;
                }
                "text-decoration-style" => {
                    paint.decoration_style = crate::decoration_value::parse_style(s)?;
                }
                "text-decoration-color" => {
                    let trimmed = s.trim();
                    paint.decoration_color = if trimmed.eq_ignore_ascii_case("currentcolor") {
                        None
                    } else {
                        Some(color(trimmed, paint.color)?)
                    };
                }
                "text-decoration-thickness" => {
                    paint.decoration_thickness = crate::decoration_value::parse_thickness(s, m)?;
                }
                "text-underline-offset" => {
                    paint.text_underline_offset = parse_underline_offset(s, m)?;
                }
                "text-shadow" => {
                    paint.text_shadows = text_shadows(s, m, paint.color)?;
                }
                "-webkit-text-stroke"
                | "-webkit-text-stroke-width"
                | "-webkit-text-stroke-color" => {
                    let current = paint.color;
                    let width_of = |token: &str| -> Result<f32, String> {
                        if token.eq_ignore_ascii_case("none") {
                            return Ok(0.0);
                        }
                        let width =
                            px(token, m).map_err(|_| "unsupported text-stroke".to_string())?;
                        if width < 0.0 {
                            return Err("unsupported text-stroke".into());
                        }
                        Ok(width)
                    };
                    let ink_of = |token: &str| -> Result<Option<Color>, String> {
                        if token.eq_ignore_ascii_case("currentcolor") {
                            Ok(None)
                        } else {
                            color(token, current)
                                .map(Some)
                                .map_err(|_| "unsupported text-stroke".to_string())
                        }
                    };
                    match name.as_str() {
                        "-webkit-text-stroke-width" => {
                            let parts = split(s, false);
                            if parts.len() != 1 {
                                return Err("unsupported text-stroke".into());
                            }
                            paint.text_stroke_width = width_of(&parts[0])?;
                        }
                        "-webkit-text-stroke-color" => {
                            let parts = split(s, false);
                            if parts.len() != 1 {
                                return Err("unsupported text-stroke".into());
                            }
                            paint.text_stroke_color = ink_of(&parts[0])?;
                        }
                        _ => match split(s, false).as_slice() {
                            [width] => {
                                paint.text_stroke_width = width_of(width)?;
                                paint.text_stroke_color = None;
                            }
                            [width, ink] => {
                                paint.text_stroke_width = width_of(width)?;
                                paint.text_stroke_color = ink_of(ink)?;
                            }
                            _ => return Err("unsupported text-stroke".into()),
                        },
                    }
                }
                "paint-order" => {
                    paint.stroke_over_fill = stroke_over_fill(s)?;
                }
                "direction" => {
                    if !matches!(s, "ltr" | "rtl") {
                        return Err("invalid text direction".into());
                    }
                    paint.rtl = s == "rtl";
                }
                "z-index" => {
                    paint.z_index = if s == "auto" {
                        0
                    } else {
                        s.parse().map_err(|_| "invalid z-index")?
                    }
                }
                "object-fit" => {
                    if !matches!(s, "fill" | "cover" | "contain" | "none" | "scale-down") {
                        return Err("unsupported object-fit".into());
                    }
                }
                "object-position" => {
                    let parsed = crate::object_pos::parse_object_position(s, m)?;
                    paint.object_position = ObjectPosition {
                        x_percent: parsed.x_percent,
                        x_offset: parsed.x_offset,
                        y_percent: parsed.y_percent,
                        y_offset: parsed.y_offset,
                    };
                }
                "clip-path" => {
                    if s != "none" {
                        let points = if let Some(content) =
                            s.strip_prefix("polygon(").and_then(|s| s.strip_suffix(')'))
                        {
                            let mut points = Vec::new();
                            for point in split(content, true) {
                                let xy = split(&point, false);
                                if xy.len() != 2 {
                                    return Err("invalid polygon point".into());
                                };
                                let x = length(&xy[0], m)?;
                                let y = length(&xy[1], m)?;
                                if !x.is_percent() || !y.is_percent() {
                                    return Err("polygon points must be percentages".into());
                                };
                                points.push((x.value(), y.value()));
                            }
                            points
                        } else {
                            clip_shape_points(s, values, m)?
                        };
                        paint.box_node.clip_path = Some(points);
                    }
                }
                "animation-name"
                | "animation-duration"
                | "animation-delay"
                | "animation-timing-function"
                | "animation-iteration-count"
                | "animation-direction"
                | "animation-fill-mode"
                | "animation-play-state"
                | "animation-timeline"
                | "animation-range-start"
                | "animation-range-end"
                | "animation-composition" => {}
                "transition-property"
                | "transition-duration"
                | "transition-delay"
                | "transition-timing-function" => {
                    if !matches!(s, "all" | "none" | "0s" | "ease") {
                        return Err("interactive CSS transitions require capture".into());
                    }
                }
                _ => return Err(format!("unsupported property '{name}'")),
            }
            Ok(())
        })();
        if let Err(e) = result {
            errors.push(format!("{name}: {s}: {e}"));
        }
    }
    // Alphabetical order applies bottom/left before position, so clear sticky insets here.
    if values.get("position").map(String::as_str) == Some("sticky") {
        style.inset.left = auto();
        style.inset.right = auto();
        style.inset.top = auto();
        style.inset.bottom = auto();
    }
    let mut colors = Vec::new();
    let mut widths = Vec::new();
    for side in ["top", "right", "bottom", "left"] {
        let disabled = values
            .get(&format!("border-{side}-style"))
            .is_none_or(|s| s == "none" || s == "hidden");
        let w = if disabled {
            0.0
        } else {
            match side {
                "top" => style.border.top.value(),
                "right" => style.border.right.value(),
                "bottom" => style.border.bottom.value(),
                _ => style.border.left.value(),
            }
        };
        widths.push(w);
        colors.push(
            values
                .get(&format!("border-{side}-color"))
                .and_then(|s| color(s, paint.color).ok())
                .unwrap_or(paint.color),
        );
    }
    style.border = taffy::geometry::Rect {
        top: taffy::prelude::length(widths[0]),
        right: taffy::prelude::length(widths[1]),
        bottom: taffy::prelude::length(widths[2]),
        left: taffy::prelude::length(widths[3]),
    };
    paint.border_colors = colors.try_into().unwrap();
    if paint.rtl {
        if values.get("text-align").is_none_or(|s| s == "start") {
            paint.align = skia_safe::textlayout::TextAlign::Right;
        } else if values.get("text-align").is_some_and(|s| s == "end") {
            paint.align = skia_safe::textlayout::TextAlign::Left;
        }
    }
    paint.box_node.style = style.clone();
    if errors.is_empty() {
        Ok((style, paint, grid_scope))
    } else {
        Err(errors)
    }
}

/// Parses a CSS `text-shadow` list in source order. `none` is empty.
fn text_shadows(
    s: &str,
    m: Metrics,
    current: Color,
) -> Result<Vec<director_core::element::TextShadow>, String> {
    if s.trim() == "none" {
        return Ok(Vec::new());
    }
    split(s, true)
        .iter()
        .map(|part| {
            let parsed = lightningcss::properties::text::TextShadow::parse_string(part)
                .map_err(|e| format!("text-shadow: {e:?}"))?;
            let spread = shadow_length(&parsed.spread, m)?;
            if spread != 0.0 {
                return Err("text-shadow spread is unsupported".into());
            }
            Ok(director_core::element::TextShadow {
                color: css_color(&parsed.color, current)?,
                blur: shadow_length(&parsed.blur, m)?.max(0.0),
                offset: (
                    shadow_length(&parsed.x_offset, m)?,
                    shadow_length(&parsed.y_offset, m)?,
                ),
            })
        })
        .collect()
}

fn shadow_length(length: &lightningcss::values::length::Length, m: Metrics) -> Result<f32, String> {
    px(
        &length
            .to_css_string(Default::default())
            .map_err(|e| e.to_string())?,
        m,
    )
}

fn shadows(s: &str, m: Metrics, current: Color) -> Result<Vec<Shadow>, String> {
    split(s, true)
        .iter()
        .map(|s| {
            let shadow = lightningcss::properties::box_shadow::BoxShadow::parse_string(s)
                .map_err(|e| format!("shadow: {e:?}"))?;
            let convert = |l: &lightningcss::values::length::Length| -> Result<f32, String> {
                px(
                    &l.to_css_string(Default::default())
                        .map_err(|e| e.to_string())?,
                    m,
                )
            };
            Ok(Shadow {
                inset: shadow.inset,
                color: css_color(&shadow.color, current)?,
                blur: convert(&shadow.blur)? / 2.0,
                x: convert(&shadow.x_offset)?,
                y: convert(&shadow.y_offset)?,
                spread: convert(&shadow.spread)?,
            })
        })
        .collect()
}

/// One CSS filter function with its amount resolved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterFn {
    Blur(f32),
    Brightness(f32),
    Contrast(f32),
    Saturate(f32),
    /// Clamped to `0.0..=1.0`.
    Grayscale(f32),
    HueRotate(f32),
    DropShadow { x: f32, y: f32, blur: f32, color: Color },
    Invert(f32),
    Sepia(f32),
    Opacity(f32),
}

fn filters(s: &str, m: Metrics) -> Result<Vec<FilterFn>, String> {
    let mut out = Vec::new();
    for call in split(s, false) {
        let (name, args) = call.split_once('(').ok_or("invalid filter function")?;
        let args = args.strip_suffix(')').ok_or("invalid filter function")?;
        // Lightning CSS writes a default amount as an empty call, such as `grayscale()` or `blur()`.
        let scalar = || -> Result<f32, String> {
            let args = args.trim();
            if args.is_empty() {
                Ok(1.0)
            } else if let Some(n) = args.strip_suffix('%') {
                Ok(number(n)? / 100.0)
            } else {
                number(args)
            }
        };
        // Omitted amount is the CSS default (1). Amounts outside 0..=1 clamp.
        let unit = |empty: f32| -> Result<f32, String> {
            let args = args.trim();
            let amount = if args.is_empty() {
                empty
            } else if let Some(n) = args.strip_suffix('%') {
                number(n.trim())? / 100.0
            } else {
                number(args)?
            };
            Ok(amount.clamp(0.0, 1.0))
        };
        out.push(match name {
            "blur" if args.trim().is_empty() => continue,
            "blur" => FilterFn::Blur(px(args, m)?),
            "brightness" => FilterFn::Brightness(scalar()?),
            "contrast" => FilterFn::Contrast(scalar()?),
            "saturate" => FilterFn::Saturate(scalar()?),
            "grayscale" => FilterFn::Grayscale(unit(1.0)?),
            "hue-rotate" if args.trim().is_empty() => continue,
            "hue-rotate" => FilterFn::HueRotate(
                lightningcss::values::angle::Angle::parse_string(args)
                    .map_err(|e| format!("hue angle: {e:?}"))?
                    .to_degrees(),
            ),
            "drop-shadow" => {
                let parsed = lightningcss::properties::box_shadow::BoxShadow::parse_string(args)
                    .map_err(|e| format!("drop-shadow: {e:?}"))?;
                let conv = |l: &lightningcss::values::length::Length| -> Result<f32, String> {
                    px(
                        &l.to_css_string(Default::default())
                            .map_err(|e| e.to_string())?,
                        m,
                    )
                };
                FilterFn::DropShadow {
                    x: conv(&parsed.x_offset)?,
                    y: conv(&parsed.y_offset)?,
                    blur: conv(&parsed.blur)?,
                    color: css_color(&parsed.color, Color::BLACK)?,
                }
            }
            "invert" => FilterFn::Invert(unit(1.0)?),
            "sepia" => FilterFn::Sepia(unit(1.0)?),
            "opacity" => FilterFn::Opacity(unit(1.0)?),
            _ => return Err(format!("unsupported filter '{name}'")),
        });
    }
    Ok(out)
}

/// Compose a filter list into one image filter, the first function innermost.
pub(crate) fn compile_filters(list: &[FilterFn]) -> Option<skia_safe::ImageFilter> {
    let mut chain = None;
    for function in list {
        let mut f = director_core::types::FilterConfig::default();
        let filter = match *function {
            FilterFn::Blur(radius) => {
                f.blur = radius;
                f.to_image_filter()
            }
            FilterFn::Brightness(amount) => {
                f.brightness = amount;
                f.to_image_filter()
            }
            FilterFn::Contrast(amount) => {
                f.contrast = amount;
                f.to_image_filter()
            }
            FilterFn::Saturate(amount) => {
                f.saturation = amount;
                f.to_image_filter()
            }
            FilterFn::Grayscale(amount) => {
                f.saturation = 1.0 - amount;
                f.to_image_filter()
            }
            FilterFn::HueRotate(degrees) => {
                f.hue_rotate = degrees;
                f.to_image_filter()
            }
            FilterFn::DropShadow { x, y, blur, color } => skia_safe::image_filters::drop_shadow(
                (x, y),
                (blur, blur),
                color.to_color4f(),
                None,
                None,
                None,
            ),
            FilterFn::Invert(amount) => {
                f.invert = amount;
                f.to_image_filter()
            }
            FilterFn::Sepia(amount) => {
                f.sepia = amount;
                f.to_image_filter()
            }
            FilterFn::Opacity(amount) => {
                f.opacity = amount;
                f.to_image_filter()
            }
        };
        if let Some(filter) = filter {
            chain = if let Some(inner) = chain {
                skia_safe::image_filters::compose(filter, inner)
            } else {
                Some(filter)
            };
        }
    }
    chain
}

fn absolute_px(l: &Length, m: Metrics) -> Result<f32, String> {
    px(
        &l.to_css_string(Default::default())
            .map_err(|e| e.to_string())?,
        m,
    )
}

fn origin_with_z(
    s: &str,
    m: Metrics,
) -> Result<((LengthPercentage, LengthPercentage), f32), String> {
    let parsed = crate::transform_origin::parse_origin(s, m)?;
    Ok(((parsed.x, parsed.y), parsed.z))
}

fn origin_pair(s: &str, m: Metrics) -> Result<(LengthPercentage, LengthPercentage), String> {
    let (origin, z) = origin_with_z(s, m)?;
    // perspective-origin is a 2D point. A non-zero z stays refused.
    if z != 0.0 {
        return Err("3D transform origin unsupported".into());
    }
    Ok(origin)
}

fn wrap_turn(angle: &mut Angle) {
    let degrees = angle.to_degrees().rem_euclid(360.0);
    *angle = Angle::Deg(if degrees == 360.0 { 0.0 } else { degrees });
}

// lightningcss builds no matrix for an angle below 0 or at a full turn and above.
fn wrap_angles(transform: &mut Transform) {
    match transform {
        Transform::Rotate(angle)
        | Transform::RotateX(angle)
        | Transform::RotateY(angle)
        | Transform::RotateZ(angle)
        | Transform::Rotate3d(_, _, _, angle)
        | Transform::SkewX(angle)
        | Transform::SkewY(angle) => wrap_turn(angle),
        Transform::Skew(x, y) => {
            wrap_turn(x);
            wrap_turn(y);
        }
        _ => {}
    }
}

fn is_projective(transform: &Transform) -> bool {
    matches!(
        transform,
        Transform::TranslateZ(_)
            | Transform::Translate3d(..)
            | Transform::ScaleZ(_)
            | Transform::Scale3d(..)
            | Transform::RotateX(_)
            | Transform::RotateY(_)
            | Transform::Rotate3d(..)
            | Transform::Perspective(_)
            | Transform::Matrix3d(_)
    )
}

fn prepare_projective(list: &TransformList, m: Metrics, size: (f32, f32)) -> TransformList {
    let mut resolved = list.clone();
    let length_px = |value: &mut CssLength, basis: f32| {
        if let Ok(length) = convert_length(value, m) {
            *value = CssLength::px(resolve(length, basis));
        }
    };
    for transform in &mut resolved.0 {
        match transform {
            Transform::Translate(x, y) => {
                length_px(x, size.0);
                length_px(y, size.1);
            }
            Transform::TranslateX(x) => length_px(x, size.0),
            Transform::TranslateY(y) => length_px(y, size.1),
            Transform::TranslateZ(z) | Transform::Perspective(z) => {
                if let Ok(value) = absolute_px(z, m) {
                    *z = Length::px(value);
                }
            }
            Transform::Translate3d(x, y, z) => {
                length_px(x, size.0);
                length_px(y, size.1);
                if let Ok(value) = absolute_px(z, m) {
                    *z = Length::px(value);
                }
            }
            other => wrap_angles(other),
        }
    }
    resolved
}

fn affine_from(matrix: &Matrix3d<f32>) -> Matrix {
    Matrix::new_all(
        matrix.m11, matrix.m21, matrix.m41, matrix.m12, matrix.m22, matrix.m42, 0.0, 0.0, 1.0,
    )
}

fn m44_from(matrix: &Matrix3d<f32>) -> M44 {
    M44::row_major(&[
        matrix.m11, matrix.m21, matrix.m31, matrix.m41, matrix.m12, matrix.m22, matrix.m32,
        matrix.m42, matrix.m13, matrix.m23, matrix.m33, matrix.m43, matrix.m14, matrix.m24,
        matrix.m34, matrix.m44,
    ])
}

/// CSS backface rule: the plane faces away when the inverse of its accumulated
/// matrix flips z. A 2D mirror such as `scaleX(-1)` keeps its front face.
pub fn shows_backface(matrix: &M44) -> bool {
    matrix.invert().is_some_and(|inverse| inverse.rc(2, 2) < 0.0)
}

pub fn perspective_matrix(distance: f32) -> M44 {
    m44_from(&Matrix3d::perspective(distance))
}

pub fn canvas_transform(list: &TransformList, m: Metrics, size: (f32, f32)) -> CanvasTransform {
    if list.0.iter().any(is_projective) {
        if let Some(matrix) = prepare_projective(list, m, size).to_matrix() {
            return if matrix.is_2d() {
                CanvasTransform::Affine(affine_from(&matrix))
            } else {
                CanvasTransform::Projective(m44_from(&matrix))
            };
        }
    }
    CanvasTransform::Affine(transform_matrix(list, m, size))
}

pub fn transform_matrix(list: &TransformList, m: Metrics, size: (f32, f32)) -> Matrix {
    let mut resolved = list.clone();
    let cv = |v: &mut CssLength, basis: f32| {
        if let Ok(l) = convert_length(v, m) {
            *v = CssLength::px(resolve(l, basis));
        }
    };
    for t in &mut resolved.0 {
        match t {
            Transform::Translate(x, y) => {
                cv(x, size.0);
                cv(y, size.1)
            }
            Transform::TranslateX(x) => cv(x, size.0),
            Transform::TranslateY(y) => cv(y, size.1),
            other => wrap_angles(other),
        }
    }
    if let Some(mat) = resolved.to_matrix().and_then(|m| m.to_matrix2d()) {
        Matrix::new_all(mat.a, mat.c, mat.e, mat.b, mat.d, mat.f, 0.0, 0.0, 1.0)
    } else {
        Matrix::new_identity()
    }
}

fn background_layers(
    s: &str,
    m: Metrics,
    current: Color,
    assets: &crate::assets::AssetStore,
) -> Result<Vec<BackgroundLayer>, String> {
    let parts = split(s, true);
    let mut layers = Vec::new();
    for part in &parts {
        if part.eq_ignore_ascii_case("none") {
            continue;
        }
        if is_css_url(part) {
            layers.push(BackgroundLayer::Image(decode_background_url(part, assets)?));
        } else {
            layers.push(BackgroundLayer::Gradient(
                crate::gradient::NativeGradient::parse(part, m, current)?,
            ));
        }
    }
    Ok(layers)
}

fn is_css_url(value: &str) -> bool {
    let value = value.trim();
    value.len() >= 4 && value[..4].eq_ignore_ascii_case("url(")
        || value.len() >= 5 && value[..5].eq_ignore_ascii_case("data:")
}

fn decode_background_url(
    value: &str,
    assets: &crate::assets::AssetStore,
) -> Result<skia_safe::Image, String> {
    let value = value.trim();
    let url = if value.len() >= 4 && value[..4].eq_ignore_ascii_case("url(") {
        crate::compiler::url(value).map_err(|e| e.to_string())?
    } else {
        value.to_string()
    };
    let bytes = assets.read(&url).map_err(|e| e.to_string())?;
    skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&bytes))
        .ok_or_else(|| format!("invalid image '{url}'"))
}

fn background_position_layers(s: &str, m: Metrics) -> Result<Vec<(AxisPos, AxisPos)>, String> {
    let parts = split(s.trim(), true);
    if parts.is_empty() {
        return Err(format!("unsupported background-position '{s}'"));
    }
    parts
        .iter()
        .map(|part| background_position(part, m))
        .collect()
}

fn axis_position_list(
    s: &str,
    m: Metrics,
    parse: fn(&str, Metrics) -> Result<AxisPos, String>,
) -> Result<Vec<AxisPos>, String> {
    let parts = split(s.trim(), true);
    if parts.is_empty() {
        return Err(format!("unsupported background-position '{s}'"));
    }
    parts.iter().map(|part| parse(part, m)).collect()
}

fn background_position(s: &str, m: Metrics) -> Result<(AxisPos, AxisPos), String> {
    let parts = split(s.trim(), false);
    let center = AxisPos {
        percent: 0.5,
        length: 0.0,
    };
    match parts.as_slice() {
        [one] => {
            if one.eq_ignore_ascii_case("top") || one.eq_ignore_ascii_case("bottom") {
                Ok((center, vertical_position(one, m)?))
            } else {
                Ok((horizontal_position(one, m)?, center))
            }
        }
        [x, y] => Ok((horizontal_position(x, m)?, vertical_position(y, m)?)),
        _ => Err(format!("unsupported background-position '{s}'")),
    }
}

fn parse_line_clamp(source: &str) -> Result<Option<u32>, String> {
    let trimmed = source.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    if !trimmed.is_empty() && trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        if let Ok(n) = trimmed.parse::<u32>() {
            if n > 0 {
                return Ok(Some(n));
            }
        }
    }
    Err(format!("unsupported line-clamp '{source}'"))
}

fn parse_underline_offset(source: &str, m: Metrics) -> Result<f32, String> {
    let trimmed = source.trim();
    if trimmed.eq_ignore_ascii_case("auto") {
        return Ok(0.0);
    }
    let parsed =
        length(trimmed, m).map_err(|_| format!("unsupported text-underline-offset '{source}'"))?;
    let px = if parsed.is_percent() {
        parsed.value() * m.font_size
    } else {
        parsed.value()
    };
    if px.is_finite() {
        Ok(px)
    } else {
        Err(format!("unsupported text-underline-offset '{source}'"))
    }
}

fn horizontal_position(s: &str, m: Metrics) -> Result<AxisPos, String> {
    let parsed = HorizontalPosition::parse_string(s)
        .map_err(|_| format!("unsupported background-position '{s}'"))?;
    match parsed {
        PositionComponent::Center => Ok(AxisPos {
            percent: 0.5,
            length: 0.0,
        }),
        PositionComponent::Length(v) => axis_from_length(&v, m),
        PositionComponent::Side { side, offset } => side_offset(
            matches!(side, HorizontalPositionKeyword::Right),
            offset.as_ref(),
            m,
        ),
    }
}

fn vertical_position(s: &str, m: Metrics) -> Result<AxisPos, String> {
    let parsed = VerticalPosition::parse_string(s)
        .map_err(|_| format!("unsupported background-position '{s}'"))?;
    match parsed {
        PositionComponent::Center => Ok(AxisPos {
            percent: 0.5,
            length: 0.0,
        }),
        PositionComponent::Length(v) => axis_from_length(&v, m),
        PositionComponent::Side { side, offset } => side_offset(
            matches!(side, VerticalPositionKeyword::Bottom),
            offset.as_ref(),
            m,
        ),
    }
}

fn side_offset(from_end: bool, offset: Option<&CssLength>, m: Metrics) -> Result<AxisPos, String> {
    let extra = match offset {
        Some(v) => axis_from_length(v, m)?,
        None => AxisPos {
            percent: 0.0,
            length: 0.0,
        },
    };
    let sign = if from_end { -1.0 } else { 1.0 };
    Ok(AxisPos {
        percent: if from_end { 1.0 } else { 0.0 } + sign * extra.percent,
        length: sign * extra.length,
    })
}

fn axis_from_length(value: &CssLength, m: Metrics) -> Result<AxisPos, String> {
    let s = value
        .to_css_string(Default::default())
        .map_err(|e| e.to_string())?;
    let l = length(&s, m)?;
    if l.is_percent() {
        Ok(AxisPos {
            percent: l.value(),
            length: 0.0,
        })
    } else {
        Ok(AxisPos {
            percent: 0.0,
            length: l.value(),
        })
    }
}

fn background_size_list(s: &str, m: Metrics) -> Result<Vec<BackgroundFit>, String> {
    let parts = split(s.trim(), true);
    if parts.is_empty() {
        return Err(format!("unsupported background-size '{s}'"));
    }
    parts.iter().map(|part| background_size(part, m)).collect()
}

fn background_size(s: &str, m: Metrics) -> Result<BackgroundFit, String> {
    let parsed = BackgroundSize::parse_string(s)
        .map_err(|_| format!("unsupported background-size '{s}'"))?;
    match parsed {
        BackgroundSize::Cover => Ok(BackgroundFit::Cover),
        BackgroundSize::Contain => Ok(BackgroundFit::Contain),
        BackgroundSize::Explicit { width, height } => Ok(BackgroundFit::Lengths(
            size_component(width, m)?,
            size_component(height, m)?,
        )),
    }
}

fn size_component(
    value: LengthPercentageOrAuto,
    m: Metrics,
) -> Result<Option<LengthPercentage>, String> {
    match value {
        LengthPercentageOrAuto::Auto => Ok(None),
        LengthPercentageOrAuto::LengthPercentage(value) => {
            let s = value
                .to_css_string(Default::default())
                .map_err(|e| e.to_string())?;
            Ok(Some(length(&s, m)?))
        }
    }
}

fn background_repeat_list(s: &str) -> Result<Vec<(AxisRepeat, AxisRepeat)>, String> {
    let parts = split(s.trim(), true);
    if parts.is_empty() {
        return Err(format!("unsupported background-repeat '{s}'"));
    }
    parts.iter().map(|part| background_repeat(part)).collect()
}

fn background_repeat(s: &str) -> Result<(AxisRepeat, AxisRepeat), String> {
    crate::background_box::parse_repeat(s)
}

fn clip_shape_points(s: &str, values: &Values, m: Metrics) -> Result<Vec<(f32, f32)>, String> {
    if let Some(inner) = s.trim().strip_prefix("inset(") {
        if inner.to_ascii_lowercase().contains("round") {
            return Err("inset round clip-path is unsupported".into());
        }
    }
    let clip = lightningcss::properties::masking::ClipPath::parse_string(s)
        .map_err(|_| "only polygon clip-path supported".to_string())?;
    match clip {
        lightningcss::properties::masking::ClipPath::Shape {
            shape,
            reference_box,
        } => {
            if reference_box != lightningcss::properties::masking::GeometryBox::BorderBox {
                return Err("only polygon clip-path supported".into());
            }
            match *shape {
                lightningcss::values::shape::BasicShape::Circle(circle) => {
                    circle_clip_points(&circle, values, m)
                }
                lightningcss::values::shape::BasicShape::Ellipse(ellipse) => {
                    ellipse_clip_points(&ellipse, values, m)
                }
                lightningcss::values::shape::BasicShape::Inset(inset) => {
                    if inset.radius != Default::default() {
                        return Err("inset round clip-path is unsupported".into());
                    }
                    inset_clip_points(&inset, values, m)
                }
                _ => Err("only polygon clip-path supported".into()),
            }
        }
        _ => Err("only polygon clip-path supported".into()),
    }
}

fn circle_clip_points(
    circle: &lightningcss::values::shape::Circle,
    values: &Values,
    m: Metrics,
) -> Result<Vec<(f32, f32)>, String> {
    let box_width = definite_px(values, "width", m).unwrap_or(m.width);
    let box_height = definite_px(values, "height", m).unwrap_or(m.height);
    let cx = position_fraction(&circle.position.x, m, box_width, |side| {
        matches!(side, HorizontalPositionKeyword::Right)
    })?;
    let cy = position_fraction(&circle.position.y, m, box_height, |side| {
        matches!(side, VerticalPositionKeyword::Bottom)
    })?;
    let radius = match &circle.radius {
        lightningcss::values::shape::ShapeRadius::ClosestSide => side_radius(cx, cy, false),
        lightningcss::values::shape::ShapeRadius::FarthestSide => side_radius(cx, cy, true),
        lightningcss::values::shape::ShapeRadius::LengthPercentage(value) => {
            match crate::calc_len::resolve_length(value, m, None)? {
                crate::calc_len::ResolvedLength::Percent(fraction) => fraction,
                crate::calc_len::ResolvedLength::Length(px) => {
                    // A length radius is a percentage of the box width; the painter only has percentages, so Y uses that same fraction of height.
                    if box_width <= 0.0 {
                        return Err("clip-path circle radius needs a box width".into());
                    }
                    px / box_width
                }
            }
        }
    };
    Ok(circle_polygon(cx, cy, radius))
}

fn ellipse_clip_points(
    ellipse: &lightningcss::values::shape::Ellipse,
    values: &Values,
    m: Metrics,
) -> Result<Vec<(f32, f32)>, String> {
    let box_width = definite_px(values, "width", m).unwrap_or(m.width);
    let box_height = definite_px(values, "height", m).unwrap_or(m.height);
    let cx = position_fraction(&ellipse.position.x, m, box_width, |side| {
        matches!(side, HorizontalPositionKeyword::Right)
    })?;
    let cy = position_fraction(&ellipse.position.y, m, box_height, |side| {
        matches!(side, VerticalPositionKeyword::Bottom)
    })?;
    let rx = ellipse_radius(&ellipse.radius_x, cx, box_width, m)?;
    let ry = ellipse_radius(&ellipse.radius_y, cy, box_height, m)?;
    Ok(ellipse_polygon(cx, cy, rx, ry))
}

fn ellipse_radius(
    radius: &lightningcss::values::shape::ShapeRadius,
    center: f32,
    axis: f32,
    m: Metrics,
) -> Result<f32, String> {
    match radius {
        lightningcss::values::shape::ShapeRadius::ClosestSide => {
            Ok(center.min(1.0 - center).max(0.0))
        }
        lightningcss::values::shape::ShapeRadius::FarthestSide => {
            Ok(center.max(1.0 - center).max(0.0))
        }
        lightningcss::values::shape::ShapeRadius::LengthPercentage(value) => {
            axis_fraction(value, m, axis)
        }
    }
}

fn ellipse_polygon(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<(f32, f32)> {
    const STEPS: usize = 24;
    (0..STEPS)
        .map(|step| {
            let angle = step as f32 * std::f32::consts::TAU / STEPS as f32;
            (cx + rx * angle.cos(), cy + ry * angle.sin())
        })
        .collect()
}

fn inset_clip_points(
    inset: &lightningcss::values::shape::InsetRect,
    values: &Values,
    m: Metrics,
) -> Result<Vec<(f32, f32)>, String> {
    let box_width = definite_px(values, "width", m).unwrap_or(m.width);
    let box_height = definite_px(values, "height", m).unwrap_or(m.height);
    let lightningcss::values::rect::Rect(top, right, bottom, left) = &inset.rect;
    let top = axis_fraction(top, m, box_height)?;
    let right = axis_fraction(right, m, box_width)?;
    let bottom = axis_fraction(bottom, m, box_height)?;
    let left = axis_fraction(left, m, box_width)?;
    Ok(vec![
        (left, top),
        (1.0 - right, top),
        (1.0 - right, 1.0 - bottom),
        (left, 1.0 - bottom),
    ])
}

fn circle_polygon(cx: f32, cy: f32, radius: f32) -> Vec<(f32, f32)> {
    const STEPS: usize = 24;
    (0..STEPS)
        .map(|step| {
            let angle = step as f32 * std::f32::consts::TAU / STEPS as f32;
            (cx + radius * angle.cos(), cy + radius * angle.sin())
        })
        .collect()
}

fn side_radius(cx: f32, cy: f32, farthest: bool) -> f32 {
    let sides = [cx, 1.0 - cx, cy, 1.0 - cy];
    let radius = if farthest {
        sides.into_iter().fold(f32::MIN, f32::max)
    } else {
        sides.into_iter().fold(f32::MAX, f32::min)
    };
    radius.max(0.0)
}

fn position_fraction<S>(
    value: &PositionComponent<S>,
    m: Metrics,
    axis: f32,
    from_end: impl Fn(&S) -> bool,
) -> Result<f32, String> {
    match value {
        PositionComponent::Center => Ok(0.5),
        PositionComponent::Length(value) => axis_fraction(value, m, axis),
        PositionComponent::Side { side, offset } => {
            let extra = match offset {
                Some(value) => axis_fraction(value, m, axis)?,
                None => 0.0,
            };
            Ok(if from_end(side) { 1.0 - extra } else { extra })
        }
    }
}

fn axis_fraction(value: &CssLength, m: Metrics, axis: f32) -> Result<f32, String> {
    match crate::calc_len::resolve_length(value, m, None)? {
        crate::calc_len::ResolvedLength::Percent(fraction) => Ok(fraction),
        crate::calc_len::ResolvedLength::Length(px) => {
            if axis <= 0.0 {
                return Err("clip-path length needs a box size".into());
            }
            Ok(px / axis)
        }
    }
}

fn definite_px(values: &Values, name: &str, m: Metrics) -> Option<f32> {
    let raw = values.get(name)?;
    let px = px(raw, m).ok()?;
    (px > 0.0).then_some(px)
}

/// Reads `paint-order` for text. Unlisted layers follow the listed ones in the order fill, stroke, markers.
fn stroke_over_fill(value: &str) -> Result<bool, String> {
    let layers: Vec<&str> = value.split_whitespace().collect();
    if layers.len() == 1 && layers[0].eq_ignore_ascii_case("normal") {
        return Ok(true);
    }
    let mut seen: Vec<String> = Vec::new();
    for layer in &layers {
        let layer = layer.to_ascii_lowercase();
        if !matches!(layer.as_str(), "fill" | "stroke" | "markers") || seen.contains(&layer) {
            return Err(format!("unsupported paint-order '{value}'"));
        }
        seen.push(layer);
    }
    if seen.is_empty() {
        return Err(format!("unsupported paint-order '{value}'"));
    }
    let fill = seen.iter().position(|layer| layer == "fill");
    let stroke = seen.iter().position(|layer| layer == "stroke");
    Ok(match (stroke, fill) {
        (Some(stroke), Some(fill)) => fill < stroke,
        (Some(_), None) => false,
        _ => true,
    })
}

#[cfg(test)]
mod mixed_calc_basis {
    use lightningcss::traits::Parse;
    use lightningcss::values::length::LengthPercentage as CssLength;

    use super::{convert_length_with_basis, length_on, LengthParts, Metrics};
    use crate::calc_len::{resolve_length, ResolvedLength};

    fn metrics() -> Metrics {
        Metrics {
            width: 64.0,
            height: 32.0,
            font_size: 16.0,
        }
    }

    #[test]
    fn pure_percent_stays_percent_when_a_basis_is_supplied() {
        let value = CssLength::parse_string("50%").expect("50%");
        match resolve_length(&value, metrics(), Some(64.0)).expect("resolve") {
            ResolvedLength::Percent(fraction) => assert!((fraction - 0.5).abs() < 1.0e-4),
            ResolvedLength::Length(px) => panic!("50% became {px}px"),
        }
        let converted = convert_length_with_basis(&value, metrics(), Some(64.0)).expect("convert");
        assert!(converted.is_percent(), "50% was stored as px");
        assert!((converted.value() - 0.5).abs() < 1.0e-4);
    }

    #[test]
    fn mixed_calc_with_basis_is_length() {
        let value = CssLength::parse_string("calc(50% + 8px)").expect("calc");
        match resolve_length(&value, metrics(), Some(64.0)).expect("resolve") {
            ResolvedLength::Length(px) => assert!((px - 40.0).abs() < 1.0e-4),
            ResolvedLength::Percent(fraction) => panic!("mixed calc stayed {fraction}"),
        }
        let converted = length_on("calc(50% + 8px)", metrics(), Some(64.0)).expect("length");
        assert!(!converted.is_percent());
        assert!((converted.value() - 40.0).abs() < 1.0e-4);
        let swapped = length_on("calc(8px + 50%)", metrics(), Some(64.0)).expect("swapped");
        assert!((swapped.value() - 40.0).abs() < 1.0e-4);
        let with_em = length_on("calc(50% + 10px + 1em)", metrics(), Some(64.0)).expect("em");
        assert!((with_em.value() - 58.0).abs() < 1.0e-4);
    }

    #[test]
    fn missing_or_non_positive_basis_keeps_the_mixed_error() {
        let value = CssLength::parse_string("calc(50% + 8px)").expect("calc");
        for basis in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f32::NAN),
            Some(f32::INFINITY),
        ] {
            let err = convert_length_with_basis(&value, metrics(), basis).expect_err("basis");
            assert!(
                err.contains("mixed calc() requires a length basis"),
                "{basis:?}: {err}"
            );
        }
        let pure = length_on("12px", metrics(), None).expect("px");
        assert!((pure.value() - 12.0).abs() < 1.0e-4);
    }
}
