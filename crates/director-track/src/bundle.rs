//! Levenberg-Marquardt bundle adjustment with a Schur complement on the
//! points, and a single camera's resection against fixed points. Residuals
//! are in pixels under a Huber loss.

use nalgebra::{DMatrix, DVector, Matrix2, Matrix2x3, Matrix3, Rotation3, SMatrix, Vector2, Vector3};

use crate::geometry::{Cam, Intrinsics};
use rayon::prelude::*;

/// Residuals beyond this many pixels count linearly.
const HUBER: f64 = 2.0;

/// One point seen by one camera at pixel `uv`.
#[derive(Clone, Copy, Debug)]
pub struct Obs {
    pub cam: usize,
    pub point: usize,
    pub uv: Vector2<f64>,
}

/// The lens parameters a bundle adjustment refines beside the cameras and points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Refine {
    /// The focal length.
    pub focal: bool,
    /// The radial distortion k1 and k2.
    pub distortion: bool,
}

impl Refine {
    /// The refined columns of `Linear::jl`: 0 the focal length, 1 k1, 2 k2.
    fn columns(self) -> Vec<usize> {
        let mut columns = Vec::new();
        if self.focal {
            columns.push(0);
        }
        if self.distortion {
            columns.extend([1, 2]);
        }
        columns
    }
}

struct Linear {
    r: Vector2<f64>,
    /// d r / d (rotation, translation), the rotation as a left increment.
    jc: SMatrix<f64, 2, 6>,
    /// d r / d (focal length, k1, k2).
    jl: SMatrix<f64, 2, 3>,
    jp: SMatrix<f64, 2, 3>,
}

fn skew(v: &Vector3<f64>) -> Matrix3<f64> {
    Matrix3::new(0.0, -v.z, v.y, v.z, 0.0, -v.x, -v.y, v.x, 0.0)
}

fn linearize(cam: &Cam, intr: &Intrinsics, x: &Vector3<f64>, uv: &Vector2<f64>) -> Option<Linear> {
    let rx = cam.r * x;
    let c = rx + cam.t;
    if c.z <= 1e-9 {
        return None;
    }
    let iz = 1.0 / c.z;
    let n = Vector2::new(c.x * iz, c.y * iz);
    let (f, s) = (intr.f, n.norm_squared());
    let radial = intr.radial(s);
    let r = f * radial * n + Vector2::new(intr.cx - uv.x, intr.cy - uv.y);
    // The distortion's own Jacobian on the z = 1 plane, then the projection's.
    let warp = Matrix2::identity() * radial + 2.0 * (intr.k1 + 2.0 * intr.k2 * s) * n * n.transpose();
    let dproj = f * warp * Matrix2x3::new(iz, 0.0, -n.x * iz, 0.0, iz, -n.y * iz);
    let jw = dproj * (-skew(&rx));
    let mut jc = SMatrix::<f64, 2, 6>::zeros();
    jc.fixed_view_mut::<2, 3>(0, 0).copy_from(&jw);
    jc.fixed_view_mut::<2, 3>(0, 3).copy_from(&dproj);
    let mut jl = SMatrix::<f64, 2, 3>::zeros();
    jl.set_column(0, &(radial * n));
    jl.set_column(1, &(f * s * n));
    jl.set_column(2, &(f * s * s * n));
    Some(Linear { r, jc, jl, jp: dproj * cam.r })
}

fn weight(norm: f64) -> f64 {
    if norm <= HUBER {
        1.0
    } else {
        HUBER / norm
    }
}

fn loss(norm: f64) -> f64 {
    if norm <= HUBER {
        0.5 * norm * norm
    } else {
        HUBER * (norm - 0.5 * HUBER)
    }
}

fn nudge(cam: &mut Cam, d: &[f64]) {
    let w = Vector3::new(d[0], d[1], d[2]);
    cam.r = Rotation3::new(w).matrix() * cam.r;
    cam.t += Vector3::new(d[3], d[4], d[5]);
}

/// The lens camera `c` sees through: `intr`, at its own focal length when
/// `focals` holds one per camera.
pub fn lens_of(intr: &Intrinsics, focals: Option<&[f64]>, c: usize) -> Intrinsics {
    match focals {
        Some(f) => Intrinsics { f: f[c], ..*intr },
        None => *intr,
    }
}

fn cost(cams: &[Cam], focals: Option<&[f64]>, points: &[Vector3<f64>], obs: &[Obs], intr: &Intrinsics) -> f64 {
    obs.iter()
        .map(|o| match lens_of(intr, focals, o.cam).project(&cams[o.cam], &points[o.point]) {
            Some(p) => loss((p - o.uv).norm()),
            None => loss(1e3),
        })
        .sum()
}

