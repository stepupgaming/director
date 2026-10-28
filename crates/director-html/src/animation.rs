use crate::{
    css::{split, Css, Values},
    gsap::ease::GsapEase,
    style::{color, number, px, AxisPos, BackgroundFit, FilterFn, Metrics, PaintStyle, Shadow},
};
use director_core::scene_export::{export_easing, rgba, ExportError};
use director_core::scene_import::{color as import_color, easing as import_easing, ImportError};
use director_core::{animation::EasingType, element::TextShadow, types::Color};
use director_scene::html as doc;
use lightningcss::traits::ToCss;
use lightningcss::{
    properties::transform::{Transform, TransformList},
    traits::Parse,
    values::{
        angle::Angle,
        length::{Length, LengthPercentage},
        percentage::NumberOrPercentage,
    },
};

#[derive(Debug, Clone)]
enum Ease {
    Curve(EasingType),
    Steps(u32, bool),
    /// A GSAP 3 ease such as `power3.out`, kept by name for export.
    Gsap(GsapEase),
}
impl Ease {
    /// A CSS timing function, else a GSAP ease name.
    fn parse(s: &str) -> Result<Self, String> {
        Self::parse_css(s).or_else(|css| GsapEase::parse(s).map(Self::Gsap).map_err(|_| css))
    }
    fn parse_css(s: &str) -> Result<Self, String> {
        let curve = |x1, y1, x2, y2| Self::Curve(EasingType::CubicBezier { x1, y1, x2, y2 });
        Ok(match s {
            "linear" => Self::Curve(EasingType::Linear),
            "ease" => curve(0.25, 0.1, 0.25, 1.0),
            "ease-in" => curve(0.42, 0.0, 1.0, 1.0),
            "ease-out" => curve(0.0, 0.0, 0.58, 1.0),
            "ease-in-out" => curve(0.42, 0.0, 0.58, 1.0),
            "step-start" => Self::Steps(1, true),
            "step-end" => Self::Steps(1, false),
            _ if s.starts_with("cubic-bezier(") => {
                let v = split(&s[13..s.len() - 1], true);
                if v.len() != 4 {
                    return Err("invalid cubic-bezier".into());
                };
                let v = v
                    .iter()
                    .map(|s| {
                        s.parse::<f64>()
                            .map_err(|_| "invalid bezier coordinate".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if !(0.0..=1.0).contains(&v[0]) || !(0.0..=1.0).contains(&v[2]) {
                    return Err("bezier x coordinates must be in [0,1]".into());
                }
                curve(v[0], v[1], v[2], v[3])
            }
            _ if s.starts_with("steps(") => {
                let v = split(&s[6..s.len() - 1], true);
                let n = v
                    .first()
                    .ok_or("steps missing count")?
                    .parse::<u32>()
                    .map_err(|_| "invalid steps count")?;
                if n == 0 {
                    return Err("steps count must be positive".into());
                }
                let position = v.get(1).map(String::as_str).unwrap_or("end");
                if !matches!(position, "start" | "jump-start" | "end" | "jump-end") {
                    return Err("only jump-start/jump-end steps supported".into());
                }
                Self::Steps(n, matches!(position, "start" | "jump-start"))
            }
            _ => return Err(format!("unsupported timing function '{s}'")),
        })
    }
    fn eval(&self, t: f32) -> f32 {
        match self {
            Self::Curve(e) => e.eval(t),
            Self::Steps(n, start) => {
                let steps = (t * (*n as f32)).floor() + if *start && t < 1.0 { 1.0 } else { 0.0 };
                (steps / (*n as f32)).clamp(0.0, 1.0)
            }
            Self::Gsap(e) => e.eval(t as f64) as f32,
        }
    }
}

#[derive(Debug, Clone)]
enum Value {
    Scalar(f32),
    Color(Color),
    Transform(TransformList),
    /// Discrete text content. A segment holds its left value.
    Text(String),
}
#[derive(Debug, Clone)]
struct Point {
    offset: f32,
    value: Value,
    ease: Ease,
}
#[derive(Debug, Clone)]
struct Channel {
    name: String,
    points: Vec<Point>,
}
#[derive(Debug, Clone)]
pub struct Animation {
    delay: f64,
    duration: f64,
    count: f64,
    direction: String,
    fill: String,
    channels: Vec<Channel>,
}

fn value(name: &str, s: &str, m: Metrics, current: Color) -> Result<Value, String> {
    Ok(match name {
        "opacity" => Value::Scalar(number(s)?),
        "background-color" | "color" => Value::Color(color(s, current)?),
        "transform" => Value::Transform(
            TransformList::parse_string(s).map_err(|e| format!("animation transform: {e:?}"))?,
        ),
        "width" | "height" | "left" | "top" | "border-top-left-radius" | "font-size" => Value::Scalar(px(s, m)?),
        "letter-spacing" | "word-spacing" if s.trim() == "normal" => Value::Scalar(0.0),
        "letter-spacing" | "word-spacing" => Value::Scalar(px(s, m)?),
        // A line height track is a multiple of the font size; px divide by it.
        "line-height" => match s.trim().strip_suffix("px") {
            Some(n) => Value::Scalar(number(n.trim())? / m.font_size.max(f32::EPSILON)),
            None => Value::Scalar(number(s.trim())?),
        },
        name if box_side(name).is_some() => Value::Scalar(px(s, m)?),
        name if border_side(name).is_some_and(|(_, field)| field == "color") => Value::Color(color(s, current)?),
        name if border_side(name).is_some() => Value::Scalar(px(s, m)?),
        name if is_component(name) => Value::Scalar(component(name, s)?),
        "filter-drop-shadow-color" => Value::Color(color(s, current)?),
        name if indexed(name).is_some_and(|(_, _, field)| field == "color") => Value::Color(color(s, current)?),
        name if is_filter(name) || indexed(name).is_some() => {
            let s = s.trim();
            let unit = ["px", "deg", "%"].iter().find_map(|u| s.strip_suffix(u));
            Value::Scalar(number(unit.unwrap_or(s).trim())?)
        }
        "text" => Value::Text(s.to_string()),
        _ => return Err(format!("unsupported animated property '{name}'")),
    })
}

/// Transform components a timeline drives one number at a time. They compose
/// in GSAP order over the element's own decomposed transform.
pub(crate) const COMPONENTS: [&str; 12] = [
    "translate-x",
    "translate-y",
    "translate-z",
    "translate-x-percent",
    "translate-y-percent",
    "rotate-z",
    "rotate-x",
    "rotate-y",
    "skew-x",
    "skew-y",
    "scale-x",
    "scale-y",
];
pub(crate) fn is_component(name: &str) -> bool {
    COMPONENTS.contains(&name)
}
/// Filter amounts a timeline drives one at a time: blur and drop-shadow
/// lengths in px, hue rotation in degrees, the rest as factors. Each writes
/// its function in the element's filter list, which gains a missing function
/// at its end.
pub(crate) const FILTERS: [&str; 13] = [
    "filter-blur",
    "filter-brightness",
    "filter-contrast",
    "filter-saturate",
    "filter-grayscale",
    "filter-hue-rotate",
    "filter-invert",
    "filter-sepia",
    "filter-opacity",
    "filter-drop-shadow-x",
    "filter-drop-shadow-y",
    "filter-drop-shadow-blur",
    "filter-drop-shadow-color",
];
pub(crate) fn is_filter(name: &str) -> bool {
    FILTERS.contains(&name)
}
/// The amount a filter channel takes when its function is absent.
pub(crate) fn filter_default(name: &str) -> f32 {
    match name {
        "filter-brightness" | "filter-contrast" | "filter-saturate" | "filter-opacity" => 1.0,
        _ => 0.0,
    }
}
const NO_SHADOW: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
/// The amount of a scalar filter channel in `list`, if its function is there.
pub(crate) fn filter_scalar(list: &[FilterFn], name: &str) -> Option<f32> {
    list.iter().find_map(|f| match (name, *f) {
        ("filter-blur", FilterFn::Blur(v))
        | ("filter-brightness", FilterFn::Brightness(v))
        | ("filter-contrast", FilterFn::Contrast(v))
        | ("filter-saturate", FilterFn::Saturate(v))
        | ("filter-grayscale", FilterFn::Grayscale(v))
        | ("filter-hue-rotate", FilterFn::HueRotate(v))
        | ("filter-invert", FilterFn::Invert(v))
        | ("filter-sepia", FilterFn::Sepia(v))
        | ("filter-opacity", FilterFn::Opacity(v)) => Some(v),
        ("filter-drop-shadow-x", FilterFn::DropShadow { x, .. }) => Some(x),
        ("filter-drop-shadow-y", FilterFn::DropShadow { y, .. }) => Some(y),
        ("filter-drop-shadow-blur", FilterFn::DropShadow { blur, .. }) => Some(blur),
        _ => None,
    })
}
pub(crate) fn filter_shadow_color(list: &[FilterFn]) -> Option<Color> {
    list.iter().find_map(|f| match *f {
        FilterFn::DropShadow { color, .. } => Some(color),
        _ => None,
    })
}
fn filter_slot<'a>(list: &'a mut Vec<FilterFn>, name: &str) -> &'a mut FilterFn {
    let same = |f: &FilterFn| match name {
        "filter-blur" => matches!(f, FilterFn::Blur(_)),
        "filter-brightness" => matches!(f, FilterFn::Brightness(_)),
        "filter-contrast" => matches!(f, FilterFn::Contrast(_)),
        "filter-saturate" => matches!(f, FilterFn::Saturate(_)),
        "filter-grayscale" => matches!(f, FilterFn::Grayscale(_)),
        "filter-hue-rotate" => matches!(f, FilterFn::HueRotate(_)),
        "filter-invert" => matches!(f, FilterFn::Invert(_)),
        "filter-sepia" => matches!(f, FilterFn::Sepia(_)),
        "filter-opacity" => matches!(f, FilterFn::Opacity(_)),
        _ => matches!(f, FilterFn::DropShadow { .. }),
    };
    let i = match list.iter().position(same) {
        Some(i) => i,
        None => {
            let d = filter_default(name);
            list.push(match name {
                "filter-blur" => FilterFn::Blur(d),
                "filter-brightness" => FilterFn::Brightness(d),
                "filter-contrast" => FilterFn::Contrast(d),
                "filter-saturate" => FilterFn::Saturate(d),
                "filter-grayscale" => FilterFn::Grayscale(d),
                "filter-hue-rotate" => FilterFn::HueRotate(d),
                "filter-invert" => FilterFn::Invert(d),
                "filter-sepia" => FilterFn::Sepia(d),
                "filter-opacity" => FilterFn::Opacity(d),
                _ => FilterFn::DropShadow { x: 0.0, y: 0.0, blur: 0.0, color: NO_SHADOW },
            });
            list.len() - 1
        }
    };
    &mut list[i]
}
fn set_filter(list: &mut Vec<FilterFn>, name: &str, v: f32) {
    match (filter_slot(list, name), name) {
        (FilterFn::HueRotate(a), _) => *a = v,
        (FilterFn::Grayscale(a) | FilterFn::Invert(a) | FilterFn::Sepia(a) | FilterFn::Opacity(a), _) => {
            *a = v.clamp(0.0, 1.0)
        }
        (FilterFn::Blur(a) | FilterFn::Brightness(a) | FilterFn::Contrast(a) | FilterFn::Saturate(a), _) => {
            *a = v.max(0.0)
        }
        (FilterFn::DropShadow { x, .. }, "filter-drop-shadow-x") => *x = v,
        (FilterFn::DropShadow { y, .. }, "filter-drop-shadow-y") => *y = v,
        (FilterFn::DropShadow { blur, .. }, _) => *blur = v.max(0.0),
    }
}
fn set_filter_color(list: &mut Vec<FilterFn>, c: Color) {
    if let FilterFn::DropShadow { color, .. } = filter_slot(list, "filter-drop-shadow-color") {
        *color = c;
    }
}

/// Box shadow and clip-path fields a timeline drives one at a time:
/// `box-shadow-<i>-<x|y|blur|spread|color|inset>` and `clip-path-<i>-<x|y>`.
/// Lengths are px, `blur` is the CSS blur radius, `inset` turns on at 0.5 and
/// clip corners are box percents. A missing shadow or corner extends its list.
pub(crate) fn indexed(name: &str) -> Option<(&'static str, usize, &str)> {
    let (list, fields, cap): (&'static str, &[&str], usize) = if name.starts_with("box-shadow-") {
        ("box-shadow", &["x", "y", "blur", "spread", "color", "inset"], 32)
    } else if name.starts_with("text-shadow-") {
        ("text-shadow", &["x", "y", "blur", "color"], 32)
    } else if name.starts_with("clip-path-") {
        ("clip-path", &["x", "y"], 256)
    } else if name.starts_with("background-position-") {
        ("background-position", &["x", "y", "x-percent", "y-percent"], 32)
    } else if name.starts_with("background-size-") {
        ("background-size", &["width", "height", "width-percent", "height-percent"], 32)
    } else {
        return None;
    };
    let (i, field) = name[list.len() + 1..].split_once('-')?;
    if i.is_empty() || !i.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let i: usize = i.parse().ok()?;
    (i < cap && fields.contains(&field)).then_some((list, i, field))
}
/// The shadow a missing list entry stands for: transparent and zero sized.
pub(crate) const NO_BOX_SHADOW: Shadow =
    Shadow { inset: false, color: NO_SHADOW, x: 0.0, y: 0.0, blur: 0.0, spread: 0.0 };
/// A scalar shadow field in its track unit.
pub(crate) fn shadow_scalar(shadow: &Shadow, field: &str) -> f32 {
    match field {
        "x" => shadow.x,
        "y" => shadow.y,
        "blur" => shadow.blur * 2.0,
        "spread" => shadow.spread,
        _ => shadow.inset as u8 as f32,
    }
}
fn set_shadow(list: &mut Vec<Shadow>, i: usize, field: &str, v: Value) {
    if list.len() <= i {
        list.resize(i + 1, NO_BOX_SHADOW);
    }
    let shadow = &mut list[i];
    match (field, v) {
        ("color", Value::Color(c)) => shadow.color = c,
        ("x", Value::Scalar(v)) => shadow.x = v,
        ("y", Value::Scalar(v)) => shadow.y = v,
        ("blur", Value::Scalar(v)) => shadow.blur = v.max(0.0) / 2.0,
        ("spread", Value::Scalar(v)) => shadow.spread = v,
        ("inset", Value::Scalar(v)) => shadow.inset = v >= 0.5,
        _ => {}
    }
}
/// The text shadow a missing list entry stands for: transparent and unblurred.
pub(crate) const NO_TEXT_SHADOW: TextShadow = TextShadow { color: NO_SHADOW, blur: 0.0, offset: (0.0, 0.0) };
fn set_text_shadow(list: &mut Vec<TextShadow>, i: usize, field: &str, v: Value) {
    if list.len() <= i {
        list.resize(i + 1, NO_TEXT_SHADOW);
    }
    let shadow = &mut list[i];
    match (field, v) {
        ("color", Value::Color(c)) => shadow.color = c,
        ("x", Value::Scalar(v)) => shadow.offset.0 = v,
        ("y", Value::Scalar(v)) => shadow.offset.1 = v,
        ("blur", Value::Scalar(v)) => shadow.blur = v.max(0.0),
        _ => {}
    }
}
/// One axis of a background layer's position. A list shorter than the layer
/// extends with the entries it cycles through.
fn set_position(list: &mut Vec<AxisPos>, i: usize, percent: bool, v: f32) {
    if list.is_empty() {
        list.push(AxisPos { percent: 0.0, length: 0.0 });
    }
    let n = list.len();
    for k in n..=i {
        list.push(list[k % n]);
    }
    if percent {
        list[i].percent = v / 100.0;
    } else {
        list[i].length = v;
    }
}
/// One axis of a background layer's size, in px or as a percent. A keyword
/// size turns into lengths with the other axis `auto`, and a list shorter than
/// the layer extends with the entries it cycles through.
fn set_size(list: &mut Vec<BackgroundFit>, i: usize, field: &str, v: f32) {
    if list.is_empty() {
        list.push(BackgroundFit::Auto);
    }
    let n = list.len();
    for k in n..=i {
        list.push(list[k % n]);
    }
    let axis = if field.ends_with("percent") {
        taffy::LengthPercentage::percent(v / 100.0)
    } else {
        taffy::LengthPercentage::length(v.max(0.0))
    };
    let (width, height) = match list[i] {
        BackgroundFit::Lengths(width, height) => (width, height),
        _ => (None, None),
    };
    list[i] = if field.starts_with("width") {
        BackgroundFit::Lengths(Some(axis), height)
    } else {
        BackgroundFit::Lengths(width, Some(axis))
    };
}
/// Padding and margin side channels: `padding-<side>` and `margin-<side>` in
/// px, sides in CSS order top, right, bottom, left.
pub(crate) fn box_side(name: &str) -> Option<(&'static str, usize)> {
    let (property, side) = name.split_once('-')?;
    let property = ["padding", "margin"].into_iter().find(|p| *p == property)?;
    let side = ["top", "right", "bottom", "left"].iter().position(|s| *s == side)?;
    Some((property, side))
}
/// Border side channels: `border-<side>-color` and `border-<side>-width`,
/// sides in CSS order top, right, bottom, left.
pub(crate) fn border_side(name: &str) -> Option<(usize, &str)> {
    let rest = name.strip_prefix("border-")?;
    let (side, field) = rest.split_once('-')?;
    let side = ["top", "right", "bottom", "left"].iter().position(|s| *s == side)?;
    matches!(field, "color" | "width").then_some((side, field))
}
fn set_clip(points: &mut Option<Vec<(f32, f32)>>, i: usize, field: &str, v: f32) {
    let list = points.get_or_insert_with(Vec::new);
    if list.len() <= i {
        list.resize(i + 1, (0.0, 0.0));
    }
    if field == "x" {
        list[i].0 = v / 100.0;
    } else {
        list[i].1 = v / 100.0;
    }
}

/// A component in its track unit: px, percent, degrees or a scale factor.
fn component(name: &str, s: &str) -> Result<f32, String> {
    let s = s.trim();
    let (n, unit) = match name {
        "translate-x" | "translate-y" | "translate-z" => (s.strip_suffix("px").unwrap_or(s), 1.0),
        "translate-x-percent" | "translate-y-percent" => (s.strip_suffix('%').unwrap_or(s), 1.0),
        "scale-x" | "scale-y" => (s, 1.0),
        _ => {
            if let Some(n) = s.strip_suffix("deg") {
                (n, 1.0)
            } else if let Some(n) = s.strip_suffix("turn") {
                (n, 360.0)
            } else if let Some(n) = s.strip_suffix("rad") {
                (n, 180.0 / std::f32::consts::PI)
            } else {
                (s, 1.0)
            }
        }
    };
    Ok(number(n.trim())? * unit)
}
fn base_value(name: &str, base: &Values, paint: &PaintStyle, m: Metrics) -> Result<Value, String> {
    if let Some(s) = base.get(name) {
        return value(name, s, m, paint.color);
    }
    Ok(match name {
        "opacity" => Value::Scalar(1.0),
        "background-color" => Value::Color(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }),
        "color" => Value::Color(paint.color),
        "transform" => Value::Transform(TransformList::default()),
        "border-top-left-radius" => Value::Scalar(0.0),
        _ => return Err(format!("animation '{name}' needs an explicit base value")),
    })
}
fn time(s: &str) -> Result<f64, String> {
    if let Some(s) = s.strip_suffix("ms") {
        Ok(number(s)? as f64 / 1000.0)
    } else if let Some(s) = s.strip_suffix('s') {
        Ok(number(s)? as f64)
    } else {
        Err(format!("invalid CSS time '{s}'"))
    }
}

