//! Resolve lightningcss `<length-percentage>` values, including calc/min/max/clamp.
use std::cmp::Ordering;

use lightningcss::values::calc::{Calc, MathFunction};
use lightningcss::values::length::{LengthPercentage as CssLength, LengthValue};

use crate::style::Metrics;

const MIXED_BASIS: &str = "mixed calc() requires a length basis";

/// Resolved `<length-percentage>`.
///
/// [`ResolvedLength::Percent`] uses the same fraction as `style::convert_length`:
/// `LengthPercentage::percent(p.0)`, so `50%` is `0.5`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResolvedLength {
    /// Absolute pixels.
    Length(f32),
    /// Percentage fraction (`1.0` == `100%`). Values may fall outside `0.0..=1.0`.
    Percent(f32),
}

/// Resolve a length, percentage, or `calc()` / `min()` / `max()` / `clamp()`.
///
/// `percent_basis` is used only when a percentage is mixed with an absolute length.
/// That percentage fraction is multiplied by the basis and added to the pixel component.
/// Division by zero and other non-finite results are errors.
pub fn resolve_length(
    value: &CssLength,
    metrics: crate::style::Metrics,
    percent_basis: Option<f32>,
) -> Result<ResolvedLength, String> {
    resolve_value(value, metrics, percent_basis)?.finish(percent_basis)
}

#[derive(Clone, Copy, Debug)]
struct Parts {
    px: f32,
    percent: f32,
    saw_length: bool,
    saw_percent: bool,
}

impl Parts {
    fn length(px: f32) -> Self {
        Self {
            px,
            percent: 0.0,
            saw_length: true,
            saw_percent: false,
        }
    }

    fn percent(percent: f32) -> Self {
        Self {
            px: 0.0,
            percent,
            saw_length: false,
            saw_percent: true,
        }
    }

    fn add(self, other: Self) -> Result<Self, String> {
        Ok(Self {
            px: finite(self.px + other.px)?,
            percent: finite(self.percent + other.percent)?,
            saw_length: self.saw_length || other.saw_length,
            saw_percent: self.saw_percent || other.saw_percent,
        })
    }

    fn scale(self, factor: f32) -> Result<Self, String> {
        if !factor.is_finite() {
            return Err("division by zero".into());
        }
        Ok(Self {
            px: finite(self.px * factor)?,
            percent: finite(self.percent * factor)?,
            saw_length: self.saw_length,
            saw_percent: self.saw_percent,
        })
    }

    fn finish(self, basis: Option<f32>) -> Result<ResolvedLength, String> {
        match (self.saw_length, self.saw_percent) {
            (true, false) => Ok(ResolvedLength::Length(finite(self.px)?)),
            (false, true) => Ok(ResolvedLength::Percent(finite(self.percent)?)),
            (true, true) => {
                let basis = basis.ok_or_else(|| MIXED_BASIS.to_string())?;
                Ok(ResolvedLength::Length(finite(
                    self.px + self.percent * basis,
                )?))
            }
            (false, false) => Err("unitless number in length calculation".into()),
        }
    }
}

fn finite(value: f32) -> Result<f32, String> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err("non-finite length".into())
    }
}

fn resolve_value(
    value: &CssLength,
    metrics: Metrics,
    basis: Option<f32>,
) -> Result<Parts, String> {
    match value {
        CssLength::Dimension(v) => Ok(Parts::length(length_to_px(v, metrics)?)),
        CssLength::Percentage(percentage) => Ok(Parts::percent(percentage.0)),
        CssLength::Calc(calc) => resolve_calc(calc, metrics, basis),
    }
}

// Same unit conversion as `style::convert_length`, including its unsupported-unit error.
fn length_to_px(v: &LengthValue, m: Metrics) -> Result<f32, String> {
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
    Ok(px)
}

