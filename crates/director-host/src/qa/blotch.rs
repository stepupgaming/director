//! Encoder blotching: the luma of a finished encode against the painted
//! frames, in dark flat areas where block quantization shows as steps.

use director_render::Frame;

/// H.264 macroblock edge in pixels. HEVC and AV1 block edges fall on it too.
const BLOCK: usize = 16;
/// A block is dark below this mean luma (8-bit, full range).
const DARK: f64 = 64.0;
/// Blocks darker than this are black, which encoders keep exact.
const BLACK: f64 = 3.0;
/// A dark block is flat when its painted luma spans at most this many levels.
const FLAT: f64 = 6.0;
/// A block edge steps when the encode's mean step across it differs from the
/// painted step by at least this many levels: three quarters of its pixels
/// jump a whole level, a contour along the edge.
const STEP: f64 = 0.75;
/// Dark flat edges must cover this share of the frame's block edges to count.
const MIN_AREA: f64 = 0.02;
/// A frame blotches when this share of its dark flat edges step.
const BLOTCHED: f64 = 0.1;

/// Blotching in one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Blotch {
    /// Share of the frame's block edges between two dark flat blocks.
    pub area: f64,
    /// Share of those edges whose encoded step departs from the painted one.
    pub stepped: f64,
}

impl Blotch {
    pub(super) fn visible(&self) -> bool {
        self.area >= MIN_AREA && self.stepped >= BLOTCHED
    }
}

/// Painted luma (BT.709 on the encoded values, over black), the encoder's own
/// conversion for full-range output.
fn luma(frame: &Frame) -> Vec<f32> {
    frame
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) * p[3] as f32 / 255.0)
        .collect()
}

/// Compares the encode's luma plane with the painted frame on the macroblock grid.
pub(super) fn measure(frame: &Frame, encoded: &[u8]) -> Blotch {
    let (width, height) = (frame.width as usize, frame.height as usize);
    let painted = luma(frame);
    let (columns, rows) = (width / BLOCK, height / BLOCK);
    let flat: Vec<bool> = (0..rows * columns)
        .map(|cell| {
            let (bx, by) = (cell % columns * BLOCK, cell / columns * BLOCK);
            let (mut sum, mut low, mut high) = (0.0f64, f32::MAX, f32::MIN);
            for y in by..by + BLOCK {
                for &value in &painted[y * width + bx..y * width + bx + BLOCK] {
                    sum += value as f64;
                    low = low.min(value);
                    high = high.max(value);
                }
            }
            let mean = sum / (BLOCK * BLOCK) as f64;
            (BLACK..DARK).contains(&mean) && ((high - low) as f64) <= FLAT
        })
        .collect();
    // Mean step across an edge, encoded minus painted, over its BLOCK pixels.
    let step = |a: usize, b: usize, along: usize| -> f64 {
        let mut total = 0.0;
        for k in 0..BLOCK {
            let (p, q) = (a + k * along, b + k * along);
            total += (encoded[q] as f64 - encoded[p] as f64) - (painted[q] - painted[p]) as f64;
        }
        (total / BLOCK as f64).abs()
    };
    let (mut edges, mut dark, mut stepped) = (0usize, 0usize, 0usize);
    for by in 0..rows {
        for bx in 0..columns {
            let here = by * columns + bx;
            // The left and the top edge of each block.
            if bx > 0 {
                edges += 1;
                if flat[here] && flat[here - 1] {
                    dark += 1;
                    let x = bx * BLOCK;
                    stepped += usize::from(step(by * BLOCK * width + x - 1, by * BLOCK * width + x, width) >= STEP);
                }
            }
            if by > 0 {
                edges += 1;
                if flat[here] && flat[here - columns] {
                    dark += 1;
                    let y = by * BLOCK;
                    stepped += usize::from(step((y - 1) * width + bx * BLOCK, y * width + bx * BLOCK, 1) >= STEP);
                }
            }
        }
    }
    Blotch {
        area: if edges == 0 { 0.0 } else { dark as f64 / edges as f64 },
        stepped: if dark == 0 { 0.0 } else { stepped as f64 / dark as f64 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> Frame {
        let mut rgba = Vec::new();
        for y in 0..height {
            for _ in 0..width {
                let value = 10 + (20 * y / height) as u8;
                rgba.extend([value, value, value, 255]);
            }
        }
        Frame { width, height, rgba }
    }

    #[test]
    fn block_steps_in_dark_gradients_blotch_and_matching_luma_does_not() {
        let frame = gradient(64, 64);
        let exact: Vec<u8> = luma(&frame).iter().map(|value| value.round() as u8).collect();
        let clean = measure(&frame, &exact);
        assert!(clean.area > 0.9 && clean.stepped == 0.0, "{clean:?}");
        assert!(!clean.visible());

        // Every 16-pixel macroblock flattened to its first row, offset per block.
        let blocky: Vec<u8> = (0..64 * 64)
            .map(|at| {
                let (x, y) = (at % 64, at / 64);
                exact[(y / 16 * 16) * 64 + x / 16 * 16] + ((x / 16 + y / 16) % 2 * 2) as u8
            })
            .collect();
        let blotched = measure(&frame, &blocky);
        assert!(blotched.visible(), "{blotched:?}");
    }
}
