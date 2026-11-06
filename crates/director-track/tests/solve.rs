//! The camera solve on `solve.mp4`, Eevee's plate of a yard walked past by a
//! camera whose true path `solve.json` holds in the solve's world, and on
//! `pan.mp4`, the same yard from a camera on a tripod with its path in `pan.json`.

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
    truth_of("solve.json")
}

fn truth_of(name: &str) -> Truth {
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(fixture(name)).unwrap()).unwrap();
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
    let options = SolveOptions { yfov: truth.yfov, refine_focal: false, distortion: None, camera_height: truth.height, pitch: None };
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
    let options = SolveOptions { yfov: 36f64.to_radians(), refine_focal: true, distortion: None, camera_height: truth.height, pitch: None };
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
    let still = Footage { width: 480, height: 270, fps: 30.0, frames: vec![first; 12], matte: Vec::new() };
    let error = solve(&still, &SolveOptions::default()).unwrap_err();
    assert!(error.contains("too little camera motion"), "{error}");
}

#[test]
fn a_missing_file_names_itself() {
    let error = read_footage(&fixture("no-such-plate.mp4"), 960).err().unwrap();
    assert!(error.contains("no-such-plate.mp4"), "{error}");
}

#[test]
fn the_solve_finds_the_lens_distortion() {
    // `lens.mp4` is the same shot through a lens with k1 = -0.15, k2 = 0.03.
    let truth = truth();
    let footage = read_footage(&fixture("lens.mp4"), 960).unwrap();
    let options = SolveOptions { yfov: truth.yfov, refine_focal: false, distortion: None, camera_height: truth.height, pitch: None };
    let solved = solve(&footage, &options).unwrap();
    let Errors { position, rotation, .. } = compare(&solved, &truth);
    // k1 and k2 trade against each other, so compare where each lens sends
    // points along the diagonal, in pixels.
    let f = 135.0 / (truth.yfov / 2.0).tan();
    let bend = |k: [f64; 2], r: f64| r * (1.0 + k[0] * r * r + k[1] * r.powi(4)) * f;
    let worst = (1..=8).map(|i| i as f64 / 8.0 * 275.0 / f).map(|r| (bend(solved.distortion, r) - bend([-0.15, 0.03], r)).abs()).fold(0.0, f64::max);
    eprintln!(
        "distortion k1 {:.4} k2 {:.4}, worst {worst:.2} px out to the corner; {:.3} px rms, worst position {position:.3} m, rotation {rotation:.3} deg",
        solved.distortion[0], solved.distortion[1], solved.error
    );
    assert!(worst < 0.75, "the solved lens bends every radius within 0.75 px of the true one: {worst:.2} px");
    assert!(solved.error < 0.4, "the cameras explain the tracks: {:.3} px", solved.error);
    // The true lens fixed gives 0.10 m and 0.16 degrees on this resampled plate.
    assert!(position < 0.12 && rotation < 0.25, "the path holds: {position:.3} m, {rotation:.3} deg");
}

#[test]
fn a_matte_keeps_moving_things_out_of_the_solve() {
    // `moving.mp4` is the same walk past a van, a walker and a rippling
    // pool, and `moving-matte.mp4` is white over them.
    let truth = truth();
    let options = SolveOptions { yfov: truth.yfov, refine_focal: false, distortion: None, camera_height: truth.height, pitch: None };
    let mut footage = read_footage(&fixture("moving.mp4"), 960).unwrap();
    let unmasked = compare(&solve(&footage, &options).unwrap(), &truth);
    footage.read_matte(&fixture("moving-matte.mp4")).unwrap();
    assert_eq!(footage.matte.len(), 60, "a matte frame for every frame");
    let solved = solve(&footage, &options).unwrap();
    let Errors { position, rotation, pixels } = compare(&solved, &truth);
    eprintln!(
        "unmasked: worst position {:.3} m, rotation {:.3} deg, grid {:.2} px; masked: {} points, {:.3} px rms, worst position {position:.3} m, rotation {rotation:.3} deg, grid {pixels:.2} px",
        unmasked.position, unmasked.rotation, unmasked.pixels, solved.points.len(), solved.error
    );
    assert!(unmasked.position > 0.3, "the movers pull the unmasked path: {:.3} m", unmasked.position);
    assert!(position < 0.15 && rotation < 0.25, "the masked path holds: {position:.3} m, {rotation:.3} deg");
    // Unmasked, the grid misses by 2.9 px.
    assert!(pixels < 1.5, "a model near the origin lands within 1.5 px of where Blender drew it: {pixels:.2} px");
}

