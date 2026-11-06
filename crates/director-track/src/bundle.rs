//! Levenberg-Marquardt bundle adjustment with a Schur complement on the
//! points, and a single camera's resection against fixed points. Residuals
//! are in pixels under a Huber loss.

use nalgebra::{DMatrix, DVector, Matrix2, Matrix2x3, Matrix3, Matrix6, Rotation3, SMatrix, Vector2, Vector3, Vector6};

use crate::geometry::{Cam, Intrinsics};

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

fn cost(cams: &[Cam], points: &[Vector3<f64>], obs: &[Obs], intr: &Intrinsics) -> f64 {
    obs.iter()
        .map(|o| match intr.project(&cams[o.cam], &points[o.point]) {
            Some(p) => loss((p - o.uv).norm()),
            None => loss(1e3),
        })
        .sum()
}

/// The root mean square reprojection error of `obs` in pixels.
pub fn rms(cams: &[Cam], points: &[Vector3<f64>], obs: &[Obs], intr: &Intrinsics) -> f64 {
    let sum: f64 = obs
        .iter()
        .map(|o| intr.project(&cams[o.cam], &points[o.point]).map_or(1e6, |p| (p - o.uv).norm_squared()))
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
    let lens = refine.columns();
    let nl = lens.len();
    let mut offset = vec![None; cams.len()];
    let mut np = nl;
    for (c, slot) in offset.iter_mut().enumerate() {
        if !fixed[c] {
            *slot = Some(np);
            np += 6;
        }
    }
    let mut by_point = vec![Vec::new(); points.len()];
    for (i, o) in obs.iter().enumerate() {
        by_point[o.point].push(i);
    }
    let mut current = cost(cams, points, obs, intr);
    let mut lambda = 1e-4;
    for _ in 0..iterations {
        let mut u = DMatrix::<f64>::zeros(np, np);
        let mut bc = DVector::<f64>::zeros(np);
        let mut v = vec![Matrix3::<f64>::zeros(); points.len()];
        let mut bp = vec![Vector3::<f64>::zeros(); points.len()];
        let mut w: Vec<Vec<(usize, Vector3<f64>)>> = vec![Vec::new(); points.len()];
        for p in 0..points.len() {
            let mut rows = Vec::new();
            let mut lens_rows = vec![Vector3::zeros(); nl];
            for &i in &by_point[p] {
                let o = &obs[i];
                let Some(l) = linearize(&cams[o.cam], intr, &points[p], &o.uv) else { continue };
                let wt = weight(l.r.norm());
                v[p] += wt * l.jp.transpose() * l.jp;
                bp[p] -= wt * l.jp.transpose() * l.r;
                if let Some(off) = offset[o.cam] {
                    let jtj: Matrix6<f64> = wt * l.jc.transpose() * l.jc;
                    let g: Vector6<f64> = wt * l.jc.transpose() * l.r;
                    let wcp: SMatrix<f64, 6, 3> = wt * l.jc.transpose() * l.jp;
                    for a in 0..6 {
                        bc[off + a] -= g[a];
                        for b in 0..6 {
                            u[(off + a, off + b)] += jtj[(a, b)];
                        }
                        rows.push((off + a, wcp.row(a).transpose()));
                    }
                    for (i, &col) in lens.iter().enumerate() {
                        let cross: Vector6<f64> = wt * l.jc.transpose() * l.jl.column(col);
                        for a in 0..6 {
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
                    lens_rows[i] += wt * l.jp.transpose() * column;
                }
            }
            rows.extend(lens_rows.into_iter().enumerate());
            w[p] = rows;
        }
        let improved = loop {
            let mut s = u.clone();
            for i in 0..np {
                s[(i, i)] += lambda * u[(i, i)] + 1e-12;
            }
            let mut rhs = bc.clone();
            let mut vinv = Vec::with_capacity(points.len());
            for p in 0..points.len() {
                let mut vp = v[p];
                for k in 0..3 {
                    vp[(k, k)] += lambda * v[p][(k, k)] + 1e-12;
                }
                let inv = vp.try_inverse().unwrap_or_else(Matrix3::zeros);
                for &(a, wa) in &w[p] {
                    let ya = inv * wa;
                    rhs[a] -= ya.dot(&bp[p]);
                    for &(b, wb) in &w[p] {
                        s[(a, b)] -= ya.dot(&wb);
                    }
                }
                vinv.push(inv);
            }
            let step = if np == 0 { Some(DVector::zeros(0)) } else { s.cholesky().map(|ch| ch.solve(&rhs)) };
            let Some(dc) = step else {
                lambda *= 10.0;
                if lambda > 1e10 {
                    break false;
                }
                continue;
            };
            let saved = (cams.to_vec(), points.to_vec(), *intr);
            for (c, cam) in cams.iter_mut().enumerate() {
                if let Some(off) = offset[c] {
                    nudge(cam, &dc.as_slice()[off..off + 6]);
                }
            }
            for (i, &col) in lens.iter().enumerate() {
                match col {
                    0 => intr.f += dc[i],
                    1 => intr.k1 += dc[i],
                    _ => intr.k2 += dc[i],
                }
            }
            for p in 0..points.len() {
                let mut g = bp[p];
                for &(a, wa) in &w[p] {
                    g -= wa * dc[a];
                }
                points[p] += vinv[p] * g;
            }
            let next = cost(cams, points, obs, intr);
            if next < current {
                let gain = (current - next) / current.max(1e-30);
                current = next;
                lambda = (lambda / 3.0).max(1e-9);
                break gain > 1e-7;
            }
            cams.copy_from_slice(&saved.0);
            points.copy_from_slice(&saved.1);
            *intr = saved.2;
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

/// Refines one camera against fixed points seen at `uvs` with Gauss-Newton
/// under the Huber loss, and returns each observation's error in pixels.
pub fn resect(cam: &mut Cam, points: &[Vector3<f64>], uvs: &[Vector2<f64>], intr: &Intrinsics, iterations: usize) -> Vec<f64> {
    let total = |cam: &Cam| -> f64 {
        points
            .iter()
            .zip(uvs)
            .map(|(x, uv)| intr.project(cam, x).map_or(loss(1e3), |p| loss((p - uv).norm())))
            .sum()
    };
    let mut current = total(cam);
    let mut lambda = 1e-4;
    for _ in 0..iterations {
        let mut h = Matrix6::<f64>::zeros();
        let mut g = Vector6::<f64>::zeros();
        for (x, uv) in points.iter().zip(uvs) {
            let Some(l) = linearize(cam, intr, x, uv) else { continue };
            let wt = weight(l.r.norm());
            h += wt * l.jc.transpose() * l.jc;
            g -= wt * l.jc.transpose() * l.r;
        }
        let mut stepped = false;
        while lambda < 1e10 {
            let mut damped = h;
            for k in 0..6 {
                damped[(k, k)] += lambda * h[(k, k)] + 1e-12;
            }
            let Some(d) = damped.cholesky().map(|ch| ch.solve(&g)) else {
                lambda *= 10.0;
                continue;
            };
            let mut next = *cam;
            nudge(&mut next, d.as_slice());
            let c = total(&next);
            if c < current {
                let gain = (current - c) / current.max(1e-30);
                *cam = next;
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
        .map(|(x, uv)| intr.project(cam, x).map_or(f64::INFINITY, |p| (p - uv).norm()))
        .collect()
}
