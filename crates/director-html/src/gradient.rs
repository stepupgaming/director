use crate::style::{self, LengthParts, Metrics};
use director_core::types::Color;
use lightningcss::{
    traits::{Parse, ToCss},
    values::{
        gradient::{
            Circle, Ellipse, EndingShape, Gradient, GradientItem, LineDirection, ShapeExtent,
        },
        length::LengthPercentage as CssLength,
        position::Position,
    },
    vendor_prefix::VendorPrefix,
};
use skia_safe::{Matrix, Rect, Shader, TileMode};
use taffy::LengthPercentage;

#[derive(Debug, Clone)]
pub struct NativeGradient {
    kind: Kind,
    repeating: bool,
    colors: Vec<Option<skia_safe::Color>>,
    stops: Vec<Option<StopPos>>,
}
#[derive(Debug, Clone)]
enum StopPos {
    Fixed(LengthPercentage),
    Mixed(CssLength),
}
#[derive(Debug, Clone)]
enum LinearTo {
    Angle(f32),
    /// `to <side> <side>`. The angle depends on the box being painted.
    Corner { right: bool, bottom: bool },
}
#[derive(Debug, Clone)]
enum Kind {
    Linear(LinearTo),
    Radial {
        center: (LengthPercentage, LengthPercentage),
        radius: Radius,
    },
    Conic {
        center: (LengthPercentage, LengthPercentage),
        angle: f32,
    },
}
#[derive(Debug, Clone)]
enum Radius {
    CircleExtent(ShapeExtent),
    EllipseExtent(ShapeExtent),
    Circle(f32),
    Ellipse(LengthPercentage, LengthPercentage),
}

/// CSS `to <corner>` is perpendicular to the line through the two neighboring corners.
fn corner_angle(right: bool, bottom: bool, width: f32, height: f32) -> f32 {
    let sx = if right { 1.0 } else { -1.0 };
    let sy = if bottom { 1.0 } else { -1.0 };
    let dx = sx * height.max(0.0);
    let dy = sy * width.max(0.0);
    dx.atan2(-dy).to_degrees()
}

fn position(p: &Position, m: Metrics) -> Result<(LengthPercentage, LengthPercentage), String> {
    let axis = |s: String| -> Result<LengthPercentage, String> {
        match s.as_str() {
            "center" => Ok(LengthPercentage::percent(0.5)),
            "left" | "top" => Ok(LengthPercentage::percent(0.0)),
            "right" | "bottom" => Ok(LengthPercentage::percent(1.0)),
            _ => style::length(&s, m),
        }
    };
    Ok((
        axis(
            p.x.to_css_string(Default::default())
                .map_err(|e| e.to_string())?,
        )?,
        axis(
            p.y.to_css_string(Default::default())
                .map_err(|e| e.to_string())?,
        )?,
    ))
}
fn stop_position<D: ToCss>(
    position: &D,
    m: Metrics,
    conic: bool,
) -> Result<StopPos, String> {
    let s = position
        .to_css_string(Default::default())
        .map_err(|e| e.to_string())?;
    if conic {
        let value = if let Some(s) = s.strip_suffix('%') {
            style::number(s)? / 100.0
        } else {
            lightningcss::values::angle::Angle::parse_string(&s)
                .map_err(|e| format!("conic stop: {e:?}"))?
                .to_degrees()
                / 360.0
        };
        Ok(StopPos::Fixed(LengthPercentage::percent(value)))
    } else {
        match style::length(&s, m) {
            Ok(length) => Ok(StopPos::Fixed(length)),
            Err(err) if err.contains(crate::calc_len::MIXED_BASIS) => {
                match crate::calc_len::parse_simple_mixed_stop(&s) {
                    Some(value) => Ok(StopPos::Mixed(value)),
                    None => Err(err),
                }
            }
            Err(err) => Err(err),
        }
    }
}
type GradientStops = Result<(Vec<Option<skia_safe::Color>>, Vec<Option<StopPos>>), String>;

