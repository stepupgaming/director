//! Parity helpers: frames painted from a live `Director` are the reference
//! that frames imported from its scene document must match.

use director_core::Director;
use director_render::{Frame, FrameSession, Renderer};

pub const CHANNEL_DELTA: u8 = 16;
pub const MAX_FRACTION: f64 = 0.001;

pub fn differing_fraction(a: &Frame, b: &Frame) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let different = a
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.rgba.as_chunks::<4>().0)
        .filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > CHANNEL_DELTA))
        .count();
    different as f64 / (a.width as f64 * a.height as f64)
}

/// Frames at `times` where `renderer` differs from the live `original`.
#[allow(dead_code)]
pub fn mismatches(name: &str, original: Director, renderer: &Renderer, times: &[f64]) -> Vec<String> {
    let mut imported = renderer.session().unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut original = FrameSession::new(original, renderer.output_size()).unwrap();
    let mut failures = Vec::new();
    for &time in times {
        let expected = original.render(time).unwrap_or_else(|e| panic!("{name} live at {time}s: {e}"));
        let actual = imported.render(time).unwrap_or_else(|e| panic!("{name} at {time}s: {e}"));
        let fraction = differing_fraction(&expected, &actual);
        if fraction > MAX_FRACTION {
            failures.push(format!("{name} at {time}s: {fraction:.6} of pixels differ"));
        }
    }
    failures
}
