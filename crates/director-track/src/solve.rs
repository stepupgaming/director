//! The incremental camera solve and the ground-aligned world it reports in.

use nalgebra::{Matrix3, Rotation3, SymmetricEigen, UnitQuaternion, Vector2, Vector3};
use serde::{Deserialize, Serialize};

use crate::bundle::{adjust, resect, rms, Obs, Refine};
use crate::footage::Footage;
use crate::geometry::{
    essential_ransac, factor_essential, factor_homography, homography_ransac, ray_angle, supported, triangulate, Cam, Intrinsics, Rng,
};
use crate::tracks::{track, Track};

/// A new point needs rays at least this far apart, in radians.
const MIN_ANGLE: f64 = 1.5 * std::f64::consts::PI / 180.0;
/// A new point must land within this many pixels of every view.
const MAX_ERROR: f64 = 2.0;
/// The bundle passes while frames register move only the cameras of this
/// many most recently placed frames.
const WINDOW: usize = 24;
/// A start pair's best pose must hold more than this many times the tracks
/// of any distinct pose.
const AMBIGUITY: f64 = 0.8;
/// A solved distortion stays only when it brings the rms reprojection error
/// below this fraction of the pinhole lens's.
const KEEP_DISTORTION: f64 = 0.9;
/// After the solve, points missing any view by more pixels than this go.
const PRUNE: f64 = 3.0;

/// What the solve knows about the lens and the scale.
#[derive(Clone, Debug, PartialEq)]
pub struct SolveOptions {
    /// The lens's vertical field of view in radians: the solve's lens, or
    /// its starting guess with `refine_focal`.
    pub yfov: f64,
    /// Refine the focal length in the bundle adjustment.
    pub refine_focal: bool,
    /// The lens's radial distortion (k1, k2) on the z = 1 plane, or `None`
    /// to solve it from none.
    pub distortion: Option<[f64; 2]>,
    /// Frame 0's camera height above the ground in metres, which sets the scale.
    pub camera_height: f64,
}

impl Default for SolveOptions {
    fn default() -> Self {
        Self { yfov: 40f64.to_radians(), refine_focal: false, distortion: None, camera_height: 1.6 }
    }
}

/// One frame's camera in the solve's world: position in metres and a glTF
/// rotation quaternion (x, y, z, w) of a camera looking down its local -Z
/// with +Y up.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

/// A solved shot: a camera per frame in a world with the ground at y = 0,
/// the origin where frame 0's optical axis meets the ground and frame 0's
/// view along -Z.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraSolve {
    /// Frames per second of the footage; frame k shows from k / fps seconds.
    pub fps: f64,
    /// The footage size in pixels.
    pub width: u32,
    pub height: u32,
    /// The solved vertical field of view in radians.
    pub yfov: f64,
    /// The solved radial distortion (k1, k2): a point (x, y) on the z = 1
    /// plane lands at (x, y) (1 + k1 r^2 + k2 r^4) with r^2 = x^2 + y^2.
    pub distortion: [f64; 2],
    pub frames: Vec<Pose>,
    /// The solved scene points in the same world.
    pub points: Vec<[f32; 3]>,
    /// The root mean square reprojection error in footage pixels.
    pub error: f64,
}

/// Whether two relative poses differ by more than 1 degree of rotation or
/// 10 degrees in the direction of travel.
fn distinct(a: &Cam, b: &Cam) -> bool {
    let turn = Rotation3::from_matrix(&(a.r * b.r.transpose())).angle();
    let (ta, tb) = (a.centre().normalize(), b.centre().normalize());
    turn > 1f64.to_radians() || ta.dot(&tb).clamp(-1.0, 1.0).acos() > 10f64.to_radians()
}

#[derive(Clone)]
struct Scene {
    cams: Vec<Option<Cam>>,
    points: Vec<Vector3<f64>>,
    point_track: Vec<usize>,
    track_point: Vec<Option<usize>>,
}

fn pixel(p: [f32; 2]) -> Vector2<f64> {
    Vector2::new(p[0] as f64, p[1] as f64)
}