#[test]
fn a_matte_must_match_the_frames() {
    let small = director_track::Gray::new(240, 135, vec![0.0; 240 * 135]);
    let footage = Footage { width: 480, height: 270, fps: 30.0, frames: footage().frames.clone(), matte: vec![small] };
    let error = solve(&footage, &SolveOptions::default()).unwrap_err();
    assert!(error.contains("matte of the footage's 480x270 frames"), "{error}");
}

/// Frame 0's true tilt below the horizon in radians.
fn pitch(truth: &Truth) -> f64 {
    let view = truth.rotations[0] * Vector3::new(0.0, 0.0, -1.0);
    (-view.y).asin()
}

#[test]
fn a_camera_that_only_turns_solves_from_its_pitch() {
    let truth = truth_of("pan.json");
    let footage = read_footage(&fixture("pan.mp4"), 960).unwrap();
    let fixed = SolveOptions { yfov: truth.yfov, refine_focal: false, distortion: Some([0.0, 0.0]), camera_height: truth.height, pitch: Some(pitch(&truth)) };
    let solved = solve(&footage, &fixed).unwrap();
    let Errors { position, rotation, pixels } = compare(&solved, &truth);
    let guessed = solve(&footage, &SolveOptions { yfov: 36f64.to_radians(), refine_focal: true, ..fixed }).unwrap();
    let from_guess = compare(&guessed, &truth);
    eprintln!(
        "pan at pitch {:.2} deg: {:.3} px rms, worst position {position:.3} m, rotation {rotation:.3} deg, grid {pixels:.2} px; from 36 deg: {:.3} deg, rotation {:.3} deg, grid {:.2} px",
        pitch(&truth).to_degrees(),
        solved.error,
        guessed.yfov.to_degrees(),
        from_guess.rotation,
        from_guess.pixels
    );
    assert!(solved.points.is_empty(), "a turning camera triangulates nothing");
    assert!(solved.error < 0.4, "the turns explain the tracks: {:.3} px", solved.error);
    assert!(position < 0.01, "the camera stays on its tripod: {position:.3} m");
    assert!(rotation < 0.1, "every camera turns within 0.1 degrees of the truth: {rotation:.3}");
    assert!(pixels < 1.0, "a model near the origin lands within a pixel of where Blender drew it: {pixels:.2} px");
    assert!((guessed.yfov / truth.yfov - 1.0).abs() < 0.005, "the turn finds the field of view within 0.5 %");
    assert!(from_guess.rotation < 0.1 && from_guess.pixels < 1.0, "the path holds from the guess: {:.3} deg, {:.2} px", from_guess.rotation, from_guess.pixels);
}

#[test]
fn a_turning_camera_needs_its_pitch() {
    let footage = read_footage(&fixture("pan.mp4"), 960).unwrap();
    let error = solve(&footage, &SolveOptions { yfov: 42f64.to_radians(), ..SolveOptions::default() }).unwrap_err();
    assert!(error.contains("its pitch"), "{error}");
    let error = solve(&footage, &SolveOptions { pitch: Some(2.0), ..SolveOptions::default() }).unwrap_err();
    assert!(error.contains("pitch between -90 and 90"), "{error}");
}
