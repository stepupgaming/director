//! Two-view geometry: cameras, the essential matrix, pose recovery and
//! triangulation, in camera coordinates with x right, y down and z ahead.

use nalgebra::{DMatrix, Matrix3, SMatrix, SVector, SymmetricEigen, Vector2, Vector3, SVD};

/// A camera's pose: a world point x lands at r x + t in camera coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cam {
    pub r: Matrix3<f64>,
    pub t: Vector3<f64>,
}

impl Cam {
    pub fn identity() -> Self {
        Self { r: Matrix3::identity(), t: Vector3::zeros() }
    }

    /// The camera's centre in world coordinates.
    pub fn centre(&self) -> Vector3<f64> {
        -self.r.transpose() * self.t
    }
}

/// A lens with square pixels: focal length and principal point in pixels,
/// and radial distortion. A point (x, y) on the z = 1 plane lands at
/// (x, y) (1 + k1 r^2 + k2 r^4) with r^2 = x^2 + y^2 before the focal length
/// scales it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Intrinsics {
    pub f: f64,
    pub cx: f64,
    pub cy: f64,
    pub k1: f64,
    pub k2: f64,
}

impl Intrinsics {
    /// The distortion factor at squared radius `r2` on the z = 1 plane.
    pub fn radial(&self, r2: f64) -> f64 {
        1.0 + self.k1 * r2 + self.k2 * r2 * r2
    }

    /// Where `x` lands in the image, or `None` behind the camera.
    pub fn project(&self, cam: &Cam, x: &Vector3<f64>) -> Option<Vector2<f64>> {
        let c = cam.r * x + cam.t;
        if c.z <= 1e-9 {
            return None;
        }
        let (a, b) = (c.x / c.z, c.y / c.z);
        let s = self.f * self.radial(a * a + b * b);
        Some(Vector2::new(s * a + self.cx, s * b + self.cy))
    }

    /// A pixel as a point on the z = 1 plane, with the distortion undone.
    pub fn normalize(&self, uv: [f32; 2]) -> Vector2<f64> {
        let d = Vector2::new((uv[0] as f64 - self.cx) / self.f, (uv[1] as f64 - self.cy) / self.f);
        if self.k1 == 0.0 && self.k2 == 0.0 {
            return d;
        }
        let mut x = d;
        for _ in 0..20 {
            x = d / self.radial(x.norm_squared());
        }
        x
    }
}

/// A deterministic xorshift generator for RANSAC samples.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

fn smallest_eigenvector<const N: usize>(m: SMatrix<f64, N, N>) -> SVector<f64, N> {
    let eigen = SymmetricEigen::new(DMatrix::from_column_slice(N, N, m.as_slice()));
    let i = eigen.eigenvalues.imin();
    SVector::from_column_slice(eigen.eigenvectors.column(i).as_slice())
}

/// Hartley's conditioning: centroid to the origin, mean distance sqrt(2).
fn condition(points: &[Vector2<f64>]) -> Matrix3<f64> {
    let n = points.len() as f64;
    let c = points.iter().sum::<Vector2<f64>>() / n;
    let mean = points.iter().map(|p| (p - c).norm()).sum::<f64>() / n;
    let s = if mean > 1e-12 { std::f64::consts::SQRT_2 / mean } else { 1.0 };
    Matrix3::new(s, 0.0, -s * c.x, 0.0, s, -s * c.y, 0.0, 0.0, 1.0)
}

