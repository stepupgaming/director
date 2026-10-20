//! Place one background-image axis.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AxisRepeat {
    Repeat,
    NoRepeat,
    Space,
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisTiles {
    pub origin: f32,
    pub tile: f32,
    pub gap: f32,
    pub count: u32,
}

const EMPTY: AxisTiles = AxisTiles {
    origin: 0.0,
    tile: 0.0,
    gap: 0.0,
    count: 0,
};

pub fn place_axis(
    area: f32,
    tile: f32,
    pos_percent: f32,
    pos_length: f32,
    repeat: AxisRepeat,
) -> AxisTiles {
    if area <= 0.0 || tile <= 0.0 || !area.is_finite() || !tile.is_finite() {
        return EMPTY;
    }
    match repeat {
        AxisRepeat::Repeat | AxisRepeat::NoRepeat => placed(area, tile, pos_percent, pos_length),
        AxisRepeat::Space => {
            let count = f32::floor(area / tile) as u32;
            if count <= 1 {
                placed(area, tile, pos_percent, pos_length)
            } else {
                AxisTiles {
                    origin: 0.0,
                    tile,
                    gap: (area - count as f32 * tile) / (count as f32 - 1.0),
                    count,
                }
            }
        }
        AxisRepeat::Round => {
            let count = f32::round(area / tile).max(1.0) as u32;
            AxisTiles {
                origin: 0.0,
                tile: area / count as f32,
                gap: 0.0,
                count,
            }
        }
    }
}

fn placed(area: f32, tile: f32, pos_percent: f32, pos_length: f32) -> AxisTiles {
    AxisTiles {
        origin: pos_percent * (area - tile) + pos_length,
        tile,
        gap: 0.0,
        count: 1,
    }
}

pub fn parse_repeat(source: &str) -> Result<(AxisRepeat, AxisRepeat), String> {
    let trimmed = source.trim();
    let mut tokens = trimmed.split_whitespace();
    let first = tokens.next();
    let second = tokens.next();
    let parsed = if tokens.next().is_some() {
        None
    } else {
        match (first, second) {
            (Some("repeat"), None) => Some((AxisRepeat::Repeat, AxisRepeat::Repeat)),
            (Some("no-repeat"), None) => Some((AxisRepeat::NoRepeat, AxisRepeat::NoRepeat)),
            (Some("repeat-x"), None) => Some((AxisRepeat::Repeat, AxisRepeat::NoRepeat)),
            (Some("repeat-y"), None) => Some((AxisRepeat::NoRepeat, AxisRepeat::Repeat)),
            (Some("space"), None) => Some((AxisRepeat::Space, AxisRepeat::Space)),
            (Some("round"), None) => Some((AxisRepeat::Round, AxisRepeat::Round)),
            (Some(first), Some(second)) => match (axis_keyword(first), axis_keyword(second)) {
                (Some(horizontal), Some(vertical)) => Some((horizontal, vertical)),
                _ => None,
            },
            _ => None,
        }
    };
    parsed.ok_or_else(|| format!("unsupported background-repeat '{trimmed}'"))
}

fn axis_keyword(token: &str) -> Option<AxisRepeat> {
    match token {
        "repeat" => Some(AxisRepeat::Repeat),
        "no-repeat" => Some(AxisRepeat::NoRepeat),
        "space" => Some(AxisRepeat::Space),
        "round" => Some(AxisRepeat::Round),
        _ => None,
    }
}

#[allow(dead_code)]
pub fn tile_origins(tiles: AxisTiles) -> Vec<f32> {
    let mut origins = Vec::with_capacity(tiles.count as usize);
    let step = tiles.tile + tiles.gap;
    for index in 0..tiles.count {
        origins.push(tiles.origin + index as f32 * step);
    }
    origins
}

#[cfg(test)]
mod tests {
    use super::{parse_repeat, place_axis, tile_origins, AxisRepeat, AxisTiles};

    fn tiles(origin: f32, tile: f32, gap: f32, count: u32) -> AxisTiles {
        AxisTiles {
            origin,
            tile,
            gap,
            count,
        }
    }

    #[test]
    fn no_repeat_percent_and_length() {
        assert_eq!(
            place_axis(100.0, 40.0, 0.5, 0.0, AxisRepeat::NoRepeat),
            tiles(30.0, 40.0, 0.0, 1)
        );
    }

    #[test]
    fn repeat_is_one_tile_at_position() {
        assert_eq!(
            place_axis(100.0, 40.0, 0.0, 10.0, AxisRepeat::Repeat),
            tiles(10.0, 40.0, 0.0, 1)
        );
    }

    #[test]
    fn space_gap_ignores_position() {
        assert_eq!(
            place_axis(100.0, 30.0, 1.0, 12.0, AxisRepeat::Space),
            tiles(0.0, 30.0, 5.0, 3)
        );
    }