/// The root mean square reprojection error of `obs` in pixels, each camera
/// at its own focal length when `focals` holds one per camera.
pub fn rms(cams: &[Cam], focals: Option<&[f64]>, points: &[Vector3<f64>], obs: &[Obs], intr: &Intrinsics) -> f64 {
    let sum: f64 = obs
        .iter()
        .map(|o| lens_of(intr, focals, o.cam).project(&cams[o.cam], &points[o.point]).map_or(1e6, |p| (p - o.uv).norm_squared()))
        .sum();
    (sum / obs.len().max(1) as f64).sqrt()
}

/// Refines every camera not marked `fixed`, every point and the shared lens
/// parameters `refine` names, for up to `iterations` steps.
pub fn adjust(
    cams: &mut [Cam],
    points: &mut [Vector3<f64>],
    obs: &[Obs],
    fixed: &[bool],
    intr: &mut Intrinsics,
    refine: Refine,
    iterations: usize,
) {
    adjust_blocks::<6>(cams, None, points, obs, fixed, intr, refine, iterations);
}

/// `adjust` for a zooming lens: every camera has its own focal length in
/// `focals`, refined with each camera not marked `fixed`. `refine.focal`
/// scales every focal length together, so a fixed camera's lens refines
/// while its pose holds.
#[allow(clippy::too_many_arguments)]
pub fn adjust_zoom(
    cams: &mut [Cam],
    focals: &mut [f64],
    points: &mut [Vector3<f64>],
    obs: &[Obs],
    fixed: &[bool],
    intr: &mut Intrinsics,
    refine: Refine,
    iterations: usize,
) {
    adjust_blocks::<7>(cams, Some(focals), points, obs, fixed, intr, refine, iterations);
}

/// The Jacobian of a residual by a camera block of `C` parameters: the
/// rotation and translation, then with 7 the camera's focal length.
fn camera_jacobian<const C: usize>(l: &Linear) -> SMatrix<f64, 2, C> {
    let mut jc = SMatrix::<f64, 2, C>::zeros();
    jc.fixed_view_mut::<2, 6>(0, 0).copy_from(&l.jc);
    if C == 7 {
        jc.set_column(6, &l.jl.column(0));
    }
    jc
}

/// Moves a camera by a block step: rotation and translation, then with a
/// seventh parameter its focal length.
fn nudge_block(cam: &mut Cam, focal: Option<&mut f64>, d: &[f64]) {
    nudge(cam, d);
    if let Some(f) = focal {
        *f += d[6];
    }
}

