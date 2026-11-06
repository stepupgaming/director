//! `estimate_sun` on `solve.mp4`, the yard walked past under a white sun of
//! 3 W/m2 at 40 degrees elevation. `make_solve.py` turns the sun with Euler
//! angles (-50, 0, 30) degrees, which in the solve's world points towards
//! (-0.5273, 0.6428, -0.5557). `sky.hdr` lights the ground with 2.94, so
//! ground in the sun's shadow reads 2.94 / (2.94 + 3 sin 40) = 0.604 of the
//! lit ground, a little less where the things casting it hide some sky.

use director_track::{estimate_sun, light_probe, read_footage, solve, CameraSolve, SolveOptions};
use nalgebra::Vector3;
use std::path::PathBuf;
use std::sync::OnceLock;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../director-3d/tests/fixtures")
        .join(name)
}

fn solved() -> &'static CameraSolve {
    static SOLVED: OnceLock<CameraSolve> = OnceLock::new();
    SOLVED.get_or_init(|| {
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(fixture("solve.json")).unwrap()).unwrap();
        let options = SolveOptions {
            yfov: json["yfov"].as_f64().unwrap(),
            refine_focal: false,
            distortion: None,
            camera_height: json["camera_height"].as_f64().unwrap(),
            pitch: None,
            zoom: false,
        };
        solve(&read_footage(&fixture("solve.mp4"), 960).unwrap(), &options).unwrap()
    })
}

#[test]
fn the_sun_falls_where_the_shadows_are() {
    let solved = solved();
    let probe = light_probe(&fixture("solve.mp4"), solved, 256).unwrap();
    let sun = estimate_sun(&fixture("solve.mp4"), solved, &probe).unwrap();
    let truth = Vector3::new(-0.52726882, 0.64278761, -0.55570827);
    let found = Vector3::from(sun.direction);
    let error = found.angle(&truth).to_degrees();
    eprintln!(
        "sun {:?} error {error:.2} deg, irradiance {:?}, shadow {:?}",
        sun.direction, sun.irradiance, sun.shadow
    );
    assert!(error < 4.0, "the sun points {error:.2} degrees off");
    let luma = |c: [f64; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let shadow = luma(sun.shadow);
    assert!(
        (shadow / 0.604 - 1.0).abs() < 0.15,
        "the shadows read {shadow:.3} of the lit ground against 0.604"
    );
    // The probe fills the sky overhead from the plate's horizon, which reads
    // dimmer than the zenith, so the strength reads somewhat low.
    let strength = luma(sun.irradiance);
    assert!(
        (strength / 3.0 - 1.0).abs() < 0.25,
        "the sun's irradiance reads {strength:.3} against 3"
    );
    for (c, e) in sun.irradiance.iter().enumerate() {
        assert!(
            (e / strength - 1.0).abs() < 0.25,
            "channel {c} tints the white sun at {:.3}",
            e / strength
        );
    }
}

#[test]
fn a_tripod_shot_cannot_read_the_sun() {
    let mut solved = solved().clone();
    solved.points.clear();
    let probe = light_probe(&fixture("solve.mp4"), &solved, 64).unwrap();
    let error = estimate_sun(&fixture("solve.mp4"), &solved, &probe).unwrap_err();
    assert!(error.contains("tripod"), "{error}");
}
