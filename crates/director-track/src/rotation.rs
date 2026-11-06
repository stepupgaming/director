//! The camera of a shot that only turns, as on a tripod: no parallax, so the
//! tracks fix each frame's rotation and, given enough turn, the focal
//! length, but no depth.

use nalgebra::{Matrix3, Vector3, SVD};

use crate::geometry::{Cam, Intrinsics, Rng};
use crate::tracks::Track;

/// A track counts as an inlier of a frame step within this many pixels.
const STEP_ERROR: f64 = 1.5;
/// After the global passes, tracks whose rms error passes this many pixels go.
const TRACK_ERROR: f64 = 2.0;
/// Global passes of alternating directions and rotations.
const PASSES: usize = 400;

/// Every frame's rotation (world to camera, frame 0 the identity) and the
/// rms reprojection error in pixels, with `intr.f` refined when `refine_focal`.
pub fn solve(tracks: &[Track], intr: &mut Intrinsics, refine_focal: bool, n: usize) -> Result<(Vec<Cam>, f64), String> {
    let mut used = vec![true; tracks.len()];
    let fit = |intr: &Intrinsics, used: &mut Vec<bool>| -> Result<(Vec<Matrix3<f64>>, f64), String> {
        let bearings = bearings(tracks, intr);
        let mut rotations = chain(tracks, &bearings, intr, n)?;
        refine(tracks, &bearings, used, &mut rotations);
        let errors = track_errors(tracks, &bearings, used, &rotations, intr);
        for (u, e) in used.iter_mut().zip(&errors) {
            if e.is_some_and(|e| e > TRACK_ERROR) {
                *u = false;
            }
        }
        refine(tracks, &bearings, used, &mut rotations);
        Ok((rotations.clone(), rms(tracks, &bearings, used, &rotations, intr)))
    };
    if refine_focal {
        // The rms over the focal length: golden-section search on its log
        // from half to twice the guess.
        let f0 = intr.f;
        let at = |log: f64| -> f64 {
            let trial = Intrinsics { f: f0 * log.exp(), ..*intr };
            let mut used = vec![true; tracks.len()];
            fit(&trial, &mut used).map_or(f64::INFINITY, |(_, e)| e)
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
    let (rotations, error) = fit(intr, &mut used)?;
    Ok((rotations.into_iter().map(|r| Cam { r, t: Vector3::zeros() }).collect(), error))
}

/// Each track's unit rays in its own frames' camera coordinates.
fn bearings(tracks: &[Track], intr: &Intrinsics) -> Vec<Vec<Vector3<f64>>> {
    tracks.iter().map(|t| t.points.iter().map(|&p| intr.normalize(p).push(1.0).normalize()).collect()).collect()
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
/// tracks both frames share.
fn chain(tracks: &[Track], bearings: &[Vec<Vector3<f64>>], intr: &Intrinsics, n: usize) -> Result<Vec<Matrix3<f64>>, String> {
    let limit = STEP_ERROR / intr.f;
    let mut rng = Rng::new(5);
    let mut rotations = vec![Matrix3::identity()];
    for k in 1..n {
        let pairs: Vec<(Vector3<f64>, Vector3<f64>)> = tracks
            .iter()
            .zip(bearings)
            .filter(|(t, _)| t.at(k - 1).is_some() && t.at(k).is_some())
            .map(|(t, b)| (b[k - 1 - t.start], b[k - t.start]))
            .collect();
        if pairs.len() < 8 {
            return Err(format!("frame {k} shares too few tracks ({}) with the frame before to follow the camera's turn", pairs.len()));
        }
        let inliers = |r: &Matrix3<f64>| -> Vec<bool> { pairs.iter().map(|(a, b)| (r * a - b).norm() < limit).collect() };
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
        let (r, _) = best.ok_or_else(|| format!("frame {k}'s turn found no consistent tracks"))?;
        let mask = inliers(&r);
        let r = kabsch(pairs.iter().zip(&mask).filter(|(_, &m)| m).map(|(p, _)| *p)).unwrap_or(r);
        rotations.push(r * rotations[k - 1]);
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

/// Alternates the tracks' directions and the frames' rotations, then holds
/// frame 0 at the identity.
fn refine(tracks: &[Track], bearings: &[Vec<Vector3<f64>>], used: &[bool], rotations: &mut [Matrix3<f64>]) {
    for _ in 0..PASSES {
        let dirs = directions(tracks, bearings, used, rotations);
        let mut pairs: Vec<Vec<(Vector3<f64>, Vector3<f64>)>> = vec![Vec::new(); rotations.len()];
        for ((t, b), d) in tracks.iter().zip(bearings).zip(&dirs) {
            let Some(d) = d else { continue };
            for (i, b) in b.iter().enumerate() {
                pairs[t.start + i].push((*d, *b));
            }
        }
        for (r, pairs) in rotations.iter_mut().zip(pairs) {
            if pairs.len() >= 3 {
                if let Some(fit) = kabsch(pairs.into_iter()) {
                    *r = fit;
                }
            }
        }
    }
    let first = rotations[0];
    for r in rotations.iter_mut() {
        *r *= first.transpose();
    }
}

/// Each used track's rms reprojection error in pixels.
fn track_errors(
    tracks: &[Track],
    bearings: &[Vec<Vector3<f64>>],
    used: &[bool],
    rotations: &[Matrix3<f64>],
    intr: &Intrinsics,
) -> Vec<Option<f64>> {
    let dirs = directions(tracks, bearings, used, rotations);
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
                    let cam = Cam { r: rotations[t.start + i], t: Vector3::zeros() };
                    intr.project(&cam, &d).map_or(1e6, |q| (q.x - p[0] as f64).powi(2) + (q.y - p[1] as f64).powi(2))
                })
                .sum();
            Some((sum / t.points.len() as f64).sqrt())
        })
        .collect()
}

/// The rms reprojection error of every used observation in pixels.
fn rms(tracks: &[Track], bearings: &[Vec<Vector3<f64>>], used: &[bool], rotations: &[Matrix3<f64>], intr: &Intrinsics) -> f64 {
    let (sum, count) = track_errors(tracks, bearings, used, rotations, intr)
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