/// The bundle adjustment with camera blocks of `C` parameters: 6 for a lens
/// every camera shares, 7 with each camera's focal length in `focals`.
#[allow(clippy::too_many_arguments)]
fn adjust_blocks<const C: usize>(
    cams: &mut [Cam],
    mut focals: Option<&mut [f64]>,
    points: &mut [Vector3<f64>],
    obs: &[Obs],
    fixed: &[bool],
    intr: &mut Intrinsics,
    refine: Refine,
    iterations: usize,
) {
    let lens = refine.columns();
    let nl = lens.len();
    let mut offset = vec![None; cams.len()];
    let mut np = nl;
    for (c, slot) in offset.iter_mut().enumerate() {
        if !fixed[c] {
            *slot = Some(np);
            np += C;
        }
    }
    let mut by_point = vec![Vec::new(); points.len()];
    for (i, o) in obs.iter().enumerate() {
        by_point[o.point].push(i);
    }
    let mut current = cost(cams, focals.as_deref(), points, obs, intr);
    let mut lambda = 1e-4;
    for _ in 0..iterations {
        let mut u = DMatrix::<f64>::zeros(np, np);
        let mut bc = DVector::<f64>::zeros(np);
        let mut v = vec![Matrix3::<f64>::zeros(); points.len()];
        let mut bp = vec![Vector3::<f64>::zeros(); points.len()];
        // Per point: the camera-point blocks of its moving cameras at their
        // offsets, and its lens rows.
        let mut w: Vec<Vec<(usize, SMatrix<f64, C, 3>)>> = vec![Vec::new(); points.len()];
        let mut wl = vec![[Vector3::<f64>::zeros(); 3]; points.len()];
        for p in 0..points.len() {
            for &i in &by_point[p] {
                let o = &obs[i];
                let Some(mut l) = linearize(&cams[o.cam], &lens_of(intr, focals.as_deref(), o.cam), &points[p], &o.uv) else { continue };
                let jc = camera_jacobian::<C>(&l);
                if let Some(f) = focals.as_deref() {
                    // With a focal length per camera the lens column scales
                    // them all: d r / d log scale.
                    let scaled = l.jl.column(0) * f[o.cam];
                    l.jl.set_column(0, &scaled);
                }
                let wt = weight(l.r.norm());
                v[p] += wt * l.jp.transpose() * l.jp;
                bp[p] -= wt * l.jp.transpose() * l.r;
                if let Some(off) = offset[o.cam] {
                    let jtj: SMatrix<f64, C, C> = wt * jc.transpose() * jc;
                    let g: SMatrix<f64, C, 1> = wt * jc.transpose() * l.r;
                    let wcp: SMatrix<f64, C, 3> = wt * jc.transpose() * l.jp;
                    for a in 0..C {
                        bc[off + a] -= g[a];
                        for b in 0..C {
                            u[(off + a, off + b)] += jtj[(a, b)];
                        }
                    }
                    w[p].push((off, wcp));
                    for (i, &col) in lens.iter().enumerate() {
                        let cross: SMatrix<f64, C, 1> = wt * jc.transpose() * l.jl.column(col);
                        for a in 0..C {
                            u[(off + a, i)] += cross[a];
                            u[(i, off + a)] += cross[a];
                        }
                    }
                }
                for (i, &ci) in lens.iter().enumerate() {
                    let column = l.jl.column(ci);
                    bc[i] -= wt * column.dot(&l.r);
                    for (j, &cj) in lens.iter().enumerate() {
                        u[(i, j)] += wt * column.dot(&l.jl.column(cj));
                    }
                    wl[p][i] += wt * l.jp.transpose() * column;
                }
            }
        }
        // The blocks each moving camera's row of the reduced system gathers.
        let mut by_cam = vec![Vec::new(); (np - nl) / C];
        for (p, blocks) in w.iter().enumerate() {
            for (j, &(off, _)) in blocks.iter().enumerate() {
                by_cam[(off - nl) / C].push((p, j));
            }
        }
        let improved = loop {
            let vinv: Vec<Matrix3<f64>> = v
                .par_iter()
                .map(|vp| {
                    let mut damped = *vp;
                    for k in 0..3 {
                        damped[(k, k)] += lambda * vp[(k, k)] + 1e-12;
                    }
                    damped.try_inverse().unwrap_or_else(Matrix3::zeros)
                })
                .collect();
            let (s, rhs) = reduce(&u, &bc, lambda, nl, (&w, &wl, &by_cam), (&vinv, &bp));
            let step = if np == 0 { Some(DVector::zeros(0)) } else { solve_spd(s, np, rhs.as_slice()).map(DVector::from_vec) };
            let Some(dc) = step else {
                lambda *= 10.0;
                if lambda > 1e10 {
                    break false;
                }
                continue;
            };
            let saved = (cams.to_vec(), points.to_vec(), *intr, focals.as_deref().map(<[f64]>::to_vec));
            for (c, cam) in cams.iter_mut().enumerate() {
                if let Some(off) = offset[c] {
                    nudge_block(cam, focals.as_deref_mut().map(|f| &mut f[c]), &dc.as_slice()[off..off + C]);
                }
            }
            for (i, &col) in lens.iter().enumerate() {
                match col {
                    0 => match focals.as_deref_mut() {
                        Some(focals) => focals.iter_mut().for_each(|f| *f *= dc[i].exp()),
                        None => intr.f += dc[i],
                    },
                    1 => intr.k1 += dc[i],
                    _ => intr.k2 += dc[i],
                }
            }
            for p in 0..points.len() {
                let mut g = bp[p];
                for &(off, wb) in &w[p] {
                    g -= wb.transpose() * dc.fixed_rows::<C>(off);
                }
                for (i, row) in wl[p].iter().enumerate().take(nl) {
                    g -= row * dc[i];
                }
                points[p] += vinv[p] * g;
            }
            let next = cost(cams, focals.as_deref(), points, obs, intr);
            if next < current {
                let gain = (current - next) / current.max(1e-30);
                current = next;
                lambda = (lambda / 3.0).max(1e-9);
                break gain > 1e-7;
            }
            cams.copy_from_slice(&saved.0);
            points.copy_from_slice(&saved.1);
            *intr = saved.2;
            if let (Some(f), Some(old)) = (focals.as_deref_mut(), saved.3) {
                f.copy_from_slice(&old);
            }
            lambda *= 4.0;
            if lambda > 1e10 {
                break false;
            }
        };
        if !improved {
            break;
        }
    }
}

