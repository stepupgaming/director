//! Parse a CSS transform-origin, including an optional absolute z length.

use crate::css::split;
use crate::style::{length, LengthParts, Metrics};
use taffy::prelude::LengthPercentage;

pub struct ParsedOrigin {
    pub x: LengthPercentage,
    pub y: LengthPercentage,
    pub z: f32,
}

pub fn parse_origin(source: &str, metrics: Metrics) -> Result<ParsedOrigin, String> {
    let tokens = split(source, false);
    if tokens.is_empty() || tokens.len() >= 4 {
        return Err("3D transform origin unsupported".into());
    }
    let axis = |token: &str| -> Result<LengthPercentage, String> {
        match token {
            "center" => Ok(LengthPercentage::percent(0.5_f32)),
            "left" | "top" => Ok(LengthPercentage::percent(0.0_f32)),
            "right" | "bottom" => Ok(LengthPercentage::percent(1.0_f32)),
            _ => length(token, metrics),
        }
    };
    let x = axis(&tokens[0])?;
    let y = if tokens.len() >= 2 {
        axis(&tokens[1])?
    } else {
        LengthPercentage::percent(0.5_f32)
    };
    let z = if tokens.len() == 3 {
        let z_length = length(&tokens[2], metrics)?;
        if z_length.is_percent() {
            return Err("3D transform origin unsupported".into());
        }
        z_length.value()
    } else {
        0.0
    };
    Ok(ParsedOrigin { x, y, z })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> Metrics {
        Metrics {
            width: 200.0,
            height: 100.0,
            font_size: 16.0,
        }
    }

    fn assert_axis(actual: LengthPercentage, expected: LengthPercentage) {
        assert_eq!(actual.is_percent(), expected.is_percent());
        assert_eq!(actual.value(), expected.value());
    }

    fn assert_origin(source: &str, x: LengthPercentage, y: LengthPercentage, z: f32) {
        let parsed = parse_origin(source, metrics()).unwrap_or_else(|err| panic!("{source}: {err}"));
        assert_axis(parsed.x, x);
        assert_axis(parsed.y, y);
        assert_eq!(parsed.z, z);
    }

    fn assert_unsupported(source: &str) {
        match parse_origin(source, metrics()) {
            Err(message) => assert_eq!(message, "3D transform origin unsupported"),
            Ok(_) => panic!("{source} parsed"),
        }
    }

    #[test]
    fn center() {
        assert_origin(
            "center",
            LengthPercentage::percent(0.5),
            LengthPercentage::percent(0.5),
            0.0,
        );
    }

    #[test]
    fn left() {
        assert_origin(
            "left",
            LengthPercentage::percent(0.0),
            LengthPercentage::percent(0.5),
            0.0,
        );
    }

    #[test]
    fn right_bottom() {
        assert_origin(
            "right bottom",
            LengthPercentage::percent(1.0),
            LengthPercentage::percent(1.0),
            0.0,
        );
    }

    #[test]
    fn px_and_percent() {
        assert_origin(
            "10px 20%",
            LengthPercentage::length(10.0),
            LengthPercentage::percent(0.2),
            0.0,
        );
    }

    #[test]
    fn center_top() {
        assert_origin(
            "center top",
            LengthPercentage::percent(0.5),
            LengthPercentage::percent(0.0),
            0.0,
        );
    }

    #[test]
    fn percent_pair_with_px_z() {
        assert_origin(
            "50% 50% 12px",
            LengthPercentage::percent(0.5),
            LengthPercentage::percent(0.5),
            12.0,
        );
    }

    #[test]
    fn center_top_em_z() {
        assert_origin(
            "center top 1em",
            LengthPercentage::percent(0.5),
            LengthPercentage::percent(0.0),
            16.0,
        );
    }

    #[test]
    fn percent_z_is_unsupported() {
        assert_unsupported("50% 50% 10%");
    }

    #[test]
    fn four_lengths_are_unsupported() {
        assert_unsupported("1px 2px 3px 4px");
    }

    #[test]
    fn empty_is_unsupported() {
        assert_unsupported("");
    }

    #[test]
    fn zero_px() {
        assert_origin(
            "0px",
            LengthPercentage::length(0.0),
            LengthPercentage::percent(0.5),
            0.0,
        );
    }
}
