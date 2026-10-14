use crate::{
    css::{split, Css, Values},
    style::{color, number, px, Metrics, PaintStyle},
};
use director_core::{animation::EasingType, types::Color};
use lightningcss::{
    properties::transform::{Transform, TransformList},
    traits::Parse,
    values::{length::LengthPercentage, percentage::NumberOrPercentage},
};

#[derive(Debug, Clone)]
enum Ease {
    Curve(EasingType),
    Steps(u32, bool),
}
impl Ease {
    fn parse(s: &str) -> Result<Self, String> {
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
        }
    }
}

#[derive(Debug, Clone)]
enum Value {
    Scalar(f32),
    Color(Color),
    Transform(TransformList),
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
        "width" | "height" | "left" | "top" | "border-top-left-radius" => Value::Scalar(px(s, m)?),
        _ => return Err(format!("unsupported animated property '{name}'")),
    })
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
        Transform::Rotate(_) => Transform::Rotate(lightningcss::values::angle::Angle::Deg(0.0)),
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
                (Transform::Rotate(a),Transform::Rotate(b))=>Transform::Rotate(lightningcss::values::angle::Angle::Deg(mix(a.to_degrees(),b.to_degrees(),t))),
                _=>return Err("transform interpolation requires matching translate/scale/rotate functions".into()),
            });
            }
            Value::Transform(TransformList(list))
        }
        _ => return Err("animation value type mismatch".into()),
    })
}

impl Animation {
    pub fn apply(&self, time: f64, style: &mut taffy::Style, paint: &mut PaintStyle) -> bool {
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
                ("transform", Value::Transform(v)) => paint.transform = v,
                ("border-top-left-radius", Value::Scalar(v)) => {
                    paint.corners = [(
                        taffy::LengthPercentage::length(v.max(0.0)),
                        taffy::LengthPercentage::length(v.max(0.0)),
                    ); 4];
                }
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