/// The essential matrix with x2' E x1 = 0 from 8 or more normalized pairs,
/// projected to two equal singular values and a zero one.
pub fn eight_point(x1: &[Vector2<f64>], x2: &[Vector2<f64>]) -> Option<Matrix3<f64>> {
    if x1.len() < 8 {
        return None;
    }
    let (t1, t2) = (condition(x1), condition(x2));
    let mut ata = SMatrix::<f64, 9, 9>::zeros();
    for (a, b) in x1.iter().zip(x2) {
        let p = t1 * a.push(1.0);
        let q = t2 * b.push(1.0);
        let row = SVector::<f64, 9>::from([q.x * p.x, q.x * p.y, q.x * p.z, q.y * p.x, q.y * p.y, q.y * p.z, q.z * p.x, q.z * p.y, q.z * p.z]);
        ata += row * row.transpose();
    }
    let e = smallest_eigenvector(ata);
    let conditioned = Matrix3::new(e[0], e[1], e[2], e[3], e[4], e[5], e[6], e[7], e[8]);
    let e = t2.transpose() * conditioned * t1;
    let svd = SVD::new(e, true, true);
    let (u, v_t) = (svd.u?, svd.v_t?);
    let e = u * Matrix3::from_diagonal(&Vector3::new(1.0, 1.0, 0.0)) * v_t;
    e.iter().all(|v| v.is_finite()).then_some(e)
}

/// The Sampson distance of a normalized pair from the epipolar constraint, squared.
pub fn sampson(e: &Matrix3<f64>, a: &Vector2<f64>, b: &Vector2<f64>) -> f64 {
    let (p, q) = (a.push(1.0), b.push(1.0));
    let ep = e * p;
    let etq = e.transpose() * q;
    let num = q.dot(&ep);
    num * num / (ep.x * ep.x + ep.y * ep.y + etq.x * etq.x + etq.y * etq.y).max(1e-30)
}

/// RANSAC over 8-point samples. `threshold` is the Sampson distance limit
/// in normalized units. Returns the matrix refit on its inliers and the
/// inlier mask.
pub fn essential_ransac(
    x1: &[Vector2<f64>],
    x2: &[Vector2<f64>],
    threshold: f64,
    iterations: usize,
    rng: &mut Rng,
) -> Option<(Matrix3<f64>, Vec<bool>)> {
    let n = x1.len();
    if n < 8 {
        return None;
    }
    let limit = threshold * threshold;
    let mask = |e: &Matrix3<f64>| -> Vec<bool> { (0..n).map(|i| sampson(e, &x1[i], &x2[i]) < limit).collect() };
    let mut best: Option<(Matrix3<f64>, usize)> = None;
    for _ in 0..iterations {
        let mut pick = Vec::with_capacity(8);
        while pick.len() < 8 {
            let i = rng.below(n);
            if !pick.contains(&i) {
                pick.push(i);
            }
        }
        let a: Vec<_> = pick.iter().map(|&i| x1[i]).collect();
        let b: Vec<_> = pick.iter().map(|&i| x2[i]).collect();
        let Some(e) = eight_point(&a, &b) else { continue };
        let count = mask(&e).iter().filter(|&&m| m).count();
        if best.as_ref().is_none_or(|(_, c)| count > *c) {
            best = Some((e, count));
        }
    }
    let (mut e, _) = best?;
    for _ in 0..3 {
        let inliers = mask(&e);
        let a: Vec<_> = (0..n).filter(|&i| inliers[i]).map(|i| x1[i]).collect();
        let b: Vec<_> = (0..n).filter(|&i| inliers[i]).map(|i| x2[i]).collect();
        match eight_point(&a, &b) {
            Some(refit) => e = refit,
            None => break,
        }
    }
    let inliers = mask(&e);
    Some((e, inliers))
}

/// The four (R, t) a normalized essential matrix factors into.
pub fn factor_essential(e: &Matrix3<f64>) -> Vec<Cam> {
    let svd = SVD::new(*e, true, true);
    let (Some(mut u), Some(mut v_t)) = (svd.u, svd.v_t) else { return Vec::new() };
    if u.determinant() < 0.0 {
        u = -u;
    }
    if v_t.determinant() < 0.0 {
        v_t = -v_t;
    }
    let w = Matrix3::new(0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0);
    let (r1, r2) = (u * w * v_t, u * w.transpose() * v_t);
    let t: Vector3<f64> = u.column(2).into_owned();
    vec![Cam { r: r1, t }, Cam { r: r1, t: -t }, Cam { r: r2, t }, Cam { r: r2, t: -t }]
}

