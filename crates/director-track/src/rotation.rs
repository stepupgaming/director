//! The camera of a shot that only turns, as on a tripod: no parallax, so the
//! tracks fix each frame's rotation and, given enough turn, the focal
//! length, frame by frame for a zooming lens, but no depth.

use nalgebra::{Matrix3, Vector2, Vector3, SVD};

use crate::geometry::{Cam, Intrinsics, Rng};
use crate::tracks::Track;

/// A track counts as an inlier of a frame step within this many pixels.
const STEP_ERROR: f64 = 1.5;
/// After the global passes, tracks whose rms error passes this many pixels go.
const TRACK_ERROR: f64 = 2.0;
/// Global passes of alternating directions, rotations and focal lengths.
const PASSES: usize = 400;
/// Passes for each trial focal length while frame 0's lens is searched.
const SEARCH_PASSES: usize = 100;

/// What the lens may do while the rotations solve.
#[derive(Clone, Copy, Debug)]
pub struct Lens {
    /// Search frame 0's focal length from the guess.
    pub refine: bool,
    /// Give every frame its own focal length.
    pub zoom: bool,
}

/// A solved turn: every frame's rotation (world to camera, frame 0 the
/// identity), its focal length in pixels and the rms reprojection error in pixels.
pub struct Turn {
    pub cams: Vec<Cam>,
    pub focals: Vec<f64>,
    pub error: f64,
}

/// Solves the turn of `n` frames from `tracks`, with `intr.f` the focal
/// length guess, refined to frame 0's when the lens refines or zooms.
pub fn solve(tracks: &[Track], intr: &mut Intrinsics, lens: Lens, n: usize) -> Result<Turn, String> {
    if lens.refine {
        // The rms over frame 0's focal length, which every other frame's
        // follows with a zoom and equals without: golden-section search on
        // its log from half to twice the guess. Freeing frame 0 beside the
        // others lets the whole lens drift.
        let f0 = intr.f;
        let at = |log: f64| -> f64 {
            let trial = Intrinsics { f: f0 * log.exp(), ..*intr };
            fit(tracks, &trial, &mut vec![trial.f; n], lens.zoom, SEARCH_PASSES).map_or(f64::INFINITY, |(_, e)| e)
        };
        let golden = (5f64.sqrt() - 1.0) / 2.0;
        let (mut a, mut b) = (0.5f64.ln(), 2f64.ln());
        let (mut c, mut d) = (b - golden * (b - a), a + golden * (b - a));
        let (mut fc, mut fd) = (at(c), at(d));
        for _ in 0..30 {
            if fc < fd {
                (b, d, fd) = (d, c, fc);
                c = b - golden * (b - a);
                fc = at(c);
            } else {
                (a, c, fc) = (c, d, fd);
                d = a + golden * (b - a);
                fd = at(d);
            }
        }
        intr.f = f0 * ((a + b) / 2.0).exp();
    }
    let mut focals = vec![intr.f; n];
    let (rotations, error) = fit(tracks, intr, &mut focals, lens.zoom, PASSES)?;
    intr.f = focals[0];
    Ok(Turn { cams: rotations.into_iter().map(|r| Cam { r, t: Vector3::zeros() }).collect(), focals, error })
}

/// Chains and refines the rotations (and with a zoom the focal lengths),
/// drops tracks that do not fit and refines again. Returns the rotations
/// and the rms.
fn fit(tracks: &[Track], intr: &Intrinsics, focals: &mut [f64], zoom: bool, passes: usize) -> Result<(Vec<Matrix3<f64>>, f64), String> {
    let mut used = vec![true; tracks.len()];
    let mut rotations = chain(tracks, intr, focals, zoom)?;
    refine(tracks, intr, focals, zoom, passes, &used, &mut rotations);
    let errors = track_errors(tracks, intr, focals, &used, &rotations);
    for (u, e) in used.iter_mut().zip(&errors) {
        if e.is_some_and(|e| e > TRACK_ERROR) {
            *u = false;
        }
    }
    refine(tracks, intr, focals, zoom, passes, &used, &mut rotations);
    let error = rms(tracks, intr, focals, &used, &rotations);
    Ok((rotations, error))
}

/// The lens of a frame with focal length `f`.
fn with_focal(intr: &Intrinsics, f: f64) -> Intrinsics {
    Intrinsics { f, ..*intr }
}

