//! Pyramidal Lucas-Kanade: a 15x15 window followed from the coarsest level
//! down, with the template's gradients standing in for the target's, and
//! an affine refinement against a track's first appearance.

use nalgebra::{Matrix6, Vector6};

use crate::image::{Gray, Level, Pyramid};

/// Half the window side.
const HALF: i32 = 7;
/// Iterations per level.
const ITERATIONS: usize = 30;

/// Where the window at `p` in `a` moved to in `b`, starting from `guess`.
/// `None` when the window has too little texture, the step leaves the
/// image, or the window no longer matches after converging.
pub fn track(a: &Pyramid, b: &Pyramid, p: [f32; 2], guess: [f32; 2]) -> Option<[f32; 2]> {
    let levels = a.levels.len().min(b.levels.len());
    let top = (1 << (levels - 1)) as f32;
    let mut d = [(guess[0] - p[0]) / top, (guess[1] - p[1]) / top];
    let side = (2 * HALF + 1) as usize;
    let n = side * side;
    let (mut tpl, mut ix, mut iy) = (vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]);
    let mut residual = 0.0;
    for l in (0..levels).rev() {
        let s = 1.0 / (1 << l) as f32;
        let (px, py) = (p[0] * s, p[1] * s);
        let (la, lb) = (&a.levels[l], &b.levels[l]);
        let (mut gxx, mut gxy, mut gyy) = (0.0f32, 0.0f32, 0.0f32);
        let mut k = 0;
        for dy in -HALF..=HALF {
            for dx in -HALF..=HALF {
                let (x, y) = (px + dx as f32, py + dy as f32);
                tpl[k] = la.image.cubic(x, y);
                ix[k] = la.gx.cubic(x, y);
                iy[k] = la.gy.cubic(x, y);
                gxx += ix[k] * ix[k];
                gxy += ix[k] * iy[k];
                gyy += iy[k] * iy[k];
                k += 1;
            }
        }
        let det = gxx * gyy - gxy * gxy;
        let min_eig = 0.5 * (gxx + gyy) - (0.25 * (gxx - gyy).powi(2) + gxy * gxy).sqrt();
        if min_eig / (n as f32) < 0.5 || det.abs() < 1e-6 {
            return None;
        }
        for _ in 0..ITERATIONS {
            let (mut bx, mut by) = (0.0f32, 0.0f32);
            let mut k = 0;
            for dy in -HALF..=HALF {
                for dx in -HALF..=HALF {
                    let diff = tpl[k] - lb.image.cubic(px + d[0] + dx as f32, py + d[1] + dy as f32);
                    bx += diff * ix[k];
                    by += diff * iy[k];
                    k += 1;
                }
            }
            let step = [(gyy * bx - gxy * by) / det, (gxx * by - gxy * bx) / det];
            d[0] += step[0];
            d[1] += step[1];
            if !(d[0].is_finite() && d[1].is_finite()) {
                return None;
            }
            if step[0] * step[0] + step[1] * step[1] < 1e-4 {
                break;
            }
        }
        if l == 0 {
            residual = window_error(&tpl, &lb.image, px + d[0], py + d[1]);
        } else {
            d[0] *= 2.0;
            d[1] *= 2.0;
        }
    }
    let q = [p[0] + d[0], p[1] + d[1]];
    let image = &b.levels[0].image;
    let inside = q[0] >= HALF as f32
        && q[1] >= HALF as f32
        && q[0] <= (image.width as i32 - 1 - HALF) as f32
        && q[1] <= (image.height as i32 - 1 - HALF) as f32;
    (inside && residual < 12.0).then_some(q)
}

/// The mean absolute difference between the template and `image` around (x, y).
fn window_error(tpl: &[f32], image: &Gray, x: f32, y: f32) -> f32 {
    let mut sum = 0.0;
    let mut k = 0;
    for dy in -HALF..=HALF {
        for dx in -HALF..=HALF {
            sum += (tpl[k] - image.cubic(x + dx as f32, y + dy as f32)).abs();
            k += 1;
        }
    }
    sum / k as f32
}