pub fn compile(
    base: &Values,
    css: &Css,
    paint: &PaintStyle,
    m: Metrics,
) -> Result<Vec<Animation>, String> {
    let names = split(
        base.get("animation-name")
            .map(String::as_str)
            .unwrap_or("none"),
        true,
    );
    let list = |key: &str, default: &str, i: usize| -> String {
        let v = split(base.get(key).map(String::as_str).unwrap_or(default), true);
        v[i % v.len()].clone()
    };
    let mut animations = Vec::new();
    for (i, name) in names.iter().enumerate() {
        if name == "none" {
            continue;
        }
        let frames = css
            .keyframes
            .get(name)
            .ok_or_else(|| format!("missing @keyframes '{name}'"))?;
        let duration = time(&list("animation-duration", "0s", i))?;
        let delay = time(&list("animation-delay", "0s", i))?;
        if duration <= 0.0 {
            return Err("animation duration must be positive".into());
        }
        let count = list("animation-iteration-count", "1", i);
        let count = if count == "infinite" {
            f64::INFINITY
        } else {
            number(&count)? as f64
        };
        if count < 0.0 {
            return Err("negative animation count".into());
        }
        let direction = list("animation-direction", "normal", i);
        if !matches!(
            direction.as_str(),
            "normal" | "reverse" | "alternate" | "alternate-reverse"
        ) {
            return Err("unsupported animation direction".into());
        }
        let fill = list("animation-fill-mode", "none", i);
        if !matches!(fill.as_str(), "none" | "forwards" | "backwards" | "both") {
            return Err("unsupported fill mode".into());
        }
        if list("animation-play-state", "running", i) != "running" {
            return Err("paused CSS animations require extraction".into());
        }
        for key in [
            "animation-timeline",
            "animation-range-start",
            "animation-range-end",
            "animation-composition",
        ] {
            if let Some(v) = base.get(key) {
                if !matches!(v.as_str(), "auto" | "normal" | "replace") {
                    return Err(format!("{key}: {v} unsupported"));
                }
            }
        }
        let ease = Ease::parse(&list("animation-timing-function", "ease", i))?;
        let mut names: Vec<_> = frames
            .iter()
            .flat_map(|f| f.values.keys())
            .filter(|k| *k != "animation-timing-function")
            .cloned()
            .collect();
        names.sort();
        names.dedup();
        let mut channels = Vec::new();
        for property in names {
            // radius shorthands expand to four identical corners; animate one channel.
            if matches!(
                property.as_str(),
                "border-top-right-radius"
                    | "border-bottom-left-radius"
                    | "border-bottom-right-radius"
            ) {
                if frames.iter().any(|frame| {
                    frame
                        .values
                        .get(&property)
                        .is_some_and(|v| frame.values.get("border-top-left-radius") != Some(v))
                }) {
                    return Err("only uniform corner-radius animation is supported".into());
                }
                continue;
            }
            let mut points = Vec::new();
            for frame in frames {
                if let Some(s) = frame.values.get(&property) {
                    let e = frame
                        .values
                        .get("animation-timing-function")
                        .map(|s| Ease::parse(s))
                        .transpose()?
                        .unwrap_or_else(|| ease.clone());
                    let v = value(&property, s, m, paint.color)?;
                    if let Some(previous) = points.last_mut() {
                        let previous: &mut Point = previous;
                        if previous.offset == frame.offset {
                            previous.value = v;
                            previous.ease = e;
                            continue;
                        }
                    }
                    points.push(Point {
                        offset: frame.offset,
                        value: v,
                        ease: e,
                    });
                }
            }
            if points.is_empty() {
                continue;
            }
            if points[0].offset > 0.0 {
                points.insert(
                    0,
                    Point {
                        offset: 0.0,
                        value: base_value(&property, base, paint, m)?,
                        ease: ease.clone(),
                    },
                );
            }
            if points.last().unwrap().offset < 1.0 {
                points.push(Point {
                    offset: 1.0,
                    value: base_value(&property, base, paint, m)?,
                    ease: ease.clone(),
                });
            }
            for pair in points.windows(2) {
                interpolate(&pair[0].value, &pair[1].value, 0.5)?;
            }
            channels.push(Channel {
                name: property,
                points,
            });
        }
        animations.push(Animation {
            delay,
            duration,
            count,
            direction,
            fill,
            channels,
        });
    }
    Ok(animations)
}

