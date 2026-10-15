use crate::css::{split, Values};
use director_core::{animation::Animated, node::BoxNode, types::Color};
use lightningcss::{
    properties::transform::{Matrix3d, Transform, TransformList},
    traits::{Parse, ToCss},
    values::{
        angle::Angle,
        color::CssColor,
        length::{Length, LengthPercentage as CssLength, LengthValue},
    },
};
use skia_safe::{M44, Matrix};
use taffy::prelude::*;
use taffy::style::Overflow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderLine {
    Solid,
    Dashed,
    Dotted,
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
    pub color: Color,
    pub font_size: f32,
    pub font_family: String,
    pub font_weight: u16,
    pub font_style: String,
    pub letter_spacing: f32,
    pub line_height: Option<f32>,
    pub align: skia_safe::textlayout::TextAlign,
    pub preserve_space: bool,
    pub no_wrap: bool,
    pub uppercase: bool,
    pub rtl: bool,
    pub z_index: i32,
    pub gradient: Vec<crate::gradient::NativeGradient>,
    pub mask: Option<crate::gradient::NativeGradient>,
    pub corners: [(LengthPercentage, LengthPercentage); 4],
    pub border_colors: [Color; 4],
    pub border_lines: [BorderLine; 4],
    pub perspective: Option<f32>,
    pub perspective_origin: (LengthPercentage, LengthPercentage),
    pub backface_hidden: bool,
    pub filter: Option<skia_safe::ImageFilter>,
    pub blend_mode: skia_safe::BlendMode,
    pub shadows: Vec<Shadow>,
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
    match l {
        CssLength::Percentage(p) => Ok(LengthPercentage::percent(p.0)),
        CssLength::Dimension(v) => {
            let px = match v {
                LengthValue::Em(v) => v * m.font_size,
                LengthValue::Rem(v) => v * 16.0,
                LengthValue::Vw(v) => v * m.width / 100.0,
                LengthValue::Vh(v) => v * m.height / 100.0,
                LengthValue::Vmin(v) => v * m.width.min(m.height) / 100.0,
                LengthValue::Vmax(v) => v * m.width.max(m.height) / 100.0,
                _ => v
                    .to_px()
                    .ok_or_else(|| format!("unsupported length unit {v:?}"))?,
            };
            Ok(LengthPercentage::length(px))
        }
        _ => Err("mixed calc()/min()/max()/clamp() lengths require extraction".into()),
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
fn dimension(s: &str, m: Metrics) -> Result<Dimension, String> {
    if s == "auto" {
        Ok(auto())
    } else {
        let v = length(s, m)?;
        Ok(if v.is_percent() {
            percent(v.value())
        } else {
            length_dimension(v.value())
        })
    }
}
fn length_dimension(v: f32) -> Dimension {
    Dimension::length(v)
}
fn lpa(s: &str, m: Metrics) -> Result<LengthPercentageAuto, String> {
    if s == "auto" {
        Ok(auto())
    } else {
        let v = length(s, m)?;
        Ok(if v.is_percent() {
            percent(v.value())
        } else {
            LengthPercentageAuto::length(v.value())
        })
    }
}

fn align(s: &str) -> Result<AlignItems, String> {
    match s {
        "normal" | "stretch" => Ok(AlignItems::Stretch),
        "start" => Ok(AlignItems::Start),
        "end" => Ok(AlignItems::End),
        "flex-start" => Ok(AlignItems::FlexStart),
        "flex-end" => Ok(AlignItems::FlexEnd),
        "center" => Ok(AlignItems::Center),
        "baseline" => Ok(AlignItems::Baseline),
        _ => Err(format!("unsupported alignment '{s}'")),
    }
}
fn content(s: &str) -> Result<JustifyContent, String> {
    match s {
        "normal" | "stretch" => Ok(JustifyContent::Stretch),
        "start" => Ok(JustifyContent::Start),
        "end" => Ok(JustifyContent::End),
        "flex-start" => Ok(JustifyContent::FlexStart),
        "flex-end" => Ok(JustifyContent::FlexEnd),
        "center" => Ok(JustifyContent::Center),
        "space-between" => Ok(JustifyContent::SpaceBetween),
        "space-around" => Ok(JustifyContent::SpaceAround),
        "space-evenly" => Ok(JustifyContent::SpaceEvenly),
        _ => Err(format!("unsupported alignment '{s}'")),
    }
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
fn grid(s: &str, m: Metrics) -> Result<Vec<GridTemplateComponent<String>>, String> {
    if s == "none" {
        return Ok(Vec::new());
    }
    let mut tracks = Vec::new();
    for token in split(s, false) {
        if token.starts_with("repeat(") {
            let args = split(&token[7..token.len() - 1], true);
            if args.len() != 2 {
                return Err("repeat requires count and tracks".into());
            }
            let count = args[0]
                .parse::<usize>()
                .map_err(|_| "auto-fill/auto-fit require extraction")?;
            if count > 4096 {
                return Err("grid repeat exceeds 4096 tracks".into());
            }
            let inner = grid(&args[1], m)?;
            for _ in 0..count {
                tracks.extend(inner.clone());
            }
        } else {
            tracks.push(GridTemplateComponent::Single(grid_track(&token, m)?));
        }
    }
    Ok(tracks)
}
fn placement(s: &str) -> Result<GridPlacement, String> {
    if s == "auto" {
        Ok(auto())
    } else if let Some(n) = s.strip_prefix("span ") {
        Ok(span(n.parse::<u16>().map_err(|_| "invalid grid span")?))
    } else {
        Ok(line(
            s.parse::<i16>()
                .map_err(|_| "named grid lines require extraction")?,
        ))
    }
}

pub fn translate(values: &Values, mut m: Metrics) -> Result<(Style, PaintStyle), Vec<String>> {
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
        color: Color::BLACK,
        font_size: 16.0,
        font_family: "Roboto".into(),
        font_weight: 400,
        font_style: "normal".into(),
        letter_spacing: 0.0,
        line_height: None,
        align: skia_safe::textlayout::TextAlign::Left,
        preserve_space: false,
        no_wrap: false,
        uppercase: false,
        rtl: false,
        z_index: 0,
        gradient: Vec::new(),
        mask: None,
        corners: [(zero(), zero()); 4],
        border_colors: [Color::BLACK; 4],
        border_lines: [BorderLine::Solid; 4],
        perspective: None,
        perspective_origin: (percent(0.5_f32), percent(0.5_f32)),
        backface_hidden: false,
        filter: Default::default(),
        blend_mode: skia_safe::BlendMode::SrcOver,
        shadows: Vec::new(),
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
    for (name, value) in values {
        let s = value.as_str();
        let result: Result<(), String> = (|| {
            match name.as_str() {
                n if n.starts_with("--") => {}
                "color" | "font-size" => {}
                "display" => {
                    style.display = match s {
                        "block" => Display::Block,
                        "flex" | "inline-flex" => Display::Flex,
                        "grid" | "inline-grid" => Display::Grid,
                        "none" => Display::None,
                        "inline" => {
                            return Err(
                                "inline formatting requires a rich text parent or extraction"
                                    .into(),
                            )
                        }
                        _ => return Err(format!("unsupported display '{s}'")),
                    }
                }
                "box-sizing" => {
                    style.box_sizing = match s {
                        "border-box" => BoxSizing::BorderBox,
                        "content-box" => BoxSizing::ContentBox,
                        _ => return Err("invalid box-sizing".into()),
                    }
                }
                "position" => {
                    style.position = match s {
                        "static" | "relative" => Position::Relative,
                        "absolute" => Position::Absolute,
                        _ => return Err("fixed/sticky positioning requires extraction".into()),
                    }
                }
                "width" => style.size.width = dimension(s, m)?,
                "height" => style.size.height = dimension(s, m)?,
                "min-width" => style.min_size.width = dimension(s, m)?,
                "min-height" => style.min_size.height = dimension(s, m)?,
                "max-width" => {
                    style.max_size.width = if s == "none" {
                        auto()
                    } else {
                        dimension(s, m)?
                    }
                }
                "max-height" => {
                    style.max_size.height = if s == "none" {
                        auto()
                    } else {
                        dimension(s, m)?
                    }
                }
                "left" => style.inset.left = lpa(s, m)?,
                "right" => style.inset.right = lpa(s, m)?,
                "top" => style.inset.top = lpa(s, m)?,
                "bottom" => style.inset.bottom = lpa(s, m)?,
                "margin-left" => style.margin.left = lpa(s, m)?,
                "margin-right" => style.margin.right = lpa(s, m)?,
                "margin-top" => style.margin.top = lpa(s, m)?,
                "margin-bottom" => style.margin.bottom = lpa(s, m)?,
                "padding-left" => style.padding.left = length(s, m)?,
                "padding-right" => style.padding.right = length(s, m)?,
                "padding-top" => style.padding.top = length(s, m)?,
                "padding-bottom" => style.padding.bottom = length(s, m)?,
                "column-gap" => {
                    style.gap.width = if s == "normal" { zero() } else { length(s, m)? }
                }
                "row-gap" => style.gap.height = if s == "normal" { zero() } else { length(s, m)? },
                "flex-direction" => {
                    style.flex_direction = match s {
                        "row" => FlexDirection::Row,
                        "column" => FlexDirection::Column,
                        "row-reverse" => FlexDirection::RowReverse,
                        "column-reverse" => FlexDirection::ColumnReverse,
                        _ => return Err("invalid flex-direction".into()),
                    }
                }
                "flex-wrap" => {
                    style.flex_wrap = match s {
                        "nowrap" => FlexWrap::NoWrap,
                        "wrap" => FlexWrap::Wrap,
                        "wrap-reverse" => FlexWrap::WrapReverse,
                        _ => return Err("invalid flex-wrap".into()),
                    }
                }
                "flex-grow" => style.flex_grow = number(s)?,
                "flex-shrink" => style.flex_shrink = number(s)?,
                "flex-basis" => style.flex_basis = dimension(s, m)?,
                "align-items" => style.align_items = Some(align(s)?),
                "align-self" => style.align_self = if s == "auto" { None } else { Some(align(s)?) },
                "justify-items" => style.justify_items = Some(align(s)?),
                "justify-self" => {
                    style.justify_self = if s == "auto" { None } else { Some(align(s)?) }
                }
                "justify-content" => style.justify_content = Some(content(s)?),
                "align-content" => style.align_content = Some(content(s)?),
                "grid-template-columns" => style.grid_template_columns = grid(s, m)?,
                "grid-template-rows" => style.grid_template_rows = grid(s, m)?,
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
                "grid-auto-flow" => {
                    style.grid_auto_flow = match s {
                        "row" => GridAutoFlow::Row,
                        "column" => GridAutoFlow::Column,
                        "row dense" | "dense" => GridAutoFlow::RowDense,
                        "column dense" => GridAutoFlow::ColumnDense,
                        _ => return Err("invalid grid-auto-flow".into()),
                    }
                }
                "grid-column-start" => style.grid_column.start = placement(s)?,
                "grid-column-end" => style.grid_column.end = placement(s)?,
                "grid-row-start" => style.grid_row.start = placement(s)?,
                "grid-row-end" => style.grid_row.end = placement(s)?,
                "aspect-ratio" => {
                    if s != "auto" {
                        let v = split(s, false);
                        style.aspect_ratio = Some(if v.len() == 3 && v[1] == "/" {
                            number(&v[0])? / number(&v[2])?
                        } else {
                            number(s)?
                        });
                    }
                }
                "background-color" => {
                    paint.box_node.bg_color = Some(Animated::new(color(s, paint.color)?))
                }
                "background-image" => {
                    if s != "none" {
                        paint.gradient = crate::gradient::parse_list(s, m, paint.color)?;
                    }
                }
                "background-position-x"
                | "background-position-y"
                | "background-size"
                | "background-repeat"
                | "background-attachment"
                | "background-origin"
                | "background-clip" => {
                    let acceptable = matches!(
                        s,
                        "0" | "0%"
                            | "auto"
                            | "auto auto"
                            | "repeat"
                            | "scroll"
                            | "padding-box"
                            | "border-box"
                    );
                    if !acceptable {
                        return Err(format!("background setting '{s}' requires extraction or native image translation"));
                    }
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
                        "none" | "solid" => BorderLine::Solid,
                        "dashed" => BorderLine::Dashed,
                        "dotted" => BorderLine::Dotted,
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
                        paint.filter = filter(s, m)?;
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
                    if !matches!(s, "flat" | "preserve-3d") {
                        return Err("unsupported transform-style".into());
                    }
                }
                "backface-visibility" => {
                    paint.backface_hidden = match s {
                        "visible" => false,
                        "hidden" => true,
                        _ => return Err("unsupported backface-visibility".into()),
                    };
                }
                "transform-origin" => {
                    paint.origin = origin_pair(s, m)?;
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
                "white-space" => {
                    paint.preserve_space = match s {
                        "normal" | "nowrap" => false,
                        "pre" | "pre-wrap" => true,
                        _ => return Err("unsupported whitespace mode".into()),
                    };
                    paint.no_wrap = matches!(s, "nowrap" | "pre");
                }
                "text-transform" => {
                    paint.uppercase = match s {
                        "none" => false,
                        "uppercase" => true,
                        _ => return Err("unsupported text-transform".into()),
                    }
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
                    if !matches!(s, "fill" | "cover" | "contain") {
                        return Err("unsupported object-fit".into());
                    }
                }
                "clip-path" => {
                    if s != "none" {
                        let content = s
                            .strip_prefix("polygon(")
                            .and_then(|s| s.strip_suffix(')'))
                            .ok_or("only polygon clip-path supported")?;
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
    let mut colors = Vec::new();
    let mut widths = Vec::new();
    for side in ["top", "right", "bottom", "left"] {
        let disabled = values
            .get(&format!("border-{side}-style"))
            .is_none_or(|s| s == "none");
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
        Ok((style, paint))
    } else {
        Err(errors)
    }
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

fn filter(s: &str, m: Metrics) -> Result<Option<skia_safe::ImageFilter>, String> {
    let mut chain = None;
    for call in split(s, false) {
        let mut f = director_core::types::FilterConfig::default();
        let (name, args) = call.split_once('(').ok_or("invalid filter function")?;
        let args = args.strip_suffix(')').ok_or("invalid filter function")?;
        let scalar = || -> Result<f32, String> {
            if let Some(n) = args.strip_suffix('%') {
                Ok(number(n)? / 100.0)
            } else {
                number(args)
            }
        };
        let mut shadow = None;
        match name {
            "blur" => f.blur = px(args, m)?,
            "brightness" => f.brightness = scalar()?,
            "contrast" => f.contrast = scalar()?,
            "saturate" => f.saturation = scalar()?,
            "grayscale" => f.saturation = 1.0 - scalar()?.clamp(0.0, 1.0),
            "hue-rotate" => {
                f.hue_rotate = lightningcss::values::angle::Angle::parse_string(args)
                    .map_err(|e| format!("hue angle: {e:?}"))?
                    .to_degrees()
            }
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
                shadow = skia_safe::image_filters::drop_shadow(
                    (conv(&parsed.x_offset)?, conv(&parsed.y_offset)?),
                    (conv(&parsed.blur)?, conv(&parsed.blur)?),
                    css_color(&parsed.color, Color::BLACK)?.to_color4f(),
                    None,
                    None,
                    None,
                );
            }
            _ => return Err(format!("unsupported filter '{name}'")),
        }
        let filter = shadow.or_else(|| f.to_image_filter());
        if let Some(filter) = filter {
            chain = if let Some(inner) = chain {
                skia_safe::image_filters::compose(filter, inner)
            } else {
                Some(filter)
            };
        }
    }
    Ok(chain)
}

fn absolute_px(l: &Length, m: Metrics) -> Result<f32, String> {
    px(
        &l.to_css_string(Default::default())
            .map_err(|e| e.to_string())?,
        m,
    )
}

fn origin_pair(s: &str, m: Metrics) -> Result<(LengthPercentage, LengthPercentage), String> {
    let v = split(s, false);
    if v.len() > 2 || v.is_empty() {
        return Err("3D transform origin unsupported".into());
    }
    let axis = |s: &str| -> Result<LengthPercentage, String> {
        match s {
            "center" => Ok(percent(0.5_f32)),
            "left" | "top" => Ok(percent(0.0_f32)),
            "right" | "bottom" => Ok(percent(1.0_f32)),
            _ => length(s, m),
        }
    };
    Ok((
        axis(&v[0])?,
        if v.len() == 2 {
            axis(&v[1])?
        } else {
            percent(0.5_f32)
        },
    ))
}

fn wrap_turn(angle: &mut Angle) {
    let degrees = angle.to_degrees().rem_euclid(360.0);
    *angle = Angle::Deg(if degrees == 360.0 { 0.0 } else { degrees });
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
            Transform::Rotate(angle)
            | Transform::RotateX(angle)
            | Transform::RotateY(angle)
            | Transform::RotateZ(angle)
            | Transform::Rotate3d(_, _, _, angle) => wrap_turn(angle),
            _ => {}
        }
    }
    resolved
}

fn affine_from(matrix: &Matrix3d<f32>) -> Matrix {
    Matrix::new_all(
        matrix.m11,
        matrix.m21,
        matrix.m41,
        matrix.m12,
        matrix.m22,
        matrix.m42,
        0.0,
        0.0,
        1.0,
    )
}

fn m44_from(matrix: &Matrix3d<f32>) -> M44 {
    M44::row_major(&[
        matrix.m11,
        matrix.m21,
        matrix.m31,
        matrix.m41,
        matrix.m12,
        matrix.m22,
        matrix.m32,
        matrix.m42,
        matrix.m13,
        matrix.m23,
        matrix.m33,
        matrix.m43,
        matrix.m14,
        matrix.m24,
        matrix.m34,
        matrix.m44,
    ])
}

pub fn shows_backface(list: &TransformList, m: Metrics, size: (f32, f32)) -> bool {
    prepare_projective(list, m, size)
        .to_matrix()
        .is_some_and(|matrix| matrix.m33 < 0.0)
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
            _ => {}
        }
    }
    if let Some(mat) = resolved.to_matrix().and_then(|m| m.to_matrix2d()) {
        Matrix::new_all(mat.a, mat.c, mat.e, mat.b, mat.d, mat.f, 0.0, 0.0, 1.0)
    } else {
        Matrix::new_identity()
    }
}
