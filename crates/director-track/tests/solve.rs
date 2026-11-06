//! The camera solve on `solve.mp4`, Eevee's plate of a yard walked past by a
//! camera whose true path `solve.json` holds in the solve's world.

use director_track::{read_footage, solve, CameraSolve, Footage, SolveOptions};
use nalgebra::{Quaternion, UnitQuaternion, Vector3};
use std::path::PathBuf;
use std::sync::OnceLock;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../director-3d/tests/fixtures").join(name)
}

struct Truth {
    yfov: f64,
    height: f64,
    positions: Vec<Vector3<f64>>,
    rotations: Vec<UnitQuaternion<f64>>,
}

fn truth() -> Truth {
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(fixture("solve.json")).unwrap()).unwrap();
    let frames = json["frames"].as_array().unwrap();
    let v = |a: &serde_json::Value, i: usize| a[i].as_f64().unwrap();
    Truth {
        yfov: json["yfov"].as_f64().unwrap(),
        height: json["camera_height"].as_f64().unwrap(),
        positions: frames.iter().map(|f| Vector3::new(v(&f["position"], 0), v(&f["position"], 1), v(&f["position"], 2))).collect(),
        rotations: frames
            .iter()
            .map(|f| {
                let r = &f["rotation"];
                UnitQuaternion::from_quaternion(Quaternion::new(v(r, 3), v(r, 0), v(r, 1), v(r, 2)))
            })
            .collect(),
    }
}

fn footage() -> &'static Footage {
    static FOOTAGE: OnceLock<Footage> = OnceLock::new();
    FOOTAGE.get_or_init(|| read_footage(&fixture("solve.mp4"), 960).unwrap())
}

fn pose(position: [f32; 3], rotation: [f32; 4]) -> (Vector3<f64>, UnitQuaternion<f64>) {
    let p = Vector3::new(position[0] as f64, position[1] as f64, position[2] as f64);
    let r = rotation.map(|v| v as f64);
    (p, UnitQuaternion::from_quaternion(Quaternion::new(r[3], r[0], r[1], r[2])))
}

/// Where a glTF camera (looking down -Z, +Y up) at `p`, `q` with vertical
/// field of view `yfov` sees `x` on a 480x270 frame.
fn project(p: &Vector3<f64>, q: &UnitQuaternion<f64>, yfov: f64, x: &Vector3<f64>) -> [f64; 2] {
    let c = q.inverse() * (x - p);
    let f = 135.0 / (yfov / 2.0).tan();
    [239.5 + f * c.x / -c.z, 134.5 - f * c.y / -c.z]
}

struct Errors {
    /// The worst camera position error in metres.
    position: f64,
    /// The worst camera rotation error in degrees.
    rotation: f64,
    /// The worst pixel distance between where the true and the solved
    /// cameras see a grid of points near the origin, on the ground and 0.8 m up.
    pixels: f64,
}

fn compare(solved: &CameraSolve, truth: &Truth) -> Errors {
    assert_eq!(solved.frames.len(), truth.positions.len(), "a camera for every frame");
    let mut grid = Vec::new();
    for x in [-1.0, 0.0, 1.0] {
        for z in [-1.0, 0.0, 1.0] {
            for y in [0.0, 0.8] {
                grid.push(Vector3::new(x, y, z));
            }
        }
    }
    let mut errors = Errors { position: 0.0, rotation: 0.0, pixels: 0.0 };
    for (i, frame) in solved.frames.iter().enumerate() {
        let (p, q) = pose(frame.position, frame.rotation);
        let (tp, tq) = (truth.positions[i], truth.rotations[i]);
        errors.position = errors.position.max((p - tp).norm());
        errors.rotation = errors.rotation.max(q.angle_to(&tq).to_degrees());
        for x in &grid {
            let (a, b) = (project(&p, &q, solved.yfov, x), project(&tp, &tq, truth.yfov, x));
            errors.pixels = errors.pixels.max((a[0] - b[0]).hypot(a[1] - b[1]));
        }
    }
    errors
}

#[test]
fn the_solve_walks_the_true_camera_path() {
    let truth = truth();
    let footage = footage();
    assert_eq!((footage.width, footage.height, footage.frames.len()), (480, 270, 60));
    let options = SolveOptions { yfov: truth.yfov, refine_focal: false, camera_height: truth.height };
    let start = std::time::Instant::now();
    let solved = solve(footage, &options).unwrap();
    let Errors { position, rotation, pixels } = compare(&solved, &truth);
    eprintln!(
        "solve {:.2} s: {} points, {:.3} px rms, worst position {:.3} m, rotation {:.3} deg, grid {:.2} px",
        start.elapsed().as_secs_f64(),
        solved.points.len(),
        solved.error,
        position,
        rotation,
        pixels
    );
    assert!(solved.error < 0.4, "the cameras explain the tracks: {:.3} px", solved.error);
    assert!(position < 0.08, "every camera stands within 8 cm of the truth: {position:.3} m");
    assert!(rotation < 0.15, "every camera turns within 0.15 degrees of the truth: {rotation:.3}");
    assert!(pixels < 1.0, "a model near the origin lands within a pixel of where Blender drew it: {pixels:.2} px");
    // The ground sits at y = 0 in the solve's world.
    let mut heights: Vec<f32> = solved.points.iter().map(|p| p[1]).collect();
    heights.sort_by(f32::total_cmp);
    assert!(heights[heights.len() / 4].abs() < 0.02, "the lowest quarter of the points lies on the ground");
}

#[test]
fn the_solve_recovers_the_lens_from_a_wrong_guess() {
    let truth = truth();
    let options = SolveOptions { yfov: 36f64.to_radians(), refine_focal: true, camera_height: truth.height };
    let solved = solve(footage(), &options).unwrap();
    let Errors { position, rotation, pixels } = compare(&solved, &truth);
    eprintln!(
        "from 36 deg: {:.3} deg against {:.3} deg, {:.3} px rms, worst position {:.3} m, rotation {:.3} deg, grid {:.2} px",
        solved.yfov.to_degrees(),
        truth.yfov.to_degrees(),
        solved.error,
        position,
        rotation,
        pixels
    );
    assert!((solved.yfov / truth.yfov - 1.0).abs() < 0.01, "the field of view within 1 %");
    assert!(position < 0.1 && rotation < 0.2, "the path holds: {position:.3} m, {rotation:.3} deg");
    assert!(pixels < 1.2, "a model near the origin lands within 1.2 px of where Blender drew it: {pixels:.2} px");
}

#[test]
fn still_footage_has_nothing_to_solve() {
    let first = footage().frames[0].clone();
    let still = Footage { width: 480, height: 270, fps: 30.0, frames: vec![first; 12] };
    let error = solve(&still, &SolveOptions::default()).unwrap_err();
    assert!(error.contains("too little camera motion"), "{error}");
}

#[test]
fn a_missing_file_names_itself() {
    let error = read_footage(&fixture("no-such-plate.mp4"), 960).err().unwrap();
    assert!(error.contains("no-such-plate.mp4"), "{error}");
}