impl Scene {
    /// Triangulates every track without a point that two placed cameras see
    /// with enough parallax and that lands within `MAX_ERROR` in every view.
    fn triangulate(&mut self, tracks: &[Track], intr: &Intrinsics) {
        for (ti, t) in tracks.iter().enumerate() {
            if self.track_point[ti].is_some() {
                continue;
            }
            let views: Vec<(Cam, [f32; 2])> = (t.start..t.end()).filter_map(|k| Some((self.cams[k]?, t.at(k)?))).collect();
            if views.len() < 2 {
                continue;
            }
            let rays: Vec<_> = views.iter().map(|(cam, p)| (*cam, intr.normalize(*p))).collect();
            let Some(x) = triangulate(&rays) else { continue };
            let (first, last) = (views[0].0.centre(), views[views.len() - 1].0.centre());
            if ray_angle(&first, &last, &x) < MIN_ANGLE {
                continue;
            }
            let fits = views
                .iter()
                .all(|(cam, p)| intr.project(cam, &x).is_some_and(|q| (q - pixel(*p)).norm() <= MAX_ERROR));
            if fits {
                self.track_point[ti] = Some(self.points.len());
                self.points.push(x);
                self.point_track.push(ti);
            }
        }
    }

    /// The placed cameras in frame order, each frame's slot among them and
    /// every observation of a point by a placed camera.
    fn observations(&self, tracks: &[Track]) -> (Vec<usize>, Vec<Option<usize>>, Vec<Obs>) {
        let frames: Vec<usize> = (0..self.cams.len()).filter(|&k| self.cams[k].is_some()).collect();
        let mut slot = vec![None; self.cams.len()];
        for (s, &k) in frames.iter().enumerate() {
            slot[k] = Some(s);
        }
        let mut obs = Vec::new();
        for (p, &ti) in self.point_track.iter().enumerate() {
            let t = &tracks[ti];
            for (i, uv) in t.points.iter().enumerate() {
                if let Some(cam) = slot[t.start + i] {
                    obs.push(Obs { cam, point: p, uv: pixel(*uv) });
                }
            }
        }
        (frames, slot, obs)
    }

    fn adjust(&mut self, tracks: &[Track], intr: &mut Intrinsics, fixed_frame: usize, refine: Refine, iterations: usize) {
        let (frames, _, obs) = self.observations(tracks);
        let mut cams: Vec<Cam> = frames.iter().map(|&k| self.cams[k].expect("a placed camera")).collect();
        let fixed: Vec<bool> = frames.iter().map(|&k| k == fixed_frame).collect();
        adjust(&mut cams, &mut self.points, &obs, &fixed, intr, refine, iterations);
        for (cam, &k) in cams.into_iter().zip(&frames) {
            self.cams[k] = Some(cam);
        }
    }

    /// Refines the cameras of the frames in `free` and every point they see,
    /// with the other placed cameras that see those points held still, and
    /// the lens parameters `refine` names.
    fn adjust_local(&mut self, tracks: &[Track], intr: &mut Intrinsics, free: &[usize], refine: Refine, iterations: usize) {
        let mut moving = vec![false; self.cams.len()];
        free.iter().for_each(|&k| moving[k] = true);
        let seen: Vec<usize> = (0..self.points.len())
            .filter(|&p| {
                let t = &tracks[self.point_track[p]];
                (t.start..t.end()).any(|k| moving[k] && self.cams[k].is_some())
            })
            .collect();
        let mut slot = vec![None; self.cams.len()];
        let mut frames = Vec::new();
        let mut obs = Vec::new();
        for (local, &p) in seen.iter().enumerate() {
            let t = &tracks[self.point_track[p]];
            for (i, uv) in t.points.iter().enumerate() {
                let k = t.start + i;
                if self.cams[k].is_none() {
                    continue;
                }
                let cam = *slot[k].get_or_insert_with(|| {
                    frames.push(k);
                    frames.len() - 1
                });
                obs.push(Obs { cam, point: local, uv: pixel(*uv) });
            }
        }
        let mut cams: Vec<Cam> = frames.iter().map(|&k| self.cams[k].expect("a placed camera")).collect();
        let fixed: Vec<bool> = frames.iter().map(|&k| !moving[k]).collect();
        let mut points: Vec<Vector3<f64>> = seen.iter().map(|&p| self.points[p]).collect();
        adjust(&mut cams, &mut points, &obs, &fixed, intr, refine, iterations);
        for (cam, &k) in cams.into_iter().zip(&frames) {
            self.cams[k] = Some(cam);
        }
        for (x, &p) in points.into_iter().zip(&seen) {
            self.points[p] = x;
        }
    }

