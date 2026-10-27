//! GSAP 3 eases evaluated with GSAP's own formulas, so a translated tween
//! keeps its exact curve and its readable name.

use std::f64::consts::{FRAC_PI_2, PI};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Family {
    Linear,
    Power(i32),
    Sine,
    Expo,
    Circ,
    Back(f64),
    Elastic(f64, Option<f64>),
    Bounce,
    Steps(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    In,
    Out,
    InOut,
}

/// A parsed GSAP ease such as `power3.out`, `back.out(1.7)` or `steps(5)`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GsapEase {
    family: Family,
    kind: Kind,
}

fn number(s: &str) -> Result<f64, String> {
    s.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("invalid GSAP ease argument '{s}'"))
}

impl GsapEase {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let (head, args) = match text.find('(') {
            Some(i) if text.ends_with(')') => (&text[..i], Some(&text[i + 1..text.len() - 1])),
            Some(_) => return Err(format!("unsupported GSAP ease '{text}'")),
            None => (text, None),
        };
        let args: Vec<f64> = match args {
            Some(a) if !a.trim().is_empty() => a.split(',').map(number).collect::<Result<_, _>>()?,
            _ => Vec::new(),
        };
        let (name, kind) = match head.split_once('.') {
            Some((name, kind)) => (name, Some(kind)),
            None => (head, None),
        };
        let kind = match kind.map(str::to_ascii_lowercase).as_deref() {
            None | Some("out") | Some("easeout") => Kind::Out,
            Some("in") | Some("easein") => Kind::In,
            Some("inout") | Some("easeinout") => Kind::InOut,
            Some(other) => return Err(format!("unsupported GSAP ease type '{other}' in '{text}'")),
        };
        let name = name.to_ascii_lowercase();
        let no_args = |family| {
            if args.is_empty() {
                Ok(family)
            } else {
                Err(format!("GSAP ease '{text}' takes no arguments"))
            }
        };
        let family = match name.as_str() {
            "none" | "linear" | "power0" => no_args(Family::Linear)?,
            "power1" | "quad" => no_args(Family::Power(2))?,
            "power2" | "cubic" => no_args(Family::Power(3))?,
            "power3" | "quart" => no_args(Family::Power(4))?,
            "power4" | "quint" | "strong" => no_args(Family::Power(5))?,
            "sine" => no_args(Family::Sine)?,
            "expo" => no_args(Family::Expo)?,
            "circ" => no_args(Family::Circ)?,
            "bounce" => no_args(Family::Bounce)?,
            "back" => match args.as_slice() {
                [] => Family::Back(1.70158),
                [overshoot] => Family::Back(*overshoot),
                _ => return Err(format!("GSAP ease '{text}' takes one overshoot")),
            },
            "elastic" => match args.as_slice() {
                [] => Family::Elastic(1.0, None),
                [amplitude] => Family::Elastic(*amplitude, None),
                [amplitude, period] => Family::Elastic(*amplitude, Some(*period)),
                _ => return Err(format!("GSAP ease '{text}' takes amplitude and period")),
            },
            "steps" | "steppedease" => match args.as_slice() {
                [n] if *n >= 1.0 && n.fract() == 0.0 => Family::Steps(*n as u32),
                _ => return Err(format!("GSAP ease '{text}' needs a positive step count")),
            },
            _ => return Err(format!("unsupported GSAP ease '{text}'")),
        };
        Ok(Self { family, kind })
    }

    /// The canonical GSAP spelling, such as `power2.inOut` or `elastic.out(1, 0.3)`.
    pub(crate) fn text(&self) -> String {
        let kind = match self.kind {
            Kind::In => "in",
            Kind::Out => "out",
            Kind::InOut => "inOut",
        };
        match self.family {
            Family::Linear => "none".into(),
            Family::Power(n) => format!("power{}.{kind}", n - 1),
            Family::Sine => format!("sine.{kind}"),
            Family::Expo => format!("expo.{kind}"),
            Family::Circ => format!("circ.{kind}"),
            Family::Bounce => format!("bounce.{kind}"),
            Family::Back(o) => format!("back.{kind}({o})"),
            Family::Elastic(a, None) => format!("elastic.{kind}({a})"),
            Family::Elastic(a, Some(p)) => format!("elastic.{kind}({a}, {p})"),
            Family::Steps(n) => format!("steps({n})"),
        }
    }

    fn ease_in(&self, p: f64) -> f64 {
        match self.family {
            Family::Linear => p,
            Family::Power(n) => p.powi(n),
            Family::Sine => {
                if p == 1.0 {
                    1.0
                } else {
                    1.0 - (p * FRAC_PI_2).cos()
                }
            }
            Family::Expo => {
                if p == 0.0 {
                    0.0
                } else {
                    2f64.powf(10.0 * (p - 1.0))
                }
            }
            Family::Circ => -((1.0 - p * p).max(0.0).sqrt() - 1.0),
            Family::Back(o) => {
                if p == 0.0 {
                    0.0
                } else {
                    p * p * ((o + 1.0) * p - o)
                }
            }
            Family::Bounce => 1.0 - bounce_out(1.0 - p),
            Family::Elastic(..) | Family::Steps(_) => unreachable!("evaluated directly"),
        }
    }

    pub(crate) fn eval(&self, p: f64) -> f64 {
        let p = p.clamp(0.0, 1.0);
        match self.family {
            Family::Steps(n) => {
                // GSAP SteppedEase: ((n + 1) * clamp(p, 0, 1 - tiny) | 0) / n.
                let max = 1.0 - 1e-8;
                (((n as f64 + 1.0) * p.min(max)).floor() / n as f64).min(1.0)
            }
            Family::Elastic(amplitude, period) => {
                let out = elastic_out(amplitude, period, self.kind);
                match self.kind {
                    Kind::Out => out(p),
                    Kind::In => 1.0 - out(1.0 - p),
                    Kind::InOut => {
                        if p < 0.5 {
                            (1.0 - out(1.0 - p * 2.0)) / 2.0
                        } else {
                            0.5 + out((p - 0.5) * 2.0) / 2.0
                        }
                    }
                }
            }
            _ => match self.kind {
                Kind::In => self.ease_in(p),
                Kind::Out => 1.0 - self.ease_in(1.0 - p),
                Kind::InOut => {
                    if p < 0.5 {
                        self.ease_in(p * 2.0) / 2.0
                    } else {
                        1.0 - self.ease_in((1.0 - p) * 2.0) / 2.0
                    }
                }
            },
        }
    }
}