/// The camera system with the points eliminated: `u` and `bc` damped by
/// `lambda` less each point's blocks through its damped inverse `vinv`. Each
/// moving camera's `C`-row strip gathers on its own thread; `u` is
/// symmetric, so its column-major storage reads as row-major.
#[allow(clippy::type_complexity)]
fn reduce<const C: usize>(
    u: &DMatrix<f64>,
    bc: &DVector<f64>,
    lambda: f64,
    nl: usize,
    (w, wl, by_cam): (&[Vec<(usize, SMatrix<f64, C, 3>)>], &[[Vector3<f64>; 3]], &[Vec<(usize, usize)>]),
    (vinv, bp): (&[Matrix3<f64>], &[Vector3<f64>]),
) -> (Vec<f64>, DVector<f64>) {
    let np = u.nrows();
    let mut buf = u.as_slice().to_vec();
    for i in 0..np {
        buf[i * np + i] += lambda * u[(i, i)] + 1e-12;
    }
    let mut rhs = bc.clone();
    let (lens_rows, cam_rows) = buf.split_at_mut(nl * np);
    let (lens_rhs, cam_rhs) = rhs.as_mut_slice().split_at_mut(nl);
    cam_rows.par_chunks_mut(C * np).zip(cam_rhs.par_chunks_mut(C)).zip(by_cam.par_iter()).for_each(|((strip, r), seen)| {
        for &(p, j) in seen {
            let y: SMatrix<f64, C, 3> = w[p][j].1 * vinv[p];
            let g = y * bp[p];
            for a in 0..C {
                r[a] -= g[a];
            }
            for &(off, wb) in &w[p] {
                let block: SMatrix<f64, C, C> = y * wb.transpose();
                for a in 0..C {
                    let row = &mut strip[a * np + off..a * np + off + C];
                    for (b, v) in row.iter_mut().enumerate() {
                        *v -= block[(a, b)];
                    }
                }
            }
            for (i, l) in wl[p].iter().enumerate().take(nl) {
                let cross = y * l;
                for a in 0..C {
                    strip[a * np + i] -= cross[a];
                }
            }
        }
    });
    for p in 0..w.len() {
        for i in 0..nl {
            let y = vinv[p] * wl[p][i];
            lens_rhs[i] -= y.dot(&bp[p]);
            let row = &mut lens_rows[i * np..(i + 1) * np];
            for &(off, wb) in &w[p] {
                let cross = wb * y;
                for a in 0..C {
                    row[off + a] -= cross[a];
                }
            }
            for j in 0..nl {
                row[j] -= y.dot(&wl[p][j]);
            }
        }
    }
    (buf, rhs)
}

/// The rows a block of the factorization covers.
const BLOCK: usize = 64;

/// Solves `a x = b` for the symmetric positive definite `a`, stored
/// row-major `n` by `n`, by a right-looking blocked Cholesky factorization
/// whose panel solves and trailing updates run in parallel, or `None` when
/// `a` is not positive definite. It reads the lower triangle alone.
fn solve_spd(mut a: Vec<f64>, n: usize, b: &[f64]) -> Option<Vec<f64>> {
    let mut k0 = 0;
    while k0 < n {
        let k1 = (k0 + BLOCK).min(n);
        let w = k1 - k0;
        for j in k0..k1 {
            let d = a[j * n + j] - dot(&a[j * n + k0..j * n + j], &a[j * n + k0..j * n + j]);
            if d.is_nan() || d <= 0.0 {
                return None;
            }
            let d = d.sqrt();
            a[j * n + j] = d;
            for i in j + 1..k1 {
                let s = a[i * n + j] - dot(&a[i * n + k0..i * n + j], &a[j * n + k0..j * n + j]);
                a[i * n + j] = s / d;
            }
        }
        let mut diag = vec![0.0; w * w];
        for j in 0..w {
            diag[j * w..j * w + j + 1].copy_from_slice(&a[(k0 + j) * n + k0..(k0 + j) * n + k0 + j + 1]);
        }
        let below = &mut a[k1 * n..];
        below.par_chunks_mut(n).for_each(|row| {
            for j in 0..w {
                let s = row[k0 + j] - dot(&row[k0..k0 + j], &diag[j * w..j * w + j]);
                row[k0 + j] = s / diag[j * w + j];
            }
        });
        let mut panel = vec![0.0; (n - k1) * w];
        for (r, chunk) in panel.chunks_mut(w).enumerate() {
            chunk.copy_from_slice(&below[r * n + k0..r * n + k1]);
        }
        below.par_chunks_mut(n).enumerate().for_each(|(r, row)| {
            let lr = &panel[r * w..(r + 1) * w];
            for (c, lc) in panel.chunks(w).take(r + 1).enumerate() {
                row[k1 + c] -= dot(lr, lc);
            }
        });
        k0 = k1;
    }
    let mut y = b.to_vec();
    for i in 0..n {
        y[i] = (y[i] - dot(&a[i * n..i * n + i], &y[..i])) / a[i * n + i];
    }
    for i in (0..n).rev() {
        y[i] /= a[i * n + i];
        let xi = y[i];
        for (yp, l) in y[..i].iter_mut().zip(&a[i * n..i * n + i]) {
            *yp -= l * xi;
        }
    }
    Some(y)
}