/// Each track's unit rays in its own frames' camera coordinates.
fn bearings(tracks: &[Track], intr: &Intrinsics, focals: &[f64]) -> Vec<Vec<Vector3<f64>>> {
    tracks
        .iter()
        .map(|t| t.points.iter().enumerate().map(|(i, &p)| bearing(intr, focals[t.start + i], p)).collect())
        .collect()
}

fn bearing(intr: &Intrinsics, f: f64, p: [f32; 2]) -> Vector3<f64> {
    with_focal(intr, f).normalize(p).push(1.0).normalize()
}

/// The focal length that best sends the rays `c` (in camera coordinates)
/// onto their pixels, by least squares on the distorted z = 1 plane.
fn focal(intr: &Intrinsics, rays: impl Iterator<Item = (Vector3<f64>, [f32; 2])>) -> Option<f64> {
    let (mut up, mut pp) = (0.0, 0.0);
    for (c, uv) in rays {
        if c.z <= 1e-9 {
            continue;
        }
        let p = Vector2::new(c.x / c.z, c.y / c.z);
        let p = p * intr.radial(p.norm_squared());
        let u = Vector2::new(uv[0] as f64 - intr.cx, uv[1] as f64 - intr.cy);
        up += u.dot(&p);
        pp += p.norm_squared();
    }
    (pp > 1e-12 && up > 0.0).then(|| up / pp)
}

/// The rotation r with b ≈ r a over the pairs, by Kabsch.
fn kabsch(pairs: impl Iterator<Item = (Vector3<f64>, Vector3<f64>)>) -> Option<Matrix3<f64>> {
    let h = pairs.fold(Matrix3::zeros(), |h, (a, b)| h + a * b.transpose());
    let svd = SVD::new(h, true, true);
    let (u, v_t) = (svd.u?, svd.v_t?);
    let v = v_t.transpose();
    let sign = (v * u.transpose()).determinant().signum();
    Some(v * Matrix3::from_diagonal(&Vector3::new(1.0, 1.0, sign)) * u.transpose())
}

/// Chains frame-to-frame rotations, each from RANSAC over pairs of the
/// tracks both frames share. With a zoom each frame's focal length starts
/// from the frame before and follows its matched rays.
fn chain(tracks: &[Track], intr: &Intrinsics, focals: &mut [f64], zoom: bool) -> Result<Vec<Matrix3<f64>>, String> {
    let mut rng = Rng::new(5);
    let mut rotations = vec![Matrix3::identity()];
    for k in 1..focals.len() {
        if zoom {
            focals[k] = focals[k - 1];
        }
        let shared: Vec<(Vector3<f64>, [f32; 2])> = tracks
            .iter()
            .filter_map(|t| Some((bearing(intr, focals[k - 1], t.at(k - 1)?), t.at(k)?)))
            .collect();
        if shared.len() < 8 {
            return Err(format!("frame {k} shares too few tracks ({}) with the frame before to follow the camera's turn", shared.len()));
        }
        let mut step = None;
        for round in 0..if zoom { 4 } else { 1 } {
            let pairs: Vec<(Vector3<f64>, Vector3<f64>)> = shared.iter().map(|&(a, uv)| (a, bearing(intr, focals[k], uv))).collect();
            let limit = STEP_ERROR / focals[k];
            let inliers = |r: &Matrix3<f64>| -> Vec<bool> { pairs.iter().map(|(a, b)| (r * a - b).norm() < limit).collect() };
            let r = match step {
                Some(r) if round > 0 => r,
                _ => {
                    let mut best: Option<(Matrix3<f64>, usize)> = None;
                    for _ in 0..100 {
                        let (i, j) = (rng.below(pairs.len()), rng.below(pairs.len()));
                        if i == j {
                            continue;
                        }
                        let Some(r) = kabsch([pairs[i], pairs[j]].into_iter()) else { continue };
                        let count = inliers(&r).iter().filter(|&&x| x).count();
                        if best.as_ref().is_none_or(|(_, c)| count > *c) {
                            best = Some((r, count));
                        }
                    }
                    best.ok_or_else(|| format!("frame {k}'s turn found no consistent tracks"))?.0
                }
            };
            let mask = inliers(&r);
            let r = kabsch(pairs.iter().zip(&mask).filter(|(_, &m)| m).map(|(p, _)| *p)).unwrap_or(r);
            if zoom {
                let rays = shared.iter().zip(&mask).filter(|(_, &m)| m).map(|(&(a, uv), _)| (r * a, uv));
                if let Some(f) = focal(intr, rays) {
                    focals[k] = f;
                }
            }
            step = Some(r);
        }
        rotations.push(step.expect("one round") * rotations[k - 1]);
    }
    Ok(rotations)
}

