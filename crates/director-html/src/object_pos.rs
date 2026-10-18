//! CSS `object-position` as leftover-space fractions plus pixel offsets.
//! A right or bottom edge offset is negative.

use lightningcss::{
    traits::{Parse, ToCss},
    values::{
        length::LengthPercentage as CssLength,
        position::{
            HorizontalPosition, HorizontalPositionKeyword, Position, PositionComponent,
            VerticalPosition, VerticalPositionKeyword,
        },
    },
};

use crate::style::{length, LengthParts, Metrics};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParsedObjectPosition {
    pub x_percent: f32,
    pub x_offset: f32,
    pub y_percent: f32,
    pub y_offset: f32,
}

pub fn parse_object_position(
    source: &str,
    metrics: Metrics,
) -> Result<ParsedObjectPosition, String> {
    if source.trim().is_empty() {
        return Err(unsupported(source));
    }
    let parsed = Position::parse_string(source).map_err(|_| unsupported(source))?;
    let x = horizontal_position(&parsed.x, metrics)?;
    let y = vertical_position(&parsed.y, metrics)?;
    Ok(ParsedObjectPosition {
        x_percent: x.percent,
        x_offset: x.offset,
        y_percent: y.percent,
        y_offset: y.offset,
    })
}

struct Axis {
    percent: f32,
    offset: f32,
}

fn unsupported(source: &str) -> String {
    format!("unsupported object-position '{source}'")
}

fn horizontal_position(value: &HorizontalPosition, metrics: Metrics) -> Result<Axis, String> {
    match value {
        PositionComponent::Center => Ok(Axis {
            percent: 0.5,
            offset: 0.0,
        }),
        PositionComponent::Length(value) => axis_from_length(value, metrics),
        PositionComponent::Side { side, offset } => side_offset(
            matches!(side, HorizontalPositionKeyword::Right),
            offset.as_ref(),
            metrics,
        ),
    }
}

fn vertical_position(value: &VerticalPosition, metrics: Metrics) -> Result<Axis, String> {
    match value {
        PositionComponent::Center => Ok(Axis {
            percent: 0.5,
            offset: 0.0,
        }),
        PositionComponent::Length(value) => axis_from_length(value, metrics),
        PositionComponent::Side { side, offset } => side_offset(
            matches!(side, VerticalPositionKeyword::Bottom),
            offset.as_ref(),
            metrics,
        ),
    }
}

fn side_offset(from_end: bool, offset: Option<&CssLength>, metrics: Metrics) -> Result<Axis, String> {
    let extra = match offset {
        Some(value) => axis_from_length(value, metrics)?,
        None => Axis {
            percent: 0.0,
            offset: 0.0,
        },
    };
    let sign = if from_end { -1.0 } else { 1.0 };
    Ok(Axis {
        percent: if from_end { 1.0 } else { 0.0 } + sign * extra.percent,
        offset: sign * extra.offset,
    })
}

fn axis_from_length(value: &CssLength, metrics: Metrics) -> Result<Axis, String> {
    let source = value
        .to_css_string(Default::default())
        .map_err(|err| err.to_string())?;
    let resolved = length(&source, metrics)?;
    if resolved.is_percent() {
        Ok(Axis {
            percent: resolved.value(),
            offset: 0.0,
        })
    } else {
        Ok(Axis {
            percent: 0.0,
            offset: resolved.value(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> Metrics {
        Metrics {
            width: 100.0,
            height: 50.0,
            font_size: 16.0,
        }
    }

    fn assert_position(
        source: &str,
        x_percent: f32,
        x_offset: f32,
        y_percent: f32,
        y_offset: f32,
    ) {
        assert_eq!(
            parse_object_position(source, metrics()).unwrap_or_else(|err| panic!("{source}: {err}")),
            ParsedObjectPosition {
                x_percent,
                x_offset,
                y_percent,
                y_offset,
            }
        );
    }

    fn assert_rejected(source: &str) {
        assert_eq!(
            parse_object_position(source, metrics()).unwrap_err(),
            format!("unsupported object-position '{source}'")
        );
    }

    #[test]
    fn left_top_and_top_left() {
        assert_position("left top", 0.0, 0.0, 0.0, 0.0);
        assert_position("top left", 0.0, 0.0, 0.0, 0.0);
    }

    #[test]
    fn right_bottom() {
        assert_position("right bottom", 1.0, 0.0, 1.0, 0.0);
    }

    #[test]
    fn center() {
        assert_position("center", 0.5, 0.0, 0.5, 0.0);
    }

    #[test]
    fn left() {
        assert_position("left", 0.0, 0.0, 0.5, 0.0);
    }

    #[test]
    fn bottom() {
        assert_position("bottom", 0.5, 0.0, 1.0, 0.0);
    }

    #[test]
    fn pixel_offsets() {
        assert_position("10px 20px", 0.0, 10.0, 0.0, 20.0);
    }

    #[test]
    fn percent_fractions() {
        assert_position("25% 75%", 0.25, 0.0, 0.75, 0.0);
    }

    #[test]
    fn left_length_applies_to_y() {
        assert_position("left 20px", 0.0, 0.0, 0.0, 20.0);
    }

    #[test]
    fn right_offset_bottom() {
        assert_position("right 10px bottom", 1.0, -10.0, 1.0, 0.0);
    }

    #[test]
    fn bottom_and_right_offsets() {
        assert_position("bottom 4px right 8px", 1.0, -8.0, 1.0, -4.0);
    }

    #[test]
    fn left_and_top_offsets() {
        assert_position("left 10px top 6px", 0.0, 10.0, 0.0, 6.0);
    }

    #[test]
    fn homogeneous_calc() {
        assert_position("calc(8px + 2px) center", 0.0, 10.0, 0.5, 0.0);
    }

    #[test]
    fn three_values_are_rejected() {
        assert_rejected("50% 50% 10px");
    }

    #[test]
    fn empty_is_rejected() {
        assert_rejected("");
    }

    #[test]
    fn blank_keeps_the_original_source() {
        assert_rejected(" \t");
    }
}