/// One timeline track as a single-iteration animation that holds its first
/// value before the first keyframe and its last value after the last. A
/// `text` track shows the element's own text before its first keyframe.
pub(crate) fn from_track(
    track: &crate::timeline::TimelineTrack,
    paint: &PaintStyle,
    m: Metrics,
) -> Result<Animation, String> {
    use crate::timeline::TimelineValue;
    let keys = &track.keyframes;
    let first = keys.first().ok_or("timeline track has no keyframes")?;
    let last = keys.last().unwrap_or(first);
    let span = last.time - first.time;
    // A single keyframe holds its value for the whole timeline.
    let duration = if span > 0.0 { span } else { 1.0 };
    let property = track.property.as_str();
    let mut points = Vec::with_capacity(keys.len().max(2));
    for (i, key) in keys.iter().enumerate() {
        let text = match &key.value {
            TimelineValue::Text(s) => s.clone(),
            TimelineValue::Number(n) => match property {
                "opacity" => n.to_string(),
                "width" | "height" | "left" | "top" | "border-top-left-radius" | "font-size" | "letter-spacing"
                | "word-spacing" => {
                    format!("{n}px")
                }
                name if border_side(name).is_some_and(|(_, field)| field == "width") => format!("{n}px"),
                name if box_side(name).is_some() => format!("{n}px"),
                "line-height" => n.to_string(),
                name if is_component(name) => n.to_string(),
                "filter-drop-shadow-color" => return Err(format!("timeline '{property}' takes CSS text")),
                name if indexed(name).is_some_and(|(_, _, field)| field == "color") => {
                    return Err(format!("timeline '{property}' takes CSS text"))
                }
                name if is_filter(name) || indexed(name).is_some() => n.to_string(),
                "text" => n.to_string(),
                "color" | "background-color" | "transform" => {
                    return Err(format!("timeline '{property}' takes CSS text"))
                }
                name if border_side(name).is_some() => return Err(format!("timeline '{property}' takes CSS text")),
                _ => return Err(format!("unsupported animated property '{property}'")),
            },
        };
        let ease = match keys.get(i + 1).and_then(|next| next.ease.as_deref()) {
            Some(ease) => Ease::parse(ease)?,
            None => Ease::Curve(EasingType::Linear),
        };
        points.push(Point {
            offset: ((key.time - first.time) / duration) as f32,
            value: value(property, &text, m, paint.color)?,
            ease,
        });
    }
    if points.len() == 1 {
        let mut end = points[0].clone();
        end.offset = 1.0;
        points.push(end);
    }
    for pair in points.windows(2) {
        interpolate(&pair[0].value, &pair[1].value, 0.5)?;
    }
    Ok(Animation {
        delay: first.time,
        duration,
        count: 1.0,
        direction: "normal".into(),
        // Before its first key a text track leaves the element's own text.
        fill: if property == "text" { "forwards" } else { "both" }.into(),
        channels: vec![Channel {
            name: track.property.clone(),
            points,
        }],
    })
}