    /// The rms reprojection error of every observation in pixels.
    fn rms(&self, tracks: &[Track], intr: &Intrinsics) -> f64 {
        let (frames, _, obs) = self.observations(tracks);
        let cams: Vec<Cam> = frames.iter().map(|&k| self.cams[k].expect("placed")).collect();
        rms(&cams, &self.points, &obs, intr)
    }

    /// Drops every point that misses one of its views by more than `limit` pixels.
    fn prune(&mut self, tracks: &[Track], intr: &Intrinsics, limit: f64) {
        let mut keep = vec![true; self.points.len()];
        for (p, &ti) in self.point_track.iter().enumerate() {
            let t = &tracks[ti];
            keep[p] = t.points.iter().enumerate().all(|(i, uv)| match self.cams[t.start + i] {
                Some(cam) => intr.project(&cam, &self.points[p]).is_some_and(|q| (q - pixel(*uv)).norm() <= limit),
                None => true,
            });
        }
        let (mut points, mut point_track) = (Vec::new(), Vec::new());
        self.track_point.iter_mut().for_each(|t| *t = None);
        for ((x, ti), keep) in self.points.iter().zip(&self.point_track).zip(keep) {
            if keep {
                self.track_point[*ti] = Some(points.len());
                points.push(*x);
                point_track.push(*ti);
            }
        }
        self.points = points;
        self.point_track = point_track;
    }
}