    #[test]
    fn space_fits_once_uses_position() {
        assert_eq!(
            place_axis(100.0, 60.0, 1.0, 0.0, AxisRepeat::Space),
            tiles(40.0, 60.0, 0.0, 1)
        );
    }

    #[test]
    fn space_does_not_fit_uses_position() {
        assert_eq!(
            place_axis(50.0, 80.0, 0.0, 0.0, AxisRepeat::Space),
            tiles(0.0, 80.0, 0.0, 1)
        );
    }

    #[test]
    fn round_three_from_thirty() {
        assert_eq!(
            place_axis(100.0, 30.0, 0.5, 4.0, AxisRepeat::Round),
            tiles(0.0, 100.0 / 3.0, 0.0, 3)
        );
    }

    #[test]
    fn round_half_away_from_zero() {
        assert_eq!(
            place_axis(100.0, 40.0, 1.0, 7.0, AxisRepeat::Round),
            tiles(0.0, 100.0 / 3.0, 0.0, 3)
        );
    }

    #[test]
    fn round_one_tile_fills_area() {
        assert_eq!(
            place_axis(100.0, 80.0, 0.25, 3.0, AxisRepeat::Round),
            tiles(0.0, 100.0, 0.0, 1)
        );
    }

    #[test]
    fn non_finite_area_is_empty() {
        assert_eq!(
            place_axis(f32::NAN, 40.0, 0.5, 1.0, AxisRepeat::Repeat),
            tiles(0.0, 0.0, 0.0, 0)
        );
        assert_eq!(
            place_axis(f32::INFINITY, 40.0, 0.0, 0.0, AxisRepeat::Space),
            tiles(0.0, 0.0, 0.0, 0)
        );
    }

    #[test]
    fn parse_repeat_one_token() {
        assert_eq!(
            parse_repeat("repeat").unwrap(),
            (AxisRepeat::Repeat, AxisRepeat::Repeat)
        );
        assert_eq!(
            parse_repeat("no-repeat").unwrap(),
            (AxisRepeat::NoRepeat, AxisRepeat::NoRepeat)
        );
        assert_eq!(
            parse_repeat("repeat-x").unwrap(),
            (AxisRepeat::Repeat, AxisRepeat::NoRepeat)
        );
        assert_eq!(
            parse_repeat("repeat-y").unwrap(),
            (AxisRepeat::NoRepeat, AxisRepeat::Repeat)
        );
        assert_eq!(
            parse_repeat("space").unwrap(),
            (AxisRepeat::Space, AxisRepeat::Space)
        );
        assert_eq!(
            parse_repeat("round").unwrap(),
            (AxisRepeat::Round, AxisRepeat::Round)
        );
    }

    #[test]
    fn parse_repeat_two_tokens() {
        assert_eq!(
            parse_repeat("repeat space").unwrap(),
            (AxisRepeat::Repeat, AxisRepeat::Space)
        );
        assert_eq!(
            parse_repeat("space round").unwrap(),
            (AxisRepeat::Space, AxisRepeat::Round)
        );
        assert_eq!(
            parse_repeat("round no-repeat").unwrap(),
            (AxisRepeat::Round, AxisRepeat::NoRepeat)
        );
        assert_eq!(
            parse_repeat("no-repeat space").unwrap(),
            (AxisRepeat::NoRepeat, AxisRepeat::Space)
        );
        assert_eq!(
            parse_repeat("repeat repeat").unwrap(),
            (AxisRepeat::Repeat, AxisRepeat::Repeat)
        );
    }

    #[test]
    fn parse_repeat_rejects_bad_values() {
        let repeat_x = parse_repeat("repeat-x repeat").unwrap_err();
        assert!(repeat_x.contains("background-repeat"));
        assert_eq!(repeat_x, "unsupported background-repeat 'repeat-x repeat'");
        assert_eq!(
            parse_repeat("").unwrap_err(),
            "unsupported background-repeat ''"
        );
        assert_eq!(
            parse_repeat("space round extra").unwrap_err(),
            "unsupported background-repeat 'space round extra'"
        );
    }

    #[test]
    fn tile_origins_steps_by_tile_plus_gap() {
        assert_eq!(
            tile_origins(tiles(0.0, 30.0, 5.0, 3)),
            vec![0.0, 35.0, 70.0]
        );
    }

    #[test]
    fn tile_origins_empty_when_count_is_zero() {
        assert_eq!(tile_origins(tiles(0.0, 30.0, 5.0, 0)), Vec::<f32>::new());
    }

    #[test]
    fn round_origins_follow_resized_tile() {
        let placed = place_axis(100.0, 30.0, 0.5, 4.0, AxisRepeat::Round);
        let origins = tile_origins(placed);
        assert_eq!(origins.len(), 3);
        assert_eq!(placed.gap, 0.0);
        assert_eq!(origins[0], placed.origin);
        assert_eq!(origins[1], placed.origin + placed.tile);
        assert_eq!(origins[2], placed.origin + 2.0 * placed.tile);
    }
}