fn resolve_calc(
    calc: &Calc<CssLength>,
    metrics: Metrics,
    basis: Option<f32>,
) -> Result<Parts, String> {
    match calc {
        Calc::Value(value) => resolve_value(value, metrics, basis),
        Calc::Number(_) => Err("unitless number in length calculation".into()),
        Calc::Sum(left, right) => {
            let left = resolve_calc(left, metrics, basis)?;
            let right = resolve_calc(right, metrics, basis)?;
            left.add(right)
        }
        Calc::Product(factor, inner) => {
            if !factor.is_finite() {
                return Err("division by zero".into());
            }
            resolve_calc(inner, metrics, basis)?.scale(*factor)
        }
        Calc::Function(function) => resolve_function(function, metrics, basis),
    }
}

fn resolve_function(
    function: &MathFunction<CssLength>,
    metrics: Metrics,
    basis: Option<f32>,
) -> Result<Parts, String> {
    match function {
        MathFunction::Calc(calc) => resolve_calc(calc, metrics, basis),
        MathFunction::Min(args) => resolve_extreme(args, metrics, basis, Ordering::Less),
        MathFunction::Max(args) => resolve_extreme(args, metrics, basis, Ordering::Greater),
        MathFunction::Clamp(min, val, max) => resolve_clamp(min, val, max, metrics, basis),
        MathFunction::Round(_, _, _)
        | MathFunction::Rem(_, _)
        | MathFunction::Mod(_, _)
        | MathFunction::Abs(_)
        | MathFunction::Sign(_)
        | MathFunction::Hypot(_) => Err("unsupported math function in length calculation".into()),
    }
}

fn resolve_extreme(
    args: &[Calc<CssLength>],
    metrics: Metrics,
    basis: Option<f32>,
    ordering: Ordering,
) -> Result<Parts, String> {
    if args.is_empty() {
        return Err("min()/max() requires a length".into());
    }
    let parts = args
        .iter()
        .map(|arg| resolve_calc(arg, metrics, basis))
        .collect::<Result<Vec<_>, _>>()?;
    let numbers = unify(&parts, basis)?;
    let mut best = 0;
    for index in 1..numbers.len() {
        let choose = if ordering == Ordering::Less {
            numbers[index].0 < numbers[best].0
        } else {
            numbers[index].0 > numbers[best].0
        };
        if choose {
            best = index;
        }
    }
    Ok(numbers[best].1)
}

fn resolve_clamp(
    min: &Calc<CssLength>,
    val: &Calc<CssLength>,
    max: &Calc<CssLength>,
    metrics: Metrics,
    basis: Option<f32>,
) -> Result<Parts, String> {
    let parts = [
        resolve_calc(min, metrics, basis)?,
        resolve_calc(val, metrics, basis)?,
        resolve_calc(max, metrics, basis)?,
    ];
    let all_percent = parts.iter().all(|part| part.saw_percent && !part.saw_length);
    let numbers = unify(&parts, basis)?;
    let lower = numbers[0].0;
    let value = numbers[1].0;
    let upper = numbers[2].0;
    // CSS clamp(MIN, VAL, MAX) is max(MIN, min(VAL, MAX)); MIN wins if MIN > MAX.
    let used = finite(lower.max(value.min(upper)))?;
    if all_percent {
        Ok(Parts::percent(used))
    } else {
        Ok(Parts::length(used))
    }
}

fn unify(parts: &[Parts], basis: Option<f32>) -> Result<Vec<(f32, Parts)>, String> {
    let mixed = parts.iter().any(|part| part.saw_length)
        && parts.iter().any(|part| part.saw_percent);
    parts
        .iter()
        .map(|part| project(*part, mixed, basis))
        .collect()
}

fn project(part: Parts, mixed: bool, basis: Option<f32>) -> Result<(f32, Parts), String> {
    if mixed {
        let basis = basis.ok_or_else(|| MIXED_BASIS.to_string())?;
        let px = finite(part.px + part.percent * basis)?;
        return Ok((px, Parts::length(px)));
    }
    if part.saw_percent {
        return Ok((finite(part.percent)?, part));
    }
    if part.saw_length {
        return Ok((finite(part.px)?, part));
    }
    Err("unitless number in length calculation".into())
}

#[cfg(test)]
mod tests {
    use lightningcss::traits::Parse;
    use lightningcss::values::length::LengthPercentage as CssLength;