/// Solves a camera for every frame of `footage`.
pub fn solve(footage: &Footage, options: &SolveOptions) -> Result<CameraSolve, String> {
    let n = footage.frames.len();
    if n < 3 {
        return Err(format!("a camera solve needs at least 3 frames, and the footage has {n}"));
    }
    if !(options.yfov > 0.0 && options.yfov < std::f64::consts::PI) {
        return Err("a camera solve needs a field of view between 0 and 180 degrees".into());
    }
    if options.camera_height.is_nan() || options.camera_height <= 0.0 {
        return Err("a camera solve needs a camera height above 0".into());
    }
    let (w, h) = (footage.frames[0].width, footage.frames[0].height);
    let [k1, k2] = options.distortion.unwrap_or_default();
    if !(k1.is_finite() && k2.is_finite()) {
        return Err("a camera solve needs finite distortion".into());
    }
    let mut intr = Intrinsics {
        f: (h as f64 / 2.0) / (options.yfov / 2.0).tan(),
        cx: (w as f64 - 1.0) / 2.0,
        cy: (h as f64 - 1.0) / 2.0,
        k1,
        k2,
    };
    let lens = Refine { focal: options.refine_focal, distortion: false };
    let tracks = track(&footage.frames);
    let mut rng = Rng::new(7);

    // The starting pair: frame 0 and, among the frames whose tracks single
    // out one pose, the frame whose shared tracks give the most triangulated
    // points times their median parallax, capped at 4 degrees.
    let a = 0;
    let mut best: Option<(usize, Cam, f64, f64)> = None;
    for b in (a + 2..n).step_by(2) {
        let shared: Vec<&Track> = tracks.iter().filter(|t| t.at(a).is_some() && t.at(b).is_some()).collect();
        if shared.len() < 30 {
            break;
        }
        let x1: Vec<_> = shared.iter().map(|t| intr.normalize(t.at(a).expect("shared"))).collect();
        let x2: Vec<_> = shared.iter().map(|t| intr.normalize(t.at(b).expect("shared"))).collect();
        // A plane of points leaves the 8-point system degenerate, so the
        // homography's poses compete with the essential matrix's.
        let threshold = 1.0 / intr.f;
        let mut candidates = Vec::new();
        if let Some((e, _)) = essential_ransac(&x1, &x2, threshold, 300, &mut rng) {
            candidates.extend(factor_essential(&e));
        }
        if let Some(h) = homography_ransac(&x1, &x2, 2.0 * threshold, 300, &mut rng) {
            candidates.extend(factor_homography(&h));
        }
        let scored: Vec<(Cam, Vec<bool>, usize)> = candidates
            .into_iter()
            .map(|cam| {
                let front = supported(&cam, &x1, &x2, threshold);
                let count = front.iter().filter(|&&f| f).count();
                (cam, front, count)
            })
            .collect();
        let Some(best_index) = (0..scored.len()).max_by_key(|&i| scored[i].2) else {
            continue;
        };
        let (cam, ref front, count) = scored[best_index];
        // Over a short baseline or a flat scene several distinct poses
        // explain the tracks about as well, and the wrong one folds the
        // scene, so the pair must single one out.
        let rival = scored.iter().filter(|(other, _, _)| distinct(&cam, other)).map(|s| s.2).max().unwrap_or(0);
        if rival as f64 > AMBIGUITY * count as f64 {
            continue;
        }
        let centre = cam.centre();
        let mut angles: Vec<f64> = (0..x1.len())
            .filter(|&i| front[i])
            .filter_map(|i| triangulate(&[(Cam::identity(), x1[i]), (cam, x2[i])]))
            .map(|x| ray_angle(&Vector3::zeros(), &centre, &x))
            .collect();
        if angles.len() < 30 {
            continue;
        }
        angles.sort_by(f64::total_cmp);
        let median = angles[angles.len() / 2].to_degrees();
        let score = angles.len() as f64 * median.min(4.0);
        if best.as_ref().is_none_or(|(_, _, s, _)| score > *s) {
            best = Some((b, cam, score, median));
        }
    }
    let Some((b, cam_b, _, _)) = best.filter(|(_, _, _, median)| *median >= 0.5) else {
        return Err("the footage shows too little camera motion or texture to solve a camera".into());
    };

    let mut scene = Scene {
        cams: vec![None; n],
        points: Vec::new(),
        point_track: Vec::new(),
        track_point: vec![None; tracks.len()],
    };
    scene.cams[a] = Some(Cam::identity());
    scene.cams[b] = Some(cam_b);
    scene.triangulate(&tracks, &intr);
    scene.adjust(&tracks, &mut intr, a, Refine::default(), 30);

    let order: Vec<usize> = (a + 1..b).chain(b + 1..n).chain((0..a).rev()).collect();
    for (i, &k) in order.iter().enumerate() {
        let from = if k > a { k - 1 } else { k + 1 };
        let mut cam = scene.cams[from].expect("the neighbouring frame is placed");
        let (xs, uvs): (Vec<_>, Vec<_>) = tracks
            .iter()
            .enumerate()
            .filter_map(|(ti, t)| Some((scene.points[scene.track_point[ti]?], pixel(t.at(k)?))))
            .unzip();
        if xs.len() < 12 {
            return Err(format!("frame {k} keeps too few solved points ({}) to place its camera", xs.len()));
        }
        resect(&mut cam, &xs, &uvs, &intr, 30);
        scene.cams[k] = Some(cam);
        scene.triangulate(&tracks, &intr);
        if (i + 1) % 8 == 0 {
            let recent = &order[(i + 1).saturating_sub(WINDOW)..=i];
            scene.adjust_local(&tracks, &mut intr, recent, Refine::default(), 10);
        }
    }
    scene.adjust(&tracks, &mut intr, a, lens, 60);
    scene.prune(&tracks, &intr, PRUNE);
    scene.adjust(&tracks, &mut intr, a, lens, 60);
    if options.distortion.is_none() {
        // Distortion trades against depth, so even a slight one bends the
        // path of a pinhole shot. The solve keeps it only when it explains
        // the same tracks clearly better than the pinhole lens.
        let pinhole = (scene.clone(), intr);
        let before = scene.rms(&tracks, &intr);
        scene.adjust(&tracks, &mut intr, a, Refine { distortion: true, ..lens }, 20);
        if scene.rms(&tracks, &intr) < KEEP_DISTORTION * before {
            scene.triangulate(&tracks, &intr);
            scene.prune(&tracks, &intr, PRUNE);
            scene.adjust(&tracks, &mut intr, a, Refine { distortion: true, ..lens }, 60);
        } else {
            (scene, intr) = pinhole;
        }
    }

    let error = scene.rms(&tracks, &intr) * footage.width as f64 / w as f64;
    let (frames, _, _) = scene.observations(&tracks);
    let cams: Vec<Cam> = frames.iter().map(|&k| scene.cams[k].expect("placed")).collect();
    let (poses, points) = align(&cams, &scene.points, options.camera_height)?;
    Ok(CameraSolve {
        fps: footage.fps,
        width: footage.width,
        height: footage.height,
        yfov: 2.0 * ((h as f64 / 2.0) / intr.f).atan(),
        distortion: [intr.k1, intr.k2],
        frames: poses,
        points,
        error,
    })
}