/// The dot product with eight running sums, so it vectorizes.
fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut acc = [0.0f64; 8];
    let ((ca, ra), (cb, rb)) = (a.as_chunks::<8>(), b[..a.len()].as_chunks::<8>());
    let tail: f64 = ra.iter().zip(rb).map(|(x, y)| x * y).sum();
    for (x, y) in ca.iter().zip(cb) {
        for (s, (x, y)) in acc.iter_mut().zip(x.iter().zip(y)) {
            *s += x * y;
        }
    }
    acc.iter().sum::<f64>() + tail
}

/// Refines one camera against fixed points seen at `uvs` with Gauss-Newton
/// under the Huber loss, and returns each observation's error in pixels.
pub fn resect(cam: &mut Cam, points: &[Vector3<f64>], uvs: &[Vector2<f64>], intr: &Intrinsics, iterations: usize) -> Vec<f64> {
    let mut focal = intr.f;
    resect_blocks::<6>(cam, &mut focal, points, uvs, intr, iterations)
}

/// `resect` for a zooming lens: the camera's own focal length `focal`
/// refines with its pose.
pub fn resect_zoom(cam: &mut Cam, focal: &mut f64, points: &[Vector3<f64>], uvs: &[Vector2<f64>], intr: &Intrinsics, iterations: usize) -> Vec<f64> {
    resect_blocks::<7>(cam, focal, points, uvs, intr, iterations)
}

fn resect_blocks<const C: usize>(cam: &mut Cam, focal: &mut f64, points: &[Vector3<f64>], uvs: &[Vector2<f64>], intr: &Intrinsics, iterations: usize) -> Vec<f64> {
    let at = |f: f64| Intrinsics { f, ..*intr };
    let total = |cam: &Cam, f: f64| -> f64 {
        let lens = at(f);
        points
            .iter()
            .zip(uvs)
            .map(|(x, uv)| lens.project(cam, x).map_or(loss(1e3), |p| loss((p - uv).norm())))
            .sum()
    };
    let mut current = total(cam, *focal);
    let mut lambda = 1e-4;
    for _ in 0..iterations {
        let mut h = SMatrix::<f64, C, C>::zeros();
        let mut g = SMatrix::<f64, C, 1>::zeros();
        let lens = at(*focal);
        for (x, uv) in points.iter().zip(uvs) {
            let Some(l) = linearize(cam, &lens, x, uv) else { continue };
            let wt = weight(l.r.norm());
            let jc = camera_jacobian::<C>(&l);
            h += wt * jc.transpose() * jc;
            g -= wt * jc.transpose() * l.r;
        }
        let mut stepped = false;
        while lambda < 1e10 {
            let mut damped = h;
            for k in 0..C {
                damped[(k, k)] += lambda * h[(k, k)] + 1e-12;
            }
            let Some(d) = damped.cholesky().map(|ch| ch.solve(&g)) else {
                lambda *= 10.0;
                continue;
            };
            let (mut next, mut f) = (*cam, *focal);
            nudge_block(&mut next, (C == 7).then_some(&mut f), d.as_slice());
            let c = total(&next, f);
            if c < current {
                let gain = (current - c) / current.max(1e-30);
                *cam = next;
                *focal = f;
                current = c;
                lambda = (lambda / 3.0).max(1e-9);
                stepped = gain > 1e-9;
                break;
            }
            lambda *= 4.0;
        }
        if !stepped {
            break;
        }
    }
    points
        .iter()
        .zip(uvs)
        .map(|(x, uv)| at(*focal).project(cam, x).map_or(f64::INFINITY, |p| (p - uv).norm()))
        .collect()
}
