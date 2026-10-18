//! Place background-image tiles from per-axis placement.

use crate::background_box::{place_axis, AxisRepeat, AxisTiles};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedTile {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub fn place_tiles(
    area_x: f32,
    area_y: f32,
    area_w: f32,
    area_h: f32,
    tile_w: f32,
    tile_h: f32,
    pos_x_percent: f32,
    pos_x_length: f32,
    pos_y_percent: f32,
    pos_y_length: f32,
    repeat_x: AxisRepeat,
    repeat_y: AxisRepeat,
) -> Vec<PlacedTile> {
    let x_axis: AxisTiles = place_axis(area_w, tile_w, pos_x_percent, pos_x_length, repeat_x);
    let y_axis: AxisTiles = place_axis(area_h, tile_h, pos_y_percent, pos_y_length, repeat_y);
    if x_axis.count == 0 || y_axis.count == 0 {
        return Vec::new();
    }
    if u64::from(x_axis.count) * u64::from(y_axis.count) > 4096 {
        return Vec::new();
    }
    let mut tiles = Vec::with_capacity(x_axis.count as usize * y_axis.count as usize);
    for j in 0..y_axis.count {
        for i in 0..x_axis.count {
            tiles.push(PlacedTile {
                x: area_x + x_axis.origin + i as f32 * (x_axis.tile + x_axis.gap),
                y: area_y + y_axis.origin + j as f32 * (y_axis.tile + y_axis.gap),
                w: x_axis.tile,
                h: y_axis.tile,
            });
        }
    }
    tiles
}

#[cfg(test)]
mod tests {
    use super::{place_tiles, PlacedTile};
    use crate::background_box::AxisRepeat;

    fn tile(x: f32, y: f32, w: f32, h: f32) -> PlacedTile {
        PlacedTile { x, y, w, h }
    }

    #[test]
    fn space_six_tiles_ignore_position() {
        assert_eq!(
            place_tiles(
                10.0,
                20.0,
                100.0,
                100.0,
                30.0,
                40.0,
                1.0,
                50.0,
                1.0,
                50.0,
                AxisRepeat::Space,
                AxisRepeat::Space,
            ),
            vec![
                tile(10.0, 20.0, 30.0, 40.0),
                tile(45.0, 20.0, 30.0, 40.0),
                tile(80.0, 20.0, 30.0, 40.0),
                tile(10.0, 80.0, 30.0, 40.0),
                tile(45.0, 80.0, 30.0, 40.0),
                tile(80.0, 80.0, 30.0, 40.0),
            ]
        );
    }

    #[test]
    fn round_resizes_tile_and_ignores_position() {
        let tile_w = 100.0 / 3.0;
        assert_eq!(
            place_tiles(
                0.0,
                0.0,
                100.0,
                90.0,
                30.0,
                40.0,
                1.0,
                50.0,
                1.0,
                50.0,
                AxisRepeat::Round,
                AxisRepeat::Round,
            ),
            vec![
                tile(0.0, 0.0, tile_w, 45.0),
                tile(tile_w, 0.0, tile_w, 45.0),
                tile(tile_w * 2.0, 0.0, tile_w, 45.0),
                tile(0.0, 45.0, tile_w, 45.0),
                tile(tile_w, 45.0, tile_w, 45.0),
                tile(tile_w * 2.0, 45.0, tile_w, 45.0),
            ]
        );
    }

    #[test]
    fn repeat_both_axes_is_one_tile() {
        let area_x = 10.0;
        let area_y = 20.0;
        assert_eq!(
            place_tiles(
                area_x,
                area_y,
                100.0,
                100.0,
                40.0,
                40.0,
                0.5,
                0.0,
                0.5,
                0.0,
                AxisRepeat::Repeat,
                AxisRepeat::Repeat,
            ),
            vec![tile(area_x + 30.0, area_y + 30.0, 40.0, 40.0)]
        );
    }

    #[test]
    fn zero_area_is_empty() {
        assert!(place_tiles(
            10.0,
            20.0,
            0.0,
            100.0,
            30.0,
            40.0,
            0.5,
            0.0,
            0.5,
            0.0,
            AxisRepeat::Repeat,
            AxisRepeat::Repeat,
        )
        .is_empty());
        assert!(place_tiles(
            10.0,
            20.0,
            100.0,
            0.0,
            30.0,
            40.0,
            0.5,
            0.0,
            0.5,
            0.0,
            AxisRepeat::Repeat,
            AxisRepeat::Repeat,
        )
        .is_empty());
    }

    #[test]
    fn count_product_above_4096_is_empty() {
        assert!(place_tiles(
            0.0,
            0.0,
            100_000.0,
            100_000.0,
            1.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            AxisRepeat::Space,
            AxisRepeat::Space,
        )
        .is_empty());
    }
}