/// Moves the solve into the ground-aligned world: the dominant plane below
/// frame 0's camera (normal within 60 degrees of its up) becomes y = 0, and
/// frame 0's camera stands `height` metres above it.
fn align(cams: &[Cam], points: &[Vector3<f64>], height: f64) -> Result<(Vec<Pose>, Vec<[f32; 3]>), String> {
    let first = cams[0];
    let centre = first.centre();
    let forward = first.r.transpose() * Vector3::z();
    let up = first.r.transpose() * -Vector3::y();
    if points.len() < 3 {
        return Err("the camera solve kept too few points to find the ground".into());
    }
    let mut depths: Vec<f64> = points.iter().map(|p| (p - centre).norm()).collect();
    depths.sort_by(f64::total_cmp);
    let threshold = 0.01 * depths[depths.len() / 2];
    let mut rng = Rng::new(11);
    let mut best: Option<(Vector3<f64>, f64, usize)> = None;
    for _ in 0..1000 {
        let (i, j, k) = (rng.below(points.len()), rng.below(points.len()), rng.below(points.len()));
        let Some(mut n) = (points[j] - points[i]).cross(&(points[k] - points[i])).try_normalize(1e-12) else { continue };
        if n.dot(&up) < 0.0 {
            n = -n;
        }
        let d = n.dot(&points[i]);
        if n.dot(&up) < 0.5 || n.dot(&centre) - d <= 0.0 {
            continue;
        }
        let count = points.iter().filter(|p| (n.dot(p) - d).abs() < threshold).count();
        if best.as_ref().is_none_or(|(_, _, c)| count > *c) {
            best = Some((n, d, count));
        }
    }
    let (n, d, _) = best.ok_or("the camera solve found no ground plane under the first camera")?;
    let inliers: Vec<&Vector3<f64>> = points.iter().filter(|p| (n.dot(p) - d).abs() < threshold).collect();
    let mean = inliers.iter().copied().sum::<Vector3<f64>>() / inliers.len() as f64;
    let scatter = inliers.iter().fold(Matrix3::zeros(), |m, p| m + (*p - mean) * (*p - mean).transpose());
    let eigen = SymmetricEigen::new(scatter);
    let mut normal: Vector3<f64> = eigen.eigenvectors.column(eigen.eigenvalues.imin()).into_owned();
    if normal.dot(&up) < 0.0 {
        normal = -normal;
    }
    let offset = normal.dot(&mean);
    let above = normal.dot(&centre) - offset;
    if above <= 0.0 {
        return Err("the first camera stands below the ground the solve found".into());
    }
    let along = normal.dot(&forward);
    let reach = -above / along;
    let origin = if along < -1e-6 && reach.is_finite() { centre + forward * reach } else { centre - normal * above };
    let mut flat = forward - normal * forward.dot(&normal);
    if flat.norm() < 1e-3 {
        flat = up - normal * up.dot(&normal);
    }
    let z = -flat.normalize();
    let x = normal.cross(&z);
    let basis = Matrix3::from_rows(&[x.transpose(), normal.transpose(), z.transpose()]);
    let scale = height / above;
    let flip = Matrix3::from_diagonal(&Vector3::new(1.0, -1.0, -1.0));
    let mut previous: Option<UnitQuaternion<f64>> = None;
    let poses = cams
        .iter()
        .map(|cam| {
            let p = scale * basis * (cam.centre() - origin);
            let rotation = Rotation3::from_matrix(&(basis * cam.r.transpose() * flip));
            let mut q = UnitQuaternion::from_rotation_matrix(&rotation);
            if previous.is_some_and(|prev| prev.coords.dot(&q.coords) < 0.0) {
                q = UnitQuaternion::new_unchecked(-q.into_inner());
            }
            previous = Some(q);
            Pose { position: [p.x as f32, p.y as f32, p.z as f32], rotation: [q.i as f32, q.j as f32, q.k as f32, q.w as f32] }
        })
        .collect();
    let points = points
        .iter()
        .map(|p| {
            let q = scale * basis * (p - origin);
            [q.x as f32, q.y as f32, q.z as f32]
        })
        .collect();
    Ok((poses, points))
}