/// The homography with x2 ~ H x1 from 4 or more normalized pairs.
pub fn four_point(x1: &[Vector2<f64>], x2: &[Vector2<f64>]) -> Option<Matrix3<f64>> {
    if x1.len() < 4 {
        return None;
    }
    let (t1, t2) = (condition(x1), condition(x2));
    let mut ata = SMatrix::<f64, 9, 9>::zeros();
    for (a, b) in x1.iter().zip(x2) {
        let p = t1 * a.push(1.0);
        let q = t2 * b.push(1.0);
        let r1 = SVector::<f64, 9>::from([0.0, 0.0, 0.0, -q.z * p.x, -q.z * p.y, -q.z * p.z, q.y * p.x, q.y * p.y, q.y * p.z]);
        let r2 = SVector::<f64, 9>::from([q.z * p.x, q.z * p.y, q.z * p.z, 0.0, 0.0, 0.0, -q.x * p.x, -q.x * p.y, -q.x * p.z]);
        ata += r1 * r1.transpose() + r2 * r2.transpose();
    }
    let h = smallest_eigenvector(ata);
    let conditioned = Matrix3::new(h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], h[8]);
    let h = t2.try_inverse()? * conditioned * t1;
    h.iter().all(|v| v.is_finite()).then_some(h)
}

/// The squared transfer error of a pair under `h`, both ways.
fn transfer(h: &Matrix3<f64>, hinv: &Matrix3<f64>, a: &Vector2<f64>, b: &Vector2<f64>) -> f64 {
    let fwd = h * a.push(1.0);
    let back = hinv * b.push(1.0);
    if fwd.z.abs() < 1e-12 || back.z.abs() < 1e-12 {
        return f64::INFINITY;
    }
    (fwd.xy() / fwd.z - b).norm_squared() + (back.xy() / back.z - a).norm_squared()
}

/// RANSAC over 4-point homographies with a transfer error limit in
/// normalized units, refit on its inliers.
pub fn homography_ransac(x1: &[Vector2<f64>], x2: &[Vector2<f64>], threshold: f64, iterations: usize, rng: &mut Rng) -> Option<Matrix3<f64>> {
    let n = x1.len();
    if n < 4 {
        return None;
    }
    let limit = 2.0 * threshold * threshold;
    let mask = |h: &Matrix3<f64>| -> Option<Vec<bool>> {
        let hinv = h.try_inverse()?;
        Some((0..n).map(|i| transfer(h, &hinv, &x1[i], &x2[i]) < limit).collect())
    };
    let mut best: Option<(Matrix3<f64>, usize)> = None;
    for _ in 0..iterations {
        let mut pick = Vec::with_capacity(4);
        while pick.len() < 4 {
            let i = rng.below(n);
            if !pick.contains(&i) {
                pick.push(i);
            }
        }
        let a: Vec<_> = pick.iter().map(|&i| x1[i]).collect();
        let b: Vec<_> = pick.iter().map(|&i| x2[i]).collect();
        let Some(h) = four_point(&a, &b) else { continue };
        let Some(m) = mask(&h) else { continue };
        let count = m.iter().filter(|&&m| m).count();
        if best.as_ref().is_none_or(|(_, c)| count > *c) {
            best = Some((h, count));
        }
    }
    let (mut h, _) = best?;
    for _ in 0..3 {
        let Some(inliers) = mask(&h) else { break };
        let a: Vec<_> = (0..n).filter(|&i| inliers[i]).map(|i| x1[i]).collect();
        let b: Vec<_> = (0..n).filter(|&i| inliers[i]).map(|i| x2[i]).collect();
        match four_point(&a, &b) {
            Some(refit) => h = refit,
            None => break,
        }
    }
    Some(h)
}