    use super::{resolve_length, ResolvedLength};
    use crate::style::Metrics;

    const EPS: f32 = 1.0e-4;

    fn metrics_at(font_size: f32) -> Metrics {
        Metrics {
            width: 200.0,
            height: 100.0,
            font_size,
        }
    }

    fn resolve_at(
        css: &str,
        metrics: Metrics,
        basis: Option<f32>,
    ) -> Result<ResolvedLength, String> {
        let value = CssLength::parse_string(css).map_err(|err| format!("{err:?}"))?;
        resolve_length(&value, metrics, basis)
    }

    fn expect_px(css: &str, metrics: Metrics, basis: Option<f32>, px: f32) {
        match resolve_at(css, metrics, basis) {
            Ok(ResolvedLength::Length(value)) => {
                assert!(
                    (value - px).abs() < EPS,
                    "{css} => {value}, expected {px}px"
                );
            }
            Ok(ResolvedLength::Percent(value)) => {
                panic!("{css} => percent {value}, expected {px}px");
            }
            Err(err) => panic!("{css} failed: {err}"),
        }
    }

    fn expect_percent(css: &str, metrics: Metrics, basis: Option<f32>, fraction: f32) {
        match resolve_at(css, metrics, basis) {
            Ok(ResolvedLength::Percent(value)) => {
                assert!(
                    (value - fraction).abs() < EPS,
                    "{css} => {value}, expected fraction {fraction}"
                );
            }
            Ok(ResolvedLength::Length(value)) => {
                panic!("{css} => {value}px, expected fraction {fraction}");
            }
            Err(err) => panic!("{css} failed: {err}"),
        }
    }

    #[test]
    fn pure_px_calc() {
        let metrics = metrics_at(16.0);
        expect_px("10px", metrics, None, 10.0);
        expect_px("calc(10px + 20px)", metrics, None, 30.0);
        expect_px("calc(100px - 25px * 2)", metrics, None, 50.0);
        expect_px("calc(40px / 4)", metrics, None, 10.0);
        expect_px("calc((10px + 20px) * 2)", metrics, None, 60.0);
        expect_px("12pt", metrics, None, 16.0);
        expect_px("1in", metrics, None, 96.0);
    }

    #[test]
    fn pure_percent_calc() {
        let metrics = metrics_at(16.0);
        expect_percent("50%", metrics, None, 0.5);
        expect_percent("100%", metrics, None, 1.0);
        expect_percent("-25%", metrics, None, -0.25);
        expect_percent("calc(25% + 50%)", metrics, None, 0.75);
        expect_percent("calc(50% * 2)", metrics, None, 1.0);
        expect_percent("calc(100% / 4)", metrics, None, 0.25);
    }

    #[test]
    fn em_plus_px() {
        let metrics = metrics_at(20.0);
        expect_px("calc(1em + 10px)", metrics, None, 30.0);
        expect_px("calc(2 * (1em + 10px))", metrics, None, 60.0);
        expect_px("calc(1rem + 2px)", metrics, None, 18.0);
        expect_px("calc(1rem + 1em)", metrics, None, 36.0);
    }

    #[test]
    fn mixed_without_basis() {
        for css in [
            "calc(10px + 25%)",
            "calc(25% + 1em)",
            "min(10px, 25%)",
            "max(1em, 10%)",
            "clamp(10%, 1em, 20px)",
        ] {
            let err = resolve_at(css, metrics_at(16.0), None).expect_err(css);
            assert!(
                err.contains("mixed calc() requires a length basis"),
                "{css}: {err}"
            );
        }
    }

    #[test]
    fn mixed_with_basis() {
        let metrics = metrics_at(16.0);
        expect_px("calc(10px + 25%)", metrics, Some(200.0), 60.0);
        expect_px("calc(100% - 10px)", metrics, Some(80.0), 70.0);
        expect_px("calc(10px - 50%)", metrics, Some(100.0), -40.0);
        expect_px("calc(50% + 1em)", metrics, Some(200.0), 116.0);
        expect_px("calc(2 * (10px + 25%))", metrics, Some(200.0), 120.0);
    }