fn bounce_out(p: f64) -> f64 {
    let (n1, d1) = (7.5625, 2.75);
    if p < 1.0 / d1 {
        n1 * p * p
    } else if p < 2.0 / d1 {
        n1 * (p - 1.5 / d1).powi(2) + 0.75
    } else if p < 2.5 / d1 {
        n1 * (p - 2.25 / d1).powi(2) + 0.9375
    } else {
        n1 * (p - 2.625 / d1).powi(2) + 0.984375
    }
}

/// GSAP `_configElastic`: the out curve that in and inOut derive from.
fn elastic_out(amplitude: f64, period: Option<f64>, kind: Kind) -> impl Fn(f64) -> f64 {
    let p1 = if amplitude >= 1.0 { amplitude } else { 1.0 };
    let default = if kind == Kind::InOut { 0.45 } else { 0.3 };
    let p2 = period.unwrap_or(default) / if amplitude < 1.0 { amplitude } else { 1.0 };
    let p3 = p2 / (2.0 * PI) * (1.0 / p1).asin();
    let p2 = 2.0 * PI / p2;
    move |p: f64| {
        if p == 1.0 {
            1.0
        } else {
            p1 * 2f64.powf(-10.0 * p) * ((p - p3) * p2).sin() + 1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GsapEase;

    fn ease(s: &str) -> GsapEase {
        GsapEase::parse(s).unwrap()
    }

    #[test]
    fn names_and_aliases_parse_to_canonical_text() {
        assert_eq!(ease("power2").text(), "power2.out");
        assert_eq!(ease("Power2.easeInOut").text(), "power2.inOut");
        assert_eq!(ease("quad.in").text(), "power1.in");
        assert_eq!(ease("back.out(1.4)").text(), "back.out(1.4)");
        assert_eq!(ease("linear").text(), "none");
        assert!(GsapEase::parse("rough").is_err());
        assert!(GsapEase::parse("power2.sideways").is_err());
    }

    #[test]
    fn curves_match_gsap_formulas() {
        let close = |a: f64, b: f64| assert!((a - b).abs() < 1e-9, "{a} vs {b}");
        close(ease("power2.out").eval(0.5), 1.0 - 0.5f64.powi(3));
        close(ease("power1.inOut").eval(0.25), 0.125);
        close(ease("sine.in").eval(1.0), 1.0);
        close(ease("expo.in").eval(0.0), 0.0);
        close(ease("back.out(1.7)").eval(1.0), 1.0);
        close(ease("bounce.out").eval(1.0), 1.0);
        close(ease("elastic.out").eval(1.0), 1.0);
        close(ease("elastic.out").eval(0.0), 0.0);
        close(ease("steps(4)").eval(0.5), 0.5);
        close(ease("steps(4)").eval(1.0), 1.0);
        assert!(ease("back.out(1.7)").eval(0.6) > 1.0);
    }
}