impl Animation {
    /// Names of the properties this animation drives.
    pub(crate) fn properties(&self) -> impl Iterator<Item = &str> {
        self.channels.iter().map(|c| c.name.as_str())
    }

    /// The channels that reach an anonymous text leaf: the inherited text
    /// channels and text tracks.
    pub(crate) fn text_part(&self) -> Option<Animation> {
        let channels: Vec<Channel> =
            self.channels.iter().filter(|c| c.name == "text" || is_inherited(&c.name)).cloned().collect();
        (!channels.is_empty()).then(|| Animation { channels, ..self.clone() })
    }

    /// The inherited text channels `keep` accepts, with font sizes scaled by
    /// `font_scale` for a descendant whose own size is relative.
    pub(crate) fn inherited(&self, keep: impl Fn(&str) -> bool, font_scale: f32) -> Option<Animation> {
        let channels: Vec<Channel> = self
            .channels
            .iter()
            .filter(|c| is_inherited(&c.name) && keep(&c.name))
            .cloned()
            .map(|mut channel| {
                if channel.name == "font-size" {
                    for point in &mut channel.points {
                        if let Value::Scalar(v) = &mut point.value {
                            *v *= font_scale;
                        }
                    }
                }
                channel
            })
            .collect();
        (!channels.is_empty()).then(|| Animation { channels, ..self.clone() })
    }
}

