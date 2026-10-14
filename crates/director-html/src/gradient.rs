use crate::{
    css::split,
    style::{self, LengthParts, Metrics},
};
use director_core::types::Color;
use lightningcss::{
    traits::{Parse, ToCss},
    values::{
        gradient::{
            Circle, Ellipse, EndingShape, Gradient, GradientItem, LineDirection, ShapeExtent,
        },
        position::Position,
    },
};
use skia_safe::{Matrix, Rect, Shader, TileMode};
use taffy::LengthPercentage;

#[derive(Debug, Clone)]
pub struct NativeGradient {
    kind: Kind,
    colors: Vec<skia_safe::Color>,
    stops: Vec<Option<LengthPercentage>>,
}
#[derive(Debug, Clone)]
enum Kind {
    Linear(f32),
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
fn stops<D: ToCss>(
    items: Vec<GradientItem<D>>,
    m: Metrics,
    current: Color,
    conic: bool,
) -> Result<(Vec<skia_safe::Color>, Vec<Option<LengthPercentage>>), String> {
    let mut colors = Vec::new();
    let mut positions = Vec::new();
    for item in items {
        let GradientItem::ColorStop(stop) = item else {
            return Err("gradient color hints unsupported".into());
        };
        colors.push(style::css_color(&stop.color, current)?.to_skia());
        positions.push(
            stop.position
                .map(|p| {
                    let s = p
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
                        Ok(LengthPercentage::percent(value))
                    } else {
                        style::length(&s, m)
                    }
                })
                .transpose()?,
        );
    }
    if colors.len() < 2 {
        return Err("gradient needs at least two color stops".into());
    }
    Ok((colors, positions))
}

pub fn parse_list(s: &str, m: Metrics, current: Color) -> Result<Vec<NativeGradient>, String> {
    split(s, true)
        .into_iter()
        .map(|s| NativeGradient::parse(&s, m, current))
        .collect()
}
impl NativeGradient {
    pub fn parse(s: &str, m: Metrics, current: Color) -> Result<Self, String> {
        let gradient = Gradient::parse_string(s).map_err(|e| format!("gradient: {e:?}"))?;
        let (kind, colors, positions) = match gradient {
            Gradient::Linear(g) => {
                let angle = match g.direction {
                    LineDirection::Angle(a) => a.to_degrees(),
                    LineDirection::Horizontal(h) => {
                        if h.to_css_string(Default::default())
                            .map_err(|e| e.to_string())?
                            == "left"
                        {
                            270.0
                        } else {
                            90.0
                        }
                    }
                    LineDirection::Vertical(v) => {
                        if v.to_css_string(Default::default())
                            .map_err(|e| e.to_string())?
                            == "top"
                        {
                            0.0
                        } else {
                            180.0
                        }
                    }
                    _ => return Err("corner gradients require explicit angle".into()),
                };
                let (colors, positions) = stops(g.items, m, current, false)?;
                (Kind::Linear(angle), colors, positions)
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
            _ => return Err("repeating/vendor gradients unsupported".into()),
        };
        Ok(Self {
            kind,
            colors,
            stops: positions,
        })
    }
    fn positions(&self, basis: f32) -> Vec<f32> {
        let mut pos: Vec<Option<f32>> = self
            .stops
            .iter()
            .map(|p| {
                p.map(|p| {
                    if p.is_percent() {
                        p.value()
                    } else {
                        p.value() / basis.max(0.0001)
                    }
                })
            })
            .collect();
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
        pos.into_iter().flatten().collect()
    }
    pub fn shader(&self, rect: Rect) -> Option<Shader> {
        match &self.kind {
            Kind::Linear(angle) => {
                let (dx, dy) = (angle.to_radians().sin(), -angle.to_radians().cos());
                let extent = rect.width() * dx.abs() + rect.height() * dy.abs();
                let center = rect.center();
                let start = (center.x - dx * extent / 2.0, center.y - dy * extent / 2.0);
                let end = (center.x + dx * extent / 2.0, center.y + dy * extent / 2.0);
                skia_safe::gradient_shader::linear(
                    (start, end),
                    self.colors.as_slice(),
                    self.positions(extent).as_slice(),
                    TileMode::Clamp,
                    None,
                    None,
                )
            }
            Kind::Radial { center, radius } => {
                let (cx, cy) = (
                    style::resolve(center.0, rect.width()),
                    style::resolve(center.1, rect.height()),
                );
                let extent = |extent: &ShapeExtent, ellipse: bool| {
                    let closest = matches!(
                        extent,
                        ShapeExtent::ClosestSide | ShapeExtent::ClosestCorner
                    );
                    let dx = if closest {
                        cx.min(rect.width() - cx)
                    } else {
                        cx.max(rect.width() - cx)
                    };
                    let dy = if closest {
                        cy.min(rect.height() - cy)
                    } else {
                        cy.max(rect.height() - cy)
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
                let matrix = Matrix::new_all(rx, 0.0, cx, 0.0, ry, cy, 0.0, 0.0, 1.0);
                skia_safe::gradient_shader::radial(
                    (0.0, 0.0),
                    1.0,
                    self.colors.as_slice(),
                    self.positions(rx).as_slice(),
                    TileMode::Clamp,
                    None,
                    Some(&matrix),
                )
            }
            Kind::Conic { center, angle } => {
                let c = (
                    style::resolve(center.0, rect.width()),
                    style::resolve(center.1, rect.height()),
                );
                let mut matrix = Matrix::new_identity();
                matrix.set_rotate(*angle - 90.0, Some(c.into()));
                skia_safe::gradient_shader::sweep(
                    c,
                    self.colors.as_slice(),
                    self.positions(1.0).as_slice(),
                    TileMode::Clamp,
                    Some((0.0, 360.0)),
                    None,
                    Some(&matrix),
                )
            }
        }
    }
}