fn stops<D: ToCss>(
    items: Vec<GradientItem<D>>,
    m: Metrics,
    current: Color,
    conic: bool,
) -> GradientStops {
    let mut colors = Vec::new();
    let mut positions = Vec::new();
    for item in items {
        match item {
            GradientItem::ColorStop(stop) => {
                colors.push(Some(style::css_color(&stop.color, current)?.to_skia()));
                positions.push(
                    stop.position
                        .as_ref()
                        .map(|position| stop_position(position, m, conic))
                        .transpose()?,
                );
            }
            GradientItem::Hint(hint) => {
                colors.push(None);
                positions.push(Some(stop_position(&hint, m, conic)?));
            }
        }
    }
    for (i, color) in colors.iter().enumerate() {
        if color.is_none()
            && (i == 0
                || i + 1 == colors.len()
                || colors[i - 1].is_none()
                || colors[i + 1].is_none())
        {
            return Err("gradient color hint needs two surrounding stops".into());
        }
    }
    if colors.iter().flatten().count() < 2 {
        return Err("gradient needs at least two color stops".into());
    }
    Ok((colors, positions))
}
fn mix(from: skia_safe::Color, to: skia_safe::Color, t: f32) -> skia_safe::Color {
    let channel = |a: u8, b: u8| -> u8 {
        (f32::from(a) + (f32::from(b) - f32::from(a)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    skia_safe::Color::from_argb(
        channel(from.a(), to.a()),
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
    )
}
fn stop_offset(stop: &Option<StopPos>, basis: f32) -> Result<Option<f32>, String> {
    match stop {
        None => Ok(None),
        Some(StopPos::Fixed(length)) => Ok(Some(if length.is_percent() {
            length.value()
        } else {
            length.value() / basis.max(0.0001)
        })),
        Some(StopPos::Mixed(value)) => {
            if !(basis.is_finite() && basis > 0.0) {
                return Err(crate::calc_len::MIXED_BASIS.to_string());
            }
            let px = crate::calc_len::resolve_stop_basis(value, basis)?;
            let at = px / basis;
            if !at.is_finite() {
                return Err("non-finite length".into());
            }
            Ok(Some(at))
        }
    }
}
fn expand_hints(
    marks: &[Option<skia_safe::Color>],
    positions: &[f32],
) -> Result<(Vec<skia_safe::Color>, Vec<f32>), String> {
    let mut colors = Vec::new();
    let mut offsets = Vec::new();
    for (i, mark) in marks.iter().enumerate() {
        let Some(color) = mark else {
            if i == 0 || i + 1 == marks.len() {
                return Err("gradient color hint needs two surrounding stops".into());
            }
            let (Some(previous), Some(next)) = (marks[i - 1], marks[i + 1]) else {
                return Err("gradient color hint needs two surrounding stops".into());
            };
            let offset1 = positions[i - 1];
            let offset = positions[i];
            let offset2 = positions[i + 1];
            if offset - offset1 == offset2 - offset {
                continue;
            }
            if offset1 == offset {
                colors.push(next);
                offsets.push(offset);
                continue;
            }
            if offset2 == offset {
                colors.push(previous);
                offsets.push(offset);
                continue;
            }
            let midpoint = (offset - offset1) / (offset2 - offset1);
            let samples = if midpoint > 0.5 {
                let mut samples = [0.0; 9];
                for (y, sample) in samples.iter_mut().enumerate().take(7) {
                    *sample = offset1 + (offset - offset1) * (7 + y) as f32 / 13.0;
                }
                samples[7] = offset + (offset2 - offset) / 3.0;
                samples[8] = offset + (offset2 - offset) * 2.0 / 3.0;
                samples
            } else {
                let mut samples = [0.0; 9];
                samples[0] = offset1 + (offset - offset1) / 3.0;
                samples[1] = offset1 + (offset - offset1) * 2.0 / 3.0;
                for y in 0..7 {
                    samples[y + 2] = offset + (offset2 - offset) * y as f32 / 13.0;
                }
                samples
            };
            let exponent = 0.5f32.ln() / midpoint.ln();
            for position in samples {
                let relative = (position - offset1) / (offset2 - offset1);
                let t = relative.powf(exponent);
                let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
                colors.push(mix(previous, next, t));
                offsets.push(position);
            }
            continue;
        };
        colors.push(*color);
        offsets.push(positions[i]);
    }
    Ok((colors, offsets))
}

impl NativeGradient {
    pub fn parse(s: &str, m: Metrics, current: Color) -> Result<Self, String> {
        let gradient = Gradient::parse_string(s).map_err(|e| format!("gradient: {e:?}"))?;
        let repeating = matches!(
            gradient,
            Gradient::RepeatingLinear(_)
                | Gradient::RepeatingRadial(_)
                | Gradient::RepeatingConic(_)
        );
        let gradient = match gradient {
            Gradient::RepeatingLinear(g) => Gradient::Linear(g),
            Gradient::RepeatingRadial(g) => Gradient::Radial(g),
            Gradient::RepeatingConic(g) => Gradient::Conic(g),
            other => other,
        };
        if gradient.get_vendor_prefix() != VendorPrefix::None {
            return Err("vendor gradients unsupported".into());
        }
        let (kind, colors, positions) = match gradient {
            Gradient::Linear(g) => {
                let direction = match g.direction {
                    LineDirection::Angle(a) => LinearTo::Angle(a.to_degrees()),
                    LineDirection::Horizontal(h) => {
                        let left = h.to_css_string(Default::default()).map_err(|e| e.to_string())?
                            == "left";
                        LinearTo::Angle(if left { 270.0 } else { 90.0 })
                    }
                    LineDirection::Vertical(v) => {
                        let top = v.to_css_string(Default::default()).map_err(|e| e.to_string())?
                            == "top";
                        LinearTo::Angle(if top { 0.0 } else { 180.0 })
                    }
                    LineDirection::Corner {
                        horizontal,
                        vertical,
                    } => {
                        let right = horizontal
                            .to_css_string(Default::default())
                            .map_err(|e| e.to_string())?
                            == "right";
                        let bottom = vertical
                            .to_css_string(Default::default())
                            .map_err(|e| e.to_string())?
                            == "bottom";
                        LinearTo::Corner { right, bottom }
                    }
                };
                let (colors, positions) = stops(g.items, m, current, false)?;
                (Kind::Linear(direction), colors, positions)
            }
            Gradient::Radial(g) => {
                let radius = match g.shape {
                    EndingShape::Circle(Circle::Extent(e)) => Radius::CircleExtent(e),
                    EndingShape::Ellipse(Ellipse::Extent(e)) => Radius::EllipseExtent(e),
                    EndingShape::Circle(Circle::Radius(r)) => Radius::Circle(style::px(
                        &r.to_css_string(Default::default())
                            .map_err(|e| e.to_string())?,
                        m,
                    )?),
                    EndingShape::Ellipse(Ellipse::Size { x, y }) => Radius::Ellipse(
                        style::convert_length(&x, m)?,
                        style::convert_length(&y, m)?,
                    ),
                };
                let (colors, positions) = stops(g.items, m, current, false)?;
                (
                    Kind::Radial {
                        center: position(&g.position, m)?,
                        radius,
                    },
                    colors,
                    positions,
                )
            }
            Gradient::Conic(g) => {
                let (colors, positions) = stops(g.items, m, current, true)?;
                (
                    Kind::Conic {
                        center: position(&g.position, m)?,
                        angle: g.angle.to_degrees(),
                    },
                    colors,
                    positions,
                )
            }
            _ => return Err("vendor gradients unsupported".into()),
        };
        Ok(Self {
            kind,
            repeating,
            colors,
            stops: positions,
        })
    }
    fn cover(
        &self,
        source: &[skia_safe::Color],
        positions: &[f32],
    ) -> (Vec<skia_safe::Color>, Vec<f32>) {
        if !self.repeating {
            return (source.to_vec(), positions.to_vec());
        }
        let first = positions[0];
        let last = *positions.last().unwrap();
        let period = last - first;
        if !period.is_finite() || period < 1.0e-4 {
            let color = *source.last().unwrap();
            return (vec![color, color], vec![0.0, 1.0]);
        }
        let mut colors = Vec::new();
        let mut pos = Vec::new();
        let end = ((1.0 - first) / period).ceil() as i32 + 1;
        let start = (((-last) / period).floor() as i32 - 1).max(end - 240);
        for k in start..=end {
            let shift = k as f32 * period;
            for (color, stop) in source.iter().zip(positions) {
                let at = stop + shift;
                if !(-0.001..=1.001).contains(&at) {
                    continue;
                }
                if pos
                    .last()
                    .is_some_and(|prev| at + 1.0e-5 < *prev)
                {
                    continue;
                }
                colors.push(*color);
                pos.push(at.clamp(0.0, 1.0));
                if colors.len() == 720 {
                    break;
                }
            }
            if colors.len() == 720 {
                break;
            }
        }
        if colors.len() < 2 {
            let color = *source.last().unwrap();
            return (vec![color, color], vec![0.0, 1.0]);
        }
        if pos[0] > 0.0 {
            colors.insert(0, colors[0]);
            pos.insert(0, 0.0);
        }
        if *pos.last().unwrap() < 1.0 {
            let color = *colors.last().unwrap();
            colors.push(color);
            pos.push(1.0);
        }
        let mut out_colors = Vec::new();
        let mut out_pos = Vec::new();
        for (color, at) in colors.into_iter().zip(pos) {
            if let Some(prev) = out_pos.last().copied() {
                if at + 1.0e-6 < prev {
                    continue;
                }
                if (at - prev).abs() <= 1.0e-6 {
                    if out_colors.last() == Some(&color) {
                        continue;
                    }
                    out_colors.push(color);
                    out_pos.push(prev + 1.0e-4);
                    continue;
                }
            }
            out_colors.push(color);
            out_pos.push(at);
        }
        if out_colors.len() < 2 {
            let color = *source.last().unwrap();
            return (vec![color, color], vec![0.0, 1.0]);
        }
        (out_colors, out_pos)
    }
    fn positions(&self, basis: f32) -> Result<Vec<f32>, String> {
        let mut pos = Vec::with_capacity(self.stops.len());
        for stop in &self.stops {
            pos.push(stop_offset(stop, basis)?);
        }
        let n = pos.len();
        pos[0] = Some(pos[0].unwrap_or(0.0));
        pos[n - 1] = Some(pos[n - 1].unwrap_or(1.0));
        let mut previous = 0;
        for next in 1..n {
            if let Some(end) = pos[next] {
                let begin = pos[previous].unwrap();
                let end = end.max(begin);
                pos[next] = Some(end);
                for (i, p) in pos.iter_mut().enumerate().take(next).skip(previous + 1) {
                    *p = Some(
                        begin + (end - begin) * (i - previous) as f32 / (next - previous) as f32,
                    );
                }
                previous = next;
            }
        }
        Ok(pos.into_iter().flatten().collect())
    }
    fn ramp(&self, basis: f32) -> Option<(Vec<skia_safe::Color>, Vec<f32>)> {
        let positions = self.positions(basis).ok()?;
        let (colors, positions) = expand_hints(&self.colors, &positions).ok()?;
        Some(self.cover(&colors, &positions))
    }
    pub fn shader(&self, rect: Rect) -> Option<Shader> {
        self.shader_with(rect, TileMode::Clamp)
    }
    pub fn shader_with(&self, rect: Rect, tile: TileMode) -> Option<Shader> {
        match &self.kind {
            Kind::Linear(direction) => {
                let angle = match direction {
                    LinearTo::Angle(angle) => *angle,
                    LinearTo::Corner { right, bottom } => {
                        corner_angle(*right, *bottom, rect.width(), rect.height())
                    }
                };
                let (dx, dy) = (angle.to_radians().sin(), -angle.to_radians().cos());
                let extent = rect.width() * dx.abs() + rect.height() * dy.abs();
                let center = rect.center();
                let start = (center.x - dx * extent / 2.0, center.y - dy * extent / 2.0);
                let end = (center.x + dx * extent / 2.0, center.y + dy * extent / 2.0);
                let (colors, stops) = self.ramp(extent)?;
                skia_safe::gradient_shader::linear(
                    (start, end),
                    colors.as_slice(),
                    stops.as_slice(),
                    tile,
                    None,
                    None,
                )
            }
            Kind::Radial { center, radius } => {
                let (local_x, local_y) = (
                    style::resolve(center.0, rect.width()),
                    style::resolve(center.1, rect.height()),
                );
                let extent = |extent: &ShapeExtent, ellipse: bool| {
                    let closest = matches!(
                        extent,
                        ShapeExtent::ClosestSide | ShapeExtent::ClosestCorner
                    );
                    let dx = if closest {
                        local_x.min(rect.width() - local_x)
                    } else {
                        local_x.max(rect.width() - local_x)
                    };
                    let dy = if closest {
                        local_y.min(rect.height() - local_y)
                    } else {
                        local_y.max(rect.height() - local_y)
                    };
                    let corner = matches!(
                        extent,
                        ShapeExtent::ClosestCorner | ShapeExtent::FarthestCorner
                    );
                    if ellipse {
                        let factor = if corner {
                            std::f32::consts::SQRT_2
                        } else {
                            1.0
                        };
                        (dx * factor, dy * factor)
                    } else {
                        let r = if corner {
                            dx.hypot(dy)
                        } else if closest {
                            dx.min(dy)
                        } else {
                            dx.max(dy)
                        };
                        (r, r)
                    }
                };
                let (rx, ry) = match radius {
                    Radius::CircleExtent(e) => extent(e, false),
                    Radius::EllipseExtent(e) => extent(e, true),
                    Radius::Circle(r) => (*r, *r),
                    Radius::Ellipse(x, y) => (
                        style::resolve(*x, rect.width()),
                        style::resolve(*y, rect.height()),
                    ),
                };
                if rx <= 0.0 || ry <= 0.0 {
                    return None;
                }
                let matrix = Matrix::new_all(
                    rx,
                    0.0,
                    rect.left + local_x,
                    0.0,
                    ry,
                    rect.top + local_y,
                    0.0,
                    0.0,
                    1.0,
                );
                let (colors, stops) = self.ramp(rx)?;
                skia_safe::gradient_shader::radial(
                    (0.0, 0.0),
                    1.0,
                    colors.as_slice(),
                    stops.as_slice(),
                    tile,
                    None,
                    Some(&matrix),
                )
            }
            Kind::Conic { center, angle } => {
                let c = (
                    rect.left + style::resolve(center.0, rect.width()),
                    rect.top + style::resolve(center.1, rect.height()),
                );
                let mut matrix = Matrix::new_identity();
                matrix.set_rotate(*angle - 90.0, Some(c.into()));
                let (colors, stops) = self.ramp(1.0)?;
                skia_safe::gradient_shader::sweep(
                    c,
                    colors.as_slice(),
                    stops.as_slice(),
                    tile,
                    Some((0.0, 360.0)),
                    None,
                    Some(&matrix),
                )
            }
        }
    }
}