/// Channels whose CSS properties descendants inherit: color, font size,
/// letter spacing, line height and text shadows.
pub(crate) fn is_inherited(name: &str) -> bool {
    matches!(name, "color" | "font-size" | "letter-spacing" | "word-spacing" | "line-height")
        || indexed(name).is_some_and(|(list, _, _)| list == "text-shadow")
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn mix_length(
    a: &LengthPercentage,
    b: &LengthPercentage,
    t: f32,
) -> Result<LengthPercentage, String> {
    match (a, b) {
        (LengthPercentage::Percentage(a), LengthPercentage::Percentage(b)) => {
            Ok(LengthPercentage::Percentage(
                lightningcss::values::percentage::Percentage(mix(a.0, b.0, t)),
            ))
        }
        (LengthPercentage::Dimension(a), LengthPercentage::Dimension(b)) => {
            Ok(LengthPercentage::px(mix(
                a.to_px().ok_or("animated transform requires px units")?,
                b.to_px().ok_or("animated transform requires px units")?,
                t,
            )))
        }
        _ => Err("animated transform units must agree".into()),
    }
}
fn mix_abs(a: &Length, b: &Length, t: f32) -> Result<Length, String> {
    Ok(Length::px(mix(
        a.to_px().ok_or("animated transform requires px units")?,
        b.to_px().ok_or("animated transform requires px units")?,
        t,
    )))
}
fn mix_number(a: &NumberOrPercentage, b: &NumberOrPercentage, t: f32) -> NumberOrPercentage {
    let n = |n: &NumberOrPercentage| match n {
        NumberOrPercentage::Number(n) => *n,
        NumberOrPercentage::Percentage(p) => p.0,
    };
    NumberOrPercentage::Number(mix(n(a), n(b), t))
}
fn identity(t: &Transform) -> Result<Transform, String> {
    Ok(match t {
        Transform::Translate(..) => {
            Transform::Translate(LengthPercentage::px(0.0), LengthPercentage::px(0.0))
        }
        Transform::TranslateX(_) => Transform::TranslateX(LengthPercentage::px(0.0)),
        Transform::TranslateY(_) => Transform::TranslateY(LengthPercentage::px(0.0)),
        Transform::Scale(..) => Transform::Scale(
            NumberOrPercentage::Number(1.0),
            NumberOrPercentage::Number(1.0),
        ),
        Transform::ScaleX(_) => Transform::ScaleX(NumberOrPercentage::Number(1.0)),
        Transform::ScaleY(_) => Transform::ScaleY(NumberOrPercentage::Number(1.0)),
        Transform::Rotate(_) => Transform::Rotate(Angle::Deg(0.0)),
        Transform::RotateX(_) => Transform::RotateX(Angle::Deg(0.0)),
        Transform::RotateY(_) => Transform::RotateY(Angle::Deg(0.0)),
        Transform::RotateZ(_) => Transform::RotateZ(Angle::Deg(0.0)),
        Transform::TranslateZ(_) => Transform::TranslateZ(Length::px(0.0)),
        Transform::Translate3d(..) => Transform::Translate3d(
            LengthPercentage::px(0.0),
            LengthPercentage::px(0.0),
            Length::px(0.0),
        ),
        Transform::Perspective(_) => Transform::Perspective(Length::px(1.0e6)),
        _ => {
            return Err(
                "transform interpolation requires matching translate/scale/rotate functions".into(),
            )
        }
    })
}
fn interpolate(a: &Value, b: &Value, t: f32) -> Result<Value, String> {
    Ok(match (a, b) {
        (Value::Scalar(a), Value::Scalar(b)) => Value::Scalar(mix(*a, *b, t)),
        (Value::Color(a), Value::Color(b)) => {
            let alpha = mix(a.a, b.a, t);
            let channel = |a: f32, b: f32| {
                if alpha > 0.0 {
                    mix(a, b, t) / alpha
                } else {
                    0.0
                }
            };
            Value::Color(Color {
                r: channel(a.r * a.a, b.r * b.a),
                g: channel(a.g * a.a, b.g * b.a),
                b: channel(a.b * a.a, b.b * b.a),
                a: alpha,
            })
        }
        (Value::Transform(a), Value::Transform(b)) => {
            let a = if a.0.is_empty() {
                TransformList(b.0.iter().map(identity).collect::<Result<_, _>>()?)
            } else {
                a.clone()
            };
            let b = if b.0.is_empty() {
                TransformList(a.0.iter().map(identity).collect::<Result<_, _>>()?)
            } else {
                b.clone()
            };
            if a.0.len() != b.0.len() {
                return Err("transform function lists must match".into());
            }
            let mut list = Vec::new();
            for (a, b) in a.0.iter().zip(&b.0) {
                list.push(match(a,b){
                (Transform::Translate(ax,ay),Transform::Translate(bx,by))=>Transform::Translate(mix_length(ax,bx,t)?,mix_length(ay,by,t)?),
                (Transform::TranslateX(a),Transform::TranslateX(b))=>Transform::TranslateX(mix_length(a,b,t)?),
                (Transform::TranslateY(a),Transform::TranslateY(b))=>Transform::TranslateY(mix_length(a,b,t)?),
                (Transform::Scale(ax,ay),Transform::Scale(bx,by))=>Transform::Scale(mix_number(ax,bx,t),mix_number(ay,by,t)),
                (Transform::ScaleX(a),Transform::ScaleX(b))=>Transform::ScaleX(mix_number(a,b,t)),(Transform::ScaleY(a),Transform::ScaleY(b))=>Transform::ScaleY(mix_number(a,b,t)),
                (Transform::Rotate(a),Transform::Rotate(b))=>Transform::Rotate(Angle::Deg(mix(a.to_degrees(),b.to_degrees(),t))),
                (Transform::RotateX(a),Transform::RotateX(b))=>Transform::RotateX(Angle::Deg(mix(a.to_degrees(),b.to_degrees(),t))),
                (Transform::RotateY(a),Transform::RotateY(b))=>Transform::RotateY(Angle::Deg(mix(a.to_degrees(),b.to_degrees(),t))),
                (Transform::RotateZ(a),Transform::RotateZ(b))=>Transform::RotateZ(Angle::Deg(mix(a.to_degrees(),b.to_degrees(),t))),
                (Transform::TranslateZ(a),Transform::TranslateZ(b))=>Transform::TranslateZ(mix_abs(a,b,t)?),
                (Transform::Translate3d(ax,ay,az),Transform::Translate3d(bx,by,bz))=>Transform::Translate3d(mix_length(ax,bx,t)?,mix_length(ay,by,t)?,mix_abs(az,bz,t)?),
                (Transform::Perspective(a),Transform::Perspective(b))=>Transform::Perspective(mix_abs(a,b,t)?),
                _=>return Err("transform interpolation requires matching translate/scale/rotate functions".into()),
            });
            }
            Value::Transform(TransformList(list))
        }
        (Value::Text(a), Value::Text(b)) => Value::Text(if t < 1.0 { a } else { b }.clone()),
        _ => return Err("animation value type mismatch".into()),
    })
}

impl Animation {
    /// Apply this animation at `time`. Component channels write into `parts`,
    /// which the caller composes into the transform after every animation.
    pub fn apply(
        &self,
        time: f64,
        style: &mut taffy::Style,
        paint: &mut PaintStyle,
        parts: &mut Option<Parts>,
    ) -> bool {
        let elapsed = time - self.delay;
        let end = self.duration * self.count;
        if (elapsed < 0.0 && !matches!(self.fill.as_str(), "both" | "backwards"))
            || (elapsed >= end && !matches!(self.fill.as_str(), "both" | "forwards"))
        {
            return false;
        }
        if self.count == 0.0 {
            return false;
        }
        let (iteration, mut progress) = if elapsed <= 0.0 {
            (0u64, 0.0)
        } else if elapsed >= end {
            let count = self.count;
            let full = count.floor();
            if count == full {
                ((full as u64).saturating_sub(1), 1.0)
            } else {
                (full as u64, (count - full) as f32)
            }
        } else {
            (
                (elapsed / self.duration).floor() as u64,
                ((elapsed % self.duration) / self.duration) as f32,
            )
        };
        let reverse = match self.direction.as_str() {
            "reverse" => true,
            "alternate" => iteration % 2 == 1,
            "alternate-reverse" => iteration % 2 == 0,
            _ => false,
        };
        if reverse {
            progress = 1.0 - progress;
        }
        let mut layout = false;
        for channel in &self.channels {
            let points = &channel.points;
            let right = points
                .partition_point(|p| p.offset <= progress)
                .min(points.len() - 1);
            let left = right.saturating_sub(1);
            let (a, b) = (&points[left], &points[right]);
            let t = if a.offset == b.offset {
                0.0
            } else {
                a.ease
                    .eval(((progress - a.offset) / (b.offset - a.offset)).clamp(0.0, 1.0))
            };
            let Ok(v) = interpolate(&a.value, &b.value, t) else {
                continue;
            };
            match (channel.name.as_str(), v) {
                ("opacity", Value::Scalar(v)) => paint.opacity = v.clamp(0.0, 1.0),
                ("color", Value::Color(v)) => paint.color = v,
                ("background-color", Value::Color(v)) => {
                    paint.box_node.bg_color = Some(director_core::animation::Animated::new(v))
                }
                // A whole transform replaces the components composed so far.
                ("transform", Value::Transform(v)) => {
                    paint.transform = v;
                    *parts = None;
                }
                ("text", Value::Text(v)) => paint.text = Some(v),
                // The element rebuilds its text spans and relayouts when these change.
                ("font-size", Value::Scalar(v)) => paint.font_size = v.max(0.0),
                ("letter-spacing", Value::Scalar(v)) => paint.letter_spacing = v,
                ("word-spacing", Value::Scalar(v)) => paint.word_spacing = v,
                ("line-height", Value::Scalar(v)) => paint.line_height = Some(v.max(0.0)),
                (name, Value::Scalar(v)) if box_side(name).is_some() => {
                    let side = box_side(name).map_or(0, |(_, side)| side);
                    if name.starts_with("padding") {
                        let v = taffy::LengthPercentage::length(v.max(0.0));
                        match side {
                            0 => style.padding.top = v,
                            1 => style.padding.right = v,
                            2 => style.padding.bottom = v,
                            _ => style.padding.left = v,
                        }
                    } else {
                        let v = taffy::LengthPercentageAuto::length(v);
                        match side {
                            0 => style.margin.top = v,
                            1 => style.margin.right = v,
                            2 => style.margin.bottom = v,
                            _ => style.margin.left = v,
                        }
                    }
                    layout = true;
                }
                ("border-top-left-radius", Value::Scalar(v)) => {
                    paint.corners = [(
                        taffy::LengthPercentage::length(v.max(0.0)),
                        taffy::LengthPercentage::length(v.max(0.0)),
                    ); 4];
                }
                ("filter-drop-shadow-color", Value::Color(c)) => set_filter_color(&mut paint.filters, c),
                (name, Value::Scalar(v)) if is_filter(name) => set_filter(&mut paint.filters, name, v),
                (name, v) if indexed(name).is_some() => match (indexed(name), v) {
                    (Some(("box-shadow", i, field)), v) => set_shadow(&mut paint.shadows, i, field, v),
                    (Some(("text-shadow", i, field)), v) => set_text_shadow(&mut paint.text_shadows, i, field, v),
                    (Some(("background-position", i, field)), Value::Scalar(v)) => {
                        let list = if field.starts_with('x') {
                            &mut paint.background_position_x
                        } else {
                            &mut paint.background_position_y
                        };
                        set_position(list, i, field.ends_with("percent"), v)
                    }
                    (Some(("background-size", i, field)), Value::Scalar(v)) => {
                        set_size(&mut paint.background_size, i, field, v)
                    }
                    (Some((_, i, field)), Value::Scalar(v)) => set_clip(&mut paint.box_node.clip_path, i, field, v),
                    _ => {}
                },
                (name, Value::Color(c)) if border_side(name).is_some() => {
                    paint.border_colors[border_side(name).map_or(0, |(side, _)| side)] = c
                }
                (name, Value::Scalar(v)) if border_side(name).is_some() => {
                    let width = taffy::prelude::length(v.max(0.0));
                    match border_side(name).map_or(0, |(side, _)| side) {
                        0 => style.border.top = width,
                        1 => style.border.right = width,
                        2 => style.border.bottom = width,
                        _ => style.border.left = width,
                    }
                    layout = true;
                }
                (name, Value::Scalar(v)) if is_component(name) => parts
                    .get_or_insert_with(|| Parts::decompose(&paint.transform).unwrap_or_default())
                    .set(name, v),
                (name, Value::Scalar(v)) => {
                    match name {
                        "width" => style.size.width = taffy::prelude::length(v),
                        "height" => style.size.height = taffy::prelude::length(v),
                        "left" => style.inset.left = taffy::prelude::length(v),
                        "top" => style.inset.top = taffy::prelude::length(v),
                        _ => {}
                    }
                    layout = true;
                }
                _ => {}
            }
        }
        layout
    }
}

impl Animation {
    /// The scene document form of this animation.
    pub(crate) fn to_doc(&self) -> Result<doc::CssAnimation, ExportError> {
        let channels = self
            .channels
            .iter()
            .map(|channel| {
                let keyframes = channel
                    .points
                    .iter()
                    .map(|point| {
                        Ok(doc::CssKeyframe {
                            offset: point.offset,
                            value: match &point.value {
                                Value::Scalar(v) => doc::CssValue::Scalar(*v),
                                Value::Color(c) => doc::CssValue::Color(rgba(*c)),
                                Value::Transform(list) => doc::CssValue::Transform(
                                    list.to_css_string(Default::default())
                                        .map_err(|e| ExportError::Unrepresentable(format!("animation transform: {e}")))?,
                                ),
                                Value::Text(text) => doc::CssValue::Text(text.clone()),
                            },
                            timing: match &point.ease {
                                Ease::Curve(easing) => doc::CssTiming::Curve(export_easing(easing)),
                                Ease::Steps(count, start) => doc::CssTiming::Steps {
                                    count: *count,
                                    start: *start,
                                },
                                Ease::Gsap(ease) => doc::CssTiming::Gsap(ease.text()),
                            },
                        })
                    })
                    .collect::<Result<_, ExportError>>()?;
                Ok(doc::CssChannel {
                    property: channel.name.clone(),
                    keyframes,
                })
            })
            .collect::<Result<_, ExportError>>()?;
        Ok(doc::CssAnimation {
            delay: self.delay,
            duration: self.duration,
            iteration_count: self.count.is_finite().then_some(self.count),
            direction: self.direction.clone(),
            fill_mode: self.fill.clone(),
            channels,
        })
    }
}

impl Animation {
    /// Rebuild an animation from its scene document form.
    pub(crate) fn from_doc(animation: &doc::CssAnimation) -> Result<Self, ImportError> {
        let channels = animation
            .channels
            .iter()
            .map(|channel| {
                let points = channel
                    .keyframes
                    .iter()
                    .map(|keyframe| {
                        Ok(Point {
                            offset: keyframe.offset,
                            value: match &keyframe.value {
                                doc::CssValue::Scalar(v) => Value::Scalar(*v),
                                doc::CssValue::Color(c) => Value::Color(import_color(*c)),
                                doc::CssValue::Transform(css) => Value::Transform(
                                    TransformList::parse_string(css)
                                        .map_err(|e| ImportError::Invalid(format!("animation transform `{css}`: {e:?}")))?,
                                ),
                                doc::CssValue::Text(text) => Value::Text(text.clone()),
                            },
                            ease: match &keyframe.timing {
                                doc::CssTiming::Curve(easing) => Ease::Curve(import_easing(easing)),
                                doc::CssTiming::Steps { count, start } => Ease::Steps(*count, *start),
                                doc::CssTiming::Gsap(text) => {
                                    Ease::Gsap(GsapEase::parse(text).map_err(ImportError::Invalid)?)
                                }
                            },
                        })
                    })
                    .collect::<Result<_, ImportError>>()?;
                Ok(Channel {
                    name: channel.property.clone(),
                    points,
                })
            })
            .collect::<Result<_, ImportError>>()?;
        Ok(Self {
            delay: animation.delay,
            duration: animation.duration,
            count: animation.iteration_count.unwrap_or(f64::INFINITY),
            direction: animation.direction.clone(),
            fill: animation.fill_mode.clone(),
            channels,
        })
    }
}

/// A transform split into GSAP components: px and percent translation,
/// rotations and skews in degrees, and scale factors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parts {
    tx: f32,
    ty: f32,
    tz: f32,
    xp: f32,
    yp: f32,
    rz: f32,
    rx: f32,
    ry: f32,
    skx: f32,
    sky: f32,
    sx: f32,
    sy: f32,
}
impl Default for Parts {
    fn default() -> Self {
        Self {
            tx: 0.0,
            ty: 0.0,
            tz: 0.0,
            xp: 0.0,
            yp: 0.0,
            rz: 0.0,
            rx: 0.0,
            ry: 0.0,
            skx: 0.0,
            sky: 0.0,
            sx: 1.0,
            sy: 1.0,
        }
    }
}
impl Parts {
    /// Split a transform written as translations, then rotate, rotateY,
    /// rotateX, one skew and scales, in that order. Anything else has no
    /// component form.
    pub(crate) fn decompose(list: &TransformList) -> Option<Self> {
        let mut parts = Self::default();
        let mut rank = 0;
        let mut skewed = false;
        let translate = |parts: &mut Self, x: &LengthPercentage, y: &LengthPercentage| {
            for (v, axis) in [(x, 0), (y, 1)] {
                match v {
                    LengthPercentage::Percentage(p) => {
                        *[&mut parts.xp, &mut parts.yp][axis] += p.0 * 100.0
                    }
                    LengthPercentage::Dimension(d) => {
                        *[&mut parts.tx, &mut parts.ty][axis] += d.to_px()?
                    }
                    _ => return None,
                }
            }
            Some(())
        };
        let zero = LengthPercentage::px(0.0);
        for t in &list.0 {
            let next = match t {
                Transform::Translate(x, y) => {
                    translate(&mut parts, x, y)?;
                    0
                }
                Transform::TranslateX(x) => {
                    translate(&mut parts, x, &zero)?;
                    0
                }
                Transform::TranslateY(y) => {
                    translate(&mut parts, &zero, y)?;
                    0
                }
                Transform::TranslateZ(z) => {
                    parts.tz += z.to_px()?;
                    0
                }
                Transform::Translate3d(x, y, z) => {
                    translate(&mut parts, x, y)?;
                    parts.tz += z.to_px()?;
                    0
                }
                Transform::Rotate(a) | Transform::RotateZ(a) => {
                    parts.rz += a.to_degrees();
                    1
                }
                Transform::RotateY(a) => {
                    parts.ry += a.to_degrees();
                    2
                }
                Transform::RotateX(a) => {
                    parts.rx += a.to_degrees();
                    3
                }
                Transform::Skew(..) | Transform::SkewX(_) | Transform::SkewY(_) if !skewed => {
                    skewed = true;
                    match t {
                        Transform::Skew(x, y) => (parts.skx, parts.sky) = (x.to_degrees(), y.to_degrees()),
                        Transform::SkewX(x) => parts.skx = x.to_degrees(),
                        Transform::SkewY(y) => parts.sky = y.to_degrees(),
                        _ => unreachable!(),
                    }
                    4
                }
                Transform::Scale(x, y) => {
                    parts.sx *= factor(x);
                    parts.sy *= factor(y);
                    5
                }
                Transform::ScaleX(x) => {
                    parts.sx *= factor(x);
                    5
                }
                Transform::ScaleY(y) => {
                    parts.sy *= factor(y);
                    5
                }
                _ => return None,
            };
            if next < rank {
                return None;
            }
            rank = next;
        }
        Some(parts)
    }

