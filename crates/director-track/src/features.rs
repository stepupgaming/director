//! Shi-Tomasi corners: the smaller eigenvalue of the gradient structure
//! tensor over a 5x5 window.

use crate::image::Gray;

/// Columns of the regions that share the corner budget; the rows follow the
/// frame's aspect.
const REGIONS_ACROSS: usize = 4;
/// The share of the strongest corner's score a corner needs to claim its
/// region's share.
const CLAIM: f32 = 0.05;

/// Up to `max` corners of `image` at least `min_distance` pixels from each
/// other and from `existing`, `margin` pixels inside the border and outside
/// `masked` (one flag per pixel, empty for none). Corners weaker than 1 % of
/// the strongest are dropped. The frame splits into regions that each take
/// the strongest corners up to an equal share of `max` with `existing`
/// counted against it, so a busy region such as foliage in the wind cannot
/// take every track from the quieter ground and walls; the strongest corners
/// left over then fill the rest of `max`.
#[allow(clippy::too_many_arguments)]
pub fn corners(
    image: &Gray,
    gx: &Gray,
    gy: &Gray,
    existing: &[[f32; 2]],
    masked: &[bool],
    max: usize,
    min_distance: f32,
    margin: usize,
) -> Vec<[f32; 2]> {
    let (w, h) = (image.width, image.height);
    if max == 0 || w <= 2 * margin + 2 || h <= 2 * margin + 2 {
        return Vec::new();
    }
    let mut score = vec![0.0f32; w * h];
    for y in margin..h - margin {
        for x in margin..w - margin {
            let (mut a, mut b, mut c) = (0.0, 0.0, 0.0);
            for dy in -2..=2isize {
                for dx in -2..=2isize {
                    let (px, py) = (x as isize + dx, y as isize + dy);
                    let (ix, iy) = (gx.get(px, py), gy.get(px, py));
                    a += ix * ix;
                    b += ix * iy;
                    c += iy * iy;
                }
            }
            let half = 0.5 * (a + c);
            score[y * w + x] = half - (0.25 * (a - c) * (a - c) + b * b).sqrt();
        }
    }
    let strongest = score.iter().copied().fold(0.0, f32::max);
    if strongest <= 0.0 {
        return Vec::new();
    }
    let floor = 0.01 * strongest;
    let mut candidates = Vec::new();
    for y in margin.max(1)..h - margin.max(1) {
        for x in margin.max(1)..w - margin.max(1) {
            let s = score[y * w + x];
            if s < floor || masked.get(y * w + x).copied().unwrap_or(false) {
                continue;
            }
            let peak = (-1..=1isize).all(|dy| {
                (-1..=1isize).all(|dx| {
                    (dx == 0 && dy == 0) || score[(y as isize + dy) as usize * w + (x as isize + dx) as usize] <= s
                })
            });
            if peak {
                candidates.push((s, x, y));
            }
        }
    }
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    let cell = min_distance.max(1.0);
    let (gw, gh) = ((w as f32 / cell).ceil() as usize + 1, (h as f32 / cell).ceil() as usize + 1);
    let mut grid: Vec<Vec<[f32; 2]>> = vec![Vec::new(); gw * gh];
    let slot = |p: [f32; 2]| ((p[0] / cell) as usize).min(gw - 1) + ((p[1] / cell) as usize).min(gh - 1) * gw;
    for &p in existing {
        grid[slot(p)].push(p);
    }
    let free = |grid: &[Vec<[f32; 2]>], p: [f32; 2]| {
        let (cx, cy) = (((p[0] / cell) as isize).min(gw as isize - 1), ((p[1] / cell) as isize).min(gh as isize - 1));
        (-1..=1).all(|dy| {
            (-1..=1).all(|dx| {
                let (x, y) = (cx + dx, cy + dy);
                if x < 0 || y < 0 || x >= gw as isize || y >= gh as isize {
                    return true;
                }
                grid[y as usize * gw + x as usize]
                    .iter()
                    .all(|q| (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) >= min_distance * min_distance)
            })
        })
    };
    let across = REGIONS_ACROSS;
    let down = ((across * h) as f32 / w as f32).round().max(1.0) as usize;
    let region = |p: [f32; 2]| ((p[0] as usize * across / w).min(across - 1)) + ((p[1] as usize * down / h).min(down - 1)) * across;
    let share = (max + existing.len()).div_ceil(across * down);
    let mut held = vec![0usize; across * down];
    for &p in existing {
        held[region(p)] += 1;
    }
    let mut out = Vec::new();
    let mut left = Vec::new();
    for (s, x, y) in candidates {
        let p = [x as f32, y as f32];
        if held[region(p)] >= share || s < CLAIM * strongest {
            left.push(p);
        } else if free(&grid, p) {
            grid[slot(p)].push(p);
            held[region(p)] += 1;
            out.push(p);
            if out.len() == max {
                return out;
            }
        }
    }
    for p in left {
        if free(&grid, p) {
            grid[slot(p)].push(p);
            out.push(p);
            if out.len() == max {
                break;
            }
        }
    }
    out
}