/// Each used track's world direction from its rotated rays.
fn directions(tracks: &[Track], bearings: &[Vec<Vector3<f64>>], used: &[bool], rotations: &[Matrix3<f64>]) -> Vec<Option<Vector3<f64>>> {
    tracks
        .iter()
        .zip(bearings)
        .zip(used)
        .map(|((t, b), &u)| {
            if !u || t.points.len() < 2 {
                return None;
            }
            let sum: Vector3<f64> = b.iter().enumerate().map(|(i, b)| rotations[t.start + i].transpose() * b).sum();
            sum.try_normalize(1e-12)
        })
        .collect()
}

/// Alternates the tracks' directions with the frames' rotations and, for a
/// zoom, their focal lengths with frame 0's held, then holds frame 0 at the
/// identity.
fn refine(tracks: &[Track], intr: &Intrinsics, focals: &mut [f64], zoom: bool, passes: usize, used: &[bool], rotations: &mut [Matrix3<f64>]) {
    let mut rays = bearings(tracks, intr, focals);
    for _ in 0..passes {
        let dirs = directions(tracks, &rays, used, rotations);
        let mut pairs: Vec<Vec<(Vector3<f64>, Vector3<f64>)>> = vec![Vec::new(); rotations.len()];
        let mut seen: Vec<Vec<(Vector3<f64>, [f32; 2])>> = vec![Vec::new(); rotations.len()];
        for ((t, b), d) in tracks.iter().zip(&rays).zip(&dirs) {
            let Some(d) = d else { continue };
            for (i, b) in b.iter().enumerate() {
                pairs[t.start + i].push((*d, *b));
                seen[t.start + i].push((*d, t.points[i]));
            }
        }
        for (r, pairs) in rotations.iter_mut().zip(pairs) {
            if pairs.len() >= 3 {
                if let Some(fit) = kabsch(pairs.into_iter()) {
                    *r = fit;
                }
            }
        }
        if zoom {
            for (k, seen) in seen.into_iter().enumerate().skip(1) {
                if seen.len() < 3 {
                    continue;
                }
                let r = rotations[k];
                if let Some(f) = focal(intr, seen.into_iter().map(|(d, uv)| (r * d, uv))) {
                    focals[k] = f;
                }
            }
            rays = bearings(tracks, intr, focals);
        }
    }
    let first = rotations[0];
    for r in rotations.iter_mut() {
        *r *= first.transpose();
    }
}

/// Each used track's rms reprojection error in pixels.
fn track_errors(tracks: &[Track], intr: &Intrinsics, focals: &[f64], used: &[bool], rotations: &[Matrix3<f64>]) -> Vec<Option<f64>> {
    let rays = bearings(tracks, intr, focals);
    let dirs = directions(tracks, &rays, used, rotations);
    tracks
        .iter()
        .zip(&dirs)
        .map(|(t, d)| {
            let d = (*d)?;
            let sum: f64 = t
                .points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let k = t.start + i;
                    let cam = Cam { r: rotations[k], t: Vector3::zeros() };
                    with_focal(intr, focals[k])
                        .project(&cam, &d)
                        .map_or(1e6, |q| (q.x - p[0] as f64).powi(2) + (q.y - p[1] as f64).powi(2))
                })
                .sum();
            Some((sum / t.points.len() as f64).sqrt())
        })
        .collect()
}

/// The rms reprojection error of every used observation in pixels.
fn rms(tracks: &[Track], intr: &Intrinsics, focals: &[f64], used: &[bool], rotations: &[Matrix3<f64>]) -> f64 {
    let (sum, count) = track_errors(tracks, intr, focals, used, rotations)
        .iter()
        .zip(tracks)
        .filter_map(|(e, t)| e.map(|e| (e * e * t.points.len() as f64, t.points.len())))
        .fold((0.0, 0), |(s, c), (e, n)| (s + e, c + n));
    if count == 0 {
        f64::INFINITY
    } else {
        (sum / count as f64).sqrt()
    }
}
