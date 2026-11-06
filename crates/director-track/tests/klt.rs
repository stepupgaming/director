//! The Lucas-Kanade tracker on a smooth texture shifted by a known amount.

use director_track::{track_point, Gray};

/// A smooth texture of crossed sines, evaluated anywhere.
fn texture(x: f32, y: f32) -> f32 {
    128.0
        + 40.0 * (0.21 * x + 0.13 * y).sin()
        + 30.0 * (0.17 * y - 0.09 * x).cos()
        + 25.0 * (0.11 * x * 0.7 + 0.29 * y).sin() * (0.05 * x).cos()
}

fn image(dx: f32, dy: f32) -> Gray {
    let (w, h) = (160, 120);
    let pixels = (0..w * h).map(|i| texture((i % w) as f32 - dx, (i / w) as f32 - dy)).collect();
    Gray::new(w, h, pixels)
}

#[test]
fn a_window_follows_a_sub_pixel_shift() {
    let (dx, dy) = (2.3, -1.7);
    let (a, b) = (image(0.0, 0.0), image(dx, dy));
    let mut worst = 0.0f32;
    for p in [[40.0, 40.0], [80.0, 60.0], [120.0, 80.0], [60.0, 90.0], [100.0, 30.0]] {
        let q = track_point(&a, &b, p).expect("a textured window tracks");
        worst = worst.max(((q[0] - p[0] - dx).powi(2) + (q[1] - p[1] - dy).powi(2)).sqrt());
    }
    eprintln!("worst tracking error {worst:.4} px");
    assert!(worst < 0.05, "the tracker lands within 0.05 px: {worst}");
}

#[test]
fn a_flat_window_does_not_track() {
    let flat = Gray::new(64, 64, vec![100.0; 64 * 64]);
    assert_eq!(track_point(&flat, &flat, [32.0, 32.0]), None);
}