/// Tracks the point `p` from image `a` to image `b` through 3 pyramid
/// levels, for callers outside the frame-to-frame tracker.
pub fn track_point(a: &Gray, b: &Gray, p: [f32; 2]) -> Option<[f32; 2]> {
    track(&Pyramid::new(a, 3), &Pyramid::new(b, 3), p, p)
}

/// Half the side of the affine template window.
const AFFINE_HALF: i32 = 8;
/// Gauss-Newton steps of the affine refinement.
const AFFINE_ITERATIONS: usize = 15;

/// The window around `p` in `level`, sampled once where a track starts, so
/// every later frame matches the track's first appearance.
pub fn template(level: &Level, p: [f32; 2]) -> Vec<f32> {
    let mut out = Vec::with_capacity(((2 * AFFINE_HALF + 1) * (2 * AFFINE_HALF + 1)) as usize);
    for dy in -AFFINE_HALF..=AFFINE_HALF {
        for dx in -AFFINE_HALF..=AFFINE_HALF {
            out.push(level.image.cubic(p[0] + dx as f32, p[1] + dy as f32));
        }
    }
    out
}

/// An affine warp of the template window: offset w lands at
/// (I + [d0 d1; d2 d3]) w + (x, y), stored as `[d0, d1, d2, d3, x, y]`.
pub type Warp = [f32; 6];

/// Where the warp sends the template offset (dx, dy).
fn warped(w: &[f64; 6], dx: f64, dy: f64) -> (f32, f32) {
    ((dx + w[0] * dx + w[1] * dy + w[4]) as f32, (dy + w[2] * dx + w[3] * dy + w[5]) as f32)
}

/// Refines `warp` so the template matches `target` under it, by forward
/// additive Gauss-Newton on the six affine parameters. Returns the warp and
/// the mean absolute difference, or `None` when the system degenerates or
/// the warp folds or stretches past a factor of two.
pub fn align(template: &[f32], target: &Level, warp: Warp) -> Option<(Warp, f32)> {
    let mut w = warp.map(|v| v as f64);
    for _ in 0..AFFINE_ITERATIONS {
        let mut h = Matrix6::<f64>::zeros();
        let mut b = Vector6::<f64>::zeros();
        let mut k = 0;
        for dy in -AFFINE_HALF..=AFFINE_HALF {
            for dx in -AFFINE_HALF..=AFFINE_HALF {
                let (fx, fy) = (dx as f64, dy as f64);
                let (x, y) = warped(&w, fx, fy);
                let e = (template[k] - target.image.cubic(x, y)) as f64;
                let (gx, gy) = (target.gx.cubic(x, y) as f64, target.gy.cubic(x, y) as f64);
                let j = Vector6::new(gx * fx, gx * fy, gy * fx, gy * fy, gx, gy);
                h += j * j.transpose();
                b += j * e;
                k += 1;
            }
        }
        let step = h.cholesky()?.solve(&b);
        if !step.iter().all(|v| v.is_finite()) {
            return None;
        }
        for (wi, si) in w.iter_mut().zip(step.iter()) {
            *wi += si;
        }
        if step[4] * step[4] + step[5] * step[5] < 1e-6 {
            break;
        }
    }
    let det = (1.0 + w[0]) * (1.0 + w[3]) - w[1] * w[2];
    if !(0.5..=2.0).contains(&det) {
        return None;
    }
    let mut sum = 0.0;
    let mut k = 0;
    for dy in -AFFINE_HALF..=AFFINE_HALF {
        for dx in -AFFINE_HALF..=AFFINE_HALF {
            let (x, y) = warped(&w, dx as f64, dy as f64);
            sum += (template[k] - target.image.cubic(x, y)).abs();
            k += 1;
        }
    }
    Some((w.map(|v| v as f32), sum / k as f32))
}