/// The eight (R, t) a calibrated homography H = R + t n' / d factors into,
/// by Faugeras' singular value decomposition.
pub fn factor_homography(h: &Matrix3<f64>) -> Vec<Cam> {
    let svd = SVD::new(*h, true, true);
    let (Some(u), Some(v_t)) = (svd.u, svd.v_t) else { return Vec::new() };
    let s = u.determinant() * v_t.determinant();
    let d = svd.singular_values;
    let (d1, d2, d3) = (d[0], d[1], d[2]);
    if d1 / d2 < 1.00001 || d2 / d3 < 1.00001 {
        return Vec::new();
    }
    let (q1, q2, q3) = (d1 * d1, d2 * d2, d3 * d3);
    let aux1 = ((q1 - q2) / (q1 - q3)).sqrt();
    let aux3 = ((q2 - q3) / (q1 - q3)).sqrt();
    let x1 = [aux1, aux1, -aux1, -aux1];
    let x3 = [aux3, -aux3, aux3, -aux3];
    let mut out = Vec::with_capacity(8);
    let sin_theta = ((q1 - q2) * (q2 - q3)).sqrt() / ((d1 + d3) * d2);
    let cos_theta = (q2 + d1 * d3) / ((d1 + d3) * d2);
    for (i, st) in [sin_theta, -sin_theta, -sin_theta, sin_theta].into_iter().enumerate() {
        let rp = Matrix3::new(cos_theta, 0.0, -st, 0.0, 1.0, 0.0, st, 0.0, cos_theta);
        let t = u * Vector3::new(x1[i], 0.0, -x3[i]) * (d1 - d3);
        out.push(Cam { r: s * u * rp * v_t, t: t.normalize() });
    }
    let sin_phi = ((q1 - q2) * (q2 - q3)).sqrt() / ((d1 - d3) * d2);
    let cos_phi = (d1 * d3 - q2) / ((d1 - d3) * d2);
    for (i, sp) in [sin_phi, -sin_phi, -sin_phi, sin_phi].into_iter().enumerate() {
        let rp = Matrix3::new(cos_phi, 0.0, sp, 0.0, -1.0, 0.0, sp, 0.0, -cos_phi);
        let t = u * Vector3::new(x1[i], 0.0, x3[i]) * (d1 + d3);
        out.push(Cam { r: s * u * rp * v_t, t: t.normalize() });
    }
    out.retain(|c| c.r.iter().all(|v| v.is_finite()) && c.t.iter().all(|v| v.is_finite()));
    out
}

/// The pairs a second camera explains with the first at the origin: within
/// `threshold` (normalized units) of its epipolar lines and triangulating in
/// front of both cameras.
pub fn supported(cam: &Cam, x1: &[Vector2<f64>], x2: &[Vector2<f64>], threshold: f64) -> Vec<bool> {
    let e = Matrix3::new(0.0, -cam.t.z, cam.t.y, cam.t.z, 0.0, -cam.t.x, -cam.t.y, cam.t.x, 0.0) * cam.r;
    let limit = threshold * threshold;
    let origin = Cam::identity();
    (0..x1.len())
        .map(|i| {
            sampson(&e, &x1[i], &x2[i]) < limit
                && triangulate(&[(origin, x1[i]), (*cam, x2[i])]).is_some_and(|x| x.z > 0.0 && (cam.r * x + cam.t).z > 0.0)
        })
        .collect()
}

/// The point that best explains its normalized views, by the linear
/// method on the homogeneous point.
pub fn triangulate(views: &[(Cam, Vector2<f64>)]) -> Option<Vector3<f64>> {
    let mut ata = SMatrix::<f64, 4, 4>::zeros();
    for (cam, x) in views {
        let row = |k: usize| SVector::<f64, 4>::new(cam.r[(k, 0)], cam.r[(k, 1)], cam.r[(k, 2)], cam.t[k]);
        let (p0, p1, p2) = (row(0), row(1), row(2));
        for a in [p2 * x.x - p0, p2 * x.y - p1] {
            let a = a.normalize();
            ata += a * a.transpose();
        }
    }
    let h = smallest_eigenvector(ata);
    (h.w.abs() > 1e-12).then(|| Vector3::new(h.x / h.w, h.y / h.w, h.z / h.w)).filter(|x| x.iter().all(|v| v.is_finite()))
}

/// The angle in radians between the rays from two camera centres to `x`.
pub fn ray_angle(a: &Vector3<f64>, b: &Vector3<f64>, x: &Vector3<f64>) -> f64 {
    (x - a).angle(&(x - b))
}