    pub(crate) fn set(&mut self, name: &str, v: f32) {
        let slot = match name {
            "translate-x" => &mut self.tx,
            "translate-y" => &mut self.ty,
            "translate-z" => &mut self.tz,
            "translate-x-percent" => &mut self.xp,
            "translate-y-percent" => &mut self.yp,
            "rotate-z" => &mut self.rz,
            "rotate-x" => &mut self.rx,
            "rotate-y" => &mut self.ry,
            "skew-x" => &mut self.skx,
            "skew-y" => &mut self.sky,
            "scale-x" => &mut self.sx,
            "scale-y" => &mut self.sy,
            _ => return,
        };
        *slot = v;
    }

    pub(crate) fn get(&self, name: &str) -> Option<f32> {
        Some(match name {
            "translate-x" => self.tx,
            "translate-y" => self.ty,
            "translate-z" => self.tz,
            "translate-x-percent" => self.xp,
            "translate-y-percent" => self.yp,
            "rotate-z" => self.rz,
            "rotate-x" => self.rx,
            "rotate-y" => self.ry,
            "skew-x" => self.skx,
            "skew-y" => self.sky,
            "scale-x" => self.sx,
            "scale-y" => self.sy,
            _ => return None,
        })
    }

    /// GSAP's CSS transform order, leaving out identity functions.
    pub(crate) fn compose(&self) -> TransformList {
        use lightningcss::values::percentage::Percentage;
        let mut list = Vec::new();
        if self.xp != 0.0 || self.yp != 0.0 {
            list.push(Transform::Translate(
                LengthPercentage::Percentage(Percentage(self.xp / 100.0)),
                LengthPercentage::Percentage(Percentage(self.yp / 100.0)),
            ));
        }
        if self.tz != 0.0 {
            list.push(Transform::Translate3d(
                LengthPercentage::px(self.tx),
                LengthPercentage::px(self.ty),
                Length::px(self.tz),
            ));
        } else if self.tx != 0.0 || self.ty != 0.0 {
            list.push(Transform::Translate(
                LengthPercentage::px(self.tx),
                LengthPercentage::px(self.ty),
            ));
        }
        if self.rz != 0.0 {
            list.push(Transform::Rotate(Angle::Deg(self.rz)));
        }
        if self.ry != 0.0 {
            list.push(Transform::RotateY(Angle::Deg(self.ry)));
        }
        if self.rx != 0.0 {
            list.push(Transform::RotateX(Angle::Deg(self.rx)));
        }
        if self.skx != 0.0 || self.sky != 0.0 {
            list.push(Transform::Skew(Angle::Deg(self.skx), Angle::Deg(self.sky)));
        }
        if self.sx != 1.0 || self.sy != 1.0 {
            list.push(Transform::Scale(
                NumberOrPercentage::Number(self.sx),
                NumberOrPercentage::Number(self.sy),
            ));
        }
        TransformList(list)
    }
}
fn factor(n: &NumberOrPercentage) -> f32 {
    match n {
        NumberOrPercentage::Number(n) => *n,
        NumberOrPercentage::Percentage(p) => p.0,
    }
}

/// Component channels compose over a decomposable base transform and never
/// share an element with a whole `transform` channel.
pub(crate) fn check_components(animations: &[Animation], base: &TransformList) -> Result<(), String> {
    let properties = || animations.iter().flat_map(Animation::properties);
    if !properties().any(is_component) {
        return Ok(());
    }
    if properties().any(|p| p == "transform") {
        return Err("a transform track cannot share an element with transform component tracks".into());
    }
    if Parts::decompose(base).is_none() {
        return Err(
            "transform component tracks need a base transform of translations, rotations, one skew and scales in GSAP order"
                .into(),
        );
    }
    Ok(())
}
