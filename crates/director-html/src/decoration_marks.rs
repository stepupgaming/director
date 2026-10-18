//! CSS text-decoration geometry for one line span.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DecorationStyle {
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    Fill { x: f32, y: f32, w: f32, h: f32 },
    Stroke { points: Vec<(f32, f32)>, width: f32 },
}

pub fn marks(style: DecorationStyle, x: f32, y: f32, width: f32, thickness: f32) -> Vec<Mark> {
    if !(x.is_finite() && y.is_finite() && width.is_finite() && thickness.is_finite())
        || width <= 0.0
        || thickness <= 0.0
    {
        return Vec::new();
    }
    match style {
        DecorationStyle::Solid => vec![Mark::Fill {
            x,
            y,
            w: width,
            h: thickness,
        }],
        DecorationStyle::Double => {
            let stripe = thickness / 3.0;
            vec![
                Mark::Fill {
                    x,
                    y,
                    w: width,
                    h: stripe,
                },
                Mark::Fill {
                    x,
                    y: y + 2.0 * stripe,
                    w: width,
                    h: stripe,
                },
            ]
        }
        DecorationStyle::Dashed => dashed(x, y, width, thickness),
        DecorationStyle::Dotted => dotted(x, y, width, thickness),
        DecorationStyle::Wavy => vec![wavy(x, y, width, thickness)],
    }
}

fn dashed(x: f32, y: f32, width: f32, thickness: f32) -> Vec<Mark> {
    let on = 3.0 * thickness;
    let period = on + 3.0 * thickness;
    let end = x + width;
    let mut cursor = x;
    let mut out = Vec::new();
    while cursor < end {
        let remaining = end - cursor;
        if remaining <= 0.0 {
            break;
        }
        let w = on.min(remaining);
        if w <= 0.0 {
            break;
        }
        out.push(Mark::Fill {
            x: cursor,
            y,
            w,
            h: thickness,
        });
        let next = cursor + period;
        if next <= cursor {
            break;
        }
        cursor = next;
    }
    out
}

fn dotted(x: f32, y: f32, width: f32, thickness: f32) -> Vec<Mark> {
    let end = x + width;
    let mut center = x + thickness / 2.0;
    let mut out = Vec::new();
    while center <= end {
        out.push(Mark::Fill {
            x: center - thickness / 2.0,
            y,
            w: thickness,
            h: thickness,
        });
        let next = center + thickness;
        if next <= center {
            break;
        }
        center = next;
    }
    out
}

fn wavy(x: f32, y: f32, width: f32, thickness: f32) -> Mark {
    let step = thickness / 2.0;
    let end = x + width;
    let mut points = Vec::new();
    let mut sx = x;
    loop {
        let at = if sx < end { sx } else { end };
        if points.last().is_none_or(|(px, _)| *px != at) {
            points.push((at, wave_y(at, x, y, thickness)));
        }
        if at == end {
            break;
        }
        let next = sx + step;
        sx = if next > sx { next } else { end };
    }
    Mark::Stroke {
        points,
        width: thickness,
    }
}

fn wave_y(sx: f32, x: f32, y: f32, thickness: f32) -> f32 {
    let wavelength = 4.0 * thickness;
    y + thickness / 2.0
        + (thickness / 2.0) * (2.0 * std::f32::consts::PI * (sx - x) / wavelength).sin()
}

#[cfg(test)]
mod tests {
    use super::{marks, DecorationStyle, Mark};

    #[test]
    fn solid() {
        assert_eq!(
            marks(DecorationStyle::Solid, 0.0, 10.0, 20.0, 2.0),
            vec![Mark::Fill {
                x: 0.0,
                y: 10.0,
                w: 20.0,
                h: 2.0,
            }]
        );
    }

    #[test]
    fn double() {
        let (x, y, width) = (0.0, 10.0, 20.0);
        assert_eq!(
            marks(DecorationStyle::Double, x, y, width, 3.0),
            vec![
                Mark::Fill {
                    x,
                    y,
                    w: width,
                    h: 1.0,
                },
                Mark::Fill {
                    x,
                    y: y + 2.0,
                    w: width,
                    h: 1.0,
                },
            ]
        );
    }

    #[test]
    fn dashed() {
        assert_eq!(
            marks(DecorationStyle::Dashed, 0.0, 0.0, 12.0, 1.0),
            vec![
                Mark::Fill {
                    x: 0.0,
                    y: 0.0,
                    w: 3.0,
                    h: 1.0,
                },
                Mark::Fill {
                    x: 6.0,
                    y: 0.0,
                    w: 3.0,
                    h: 1.0,
                },
            ]
        );
        assert_eq!(
            marks(DecorationStyle::Dashed, 0.0, 0.0, 14.0, 1.0),
            vec![
                Mark::Fill {
                    x: 0.0,
                    y: 0.0,
                    w: 3.0,
                    h: 1.0,
                },
                Mark::Fill {
                    x: 6.0,
                    y: 0.0,
                    w: 3.0,
                    h: 1.0,
                },
                Mark::Fill {
                    x: 12.0,
                    y: 0.0,
                    w: 2.0,
                    h: 1.0,
                },
            ]
        );
    }

    #[test]
    fn dotted() {
        assert_eq!(
            marks(DecorationStyle::Dotted, 0.0, 0.0, 4.0, 2.0),
            vec![
                Mark::Fill {
                    x: 0.0,
                    y: 0.0,
                    w: 2.0,
                    h: 2.0,
                },
                Mark::Fill {
                    x: 2.0,
                    y: 0.0,
                    w: 2.0,
                    h: 2.0,
                },
            ]
        );
    }

    #[test]
    fn zero_width() {
        assert_eq!(
            marks(DecorationStyle::Solid, 0.0, 10.0, 0.0, 2.0),
            Vec::<Mark>::new()
        );
    }

    #[test]
    fn non_finite_thickness() {
        assert_eq!(
            marks(DecorationStyle::Solid, 0.0, 10.0, 20.0, f32::NAN),
            Vec::<Mark>::new()
        );
    }

    #[test]
    fn wavy() {
        let y = 0.0;
        let thickness = 2.0;
        let got = marks(DecorationStyle::Wavy, 0.0, y, 4.0, thickness);
        let Mark::Stroke { points, width } = &got[0] else {
            panic!("expected stroke");
        };
        assert_eq!(got.len(), 1);
        assert_eq!(*width, thickness);
        assert_eq!(points.len(), 5);
        assert_eq!(points[0].1, y + thickness / 2.0);
        assert_eq!(
            points.iter().map(|point| point.0).collect::<Vec<_>>(),
            vec![0.0, 1.0, 2.0, 3.0, 4.0]
        );
    }
}