    #[test]
    fn min_compares_resolved_values() {
        let metrics = metrics_at(16.0);
        expect_px("min(1em, 10px)", metrics, None, 10.0);
        expect_px("min(1em, 10px)", metrics_at(8.0), None, 8.0);
        expect_px("min(-1em, 4px)", metrics, None, -16.0);
        expect_px("min(2em, 1em, 4px)", metrics, None, 4.0);
        expect_px("min(10px, 40px)", metrics, None, 10.0);
        expect_px("calc(min(1em, 30px) / 2)", metrics, None, 8.0);
        expect_percent("min(25%, 75%)", metrics, None, 0.25);
        expect_px("min(25%, 80px)", metrics, Some(200.0), 50.0);
        expect_px("min(25%, 80px)", metrics, Some(400.0), 80.0);
    }

    #[test]
    fn max_compares_resolved_values() {
        let metrics = metrics_at(16.0);
        expect_px("max(1em, 10px)", metrics, None, 16.0);
        expect_px("max(1em, 10px)", metrics_at(8.0), None, 10.0);
        expect_px("max(2em, 1em, 4px)", metrics, None, 32.0);
        expect_px("max(10px, 40px)", metrics, None, 40.0);
        expect_percent("max(25%, 75%)", metrics, None, 0.75);
        expect_px("max(25%, 80px)", metrics, Some(200.0), 80.0);
        expect_px("max(25%, 80px)", metrics, Some(400.0), 100.0);
    }

    #[test]
    fn clamp_follows_css() {
        let metrics = metrics_at(16.0);
        expect_px("clamp(10px, 1em, 40px)", metrics, None, 16.0);
        expect_px("clamp(10px, 1em, 40px)", metrics_at(8.0), None, 10.0);
        expect_px("clamp(10px, 1em, 40px)", metrics_at(50.0), None, 40.0);
        expect_px("clamp(40px, 1em, 10px)", metrics, None, 40.0);
        expect_px("clamp(10px, 25px, 40px)", metrics, None, 25.0);
        expect_percent("clamp(25%, 50%, 75%)", metrics, None, 0.5);
        expect_percent("clamp(75%, 50%, 25%)", metrics, None, 0.75);
        expect_px("clamp(10%, 1em, 40%)", metrics, Some(100.0), 16.0);
        expect_px("clamp(10%, 1em, 20%)", metrics_at(40.0), Some(100.0), 20.0);
    }

    #[test]
    fn division_by_zero_is_an_error() {
        for css in [
            "calc(10px / 0)",
            "calc(10px / (0))",
            "calc(100% / 0)",
            "calc(1em / (2 - 2))",
        ] {
            let err = resolve_at(css, metrics_at(16.0), None).expect_err(css);
            assert!(!err.is_empty(), "{css}");
        }
    }

    #[test]
    fn nested_calc() {
        let metrics = metrics_at(16.0);
        expect_px("calc(1em + calc(10px + 5px))", metrics, None, 31.0);
        expect_px("calc(min(1em, calc(20px + 5px)))", metrics, None, 16.0);
        expect_px(
            "calc(2 * (1em + calc(10% + 5px)))",
            metrics,
            Some(100.0),
            62.0,
        );
    }

    #[test]
    fn viewport_units() {
        let metrics = metrics_at(16.0);
        expect_px("10vw", metrics, None, 20.0);
        expect_px("10vh", metrics, None, 10.0);
        expect_px("10vmin", metrics, None, 10.0);
        expect_px("10vmax", metrics, None, 20.0);
        expect_px("calc(10vw + 10vh)", metrics, None, 30.0);
        expect_px("calc(1vmin + 1vmax)", metrics, None, 3.0);
    }

    #[test]
    fn unsupported_unit_is_an_error() {
        for css in ["1ex", "calc(1ch + 1px)"] {
            let err = resolve_at(css, metrics_at(16.0), None).expect_err(css);
            assert!(err.contains("unsupported length unit"), "{css}: {err}");
        }
    }
}
