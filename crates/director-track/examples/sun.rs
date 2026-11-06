//! `sun <footage> <solve.json>` reads the sun from the footage's shadows
//! through a `CameraSolve` that the `solve` example wrote with `--json`, and
//! prints its direction, elevation, azimuth, shadow ratio and irradiance.

use director_track::{estimate_sun, light_probe, CameraSolve};
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let usage = "usage: sun <footage> <solve.json>";
    let footage = PathBuf::from(args.next().expect(usage));
    let solve: CameraSolve = serde_json::from_str(&std::fs::read_to_string(args.next().expect(usage)).expect("read the solve")).expect("a CameraSolve");
    let start = std::time::Instant::now();
    let probe = light_probe(&footage, &solve, 256).expect("stitch the probe");
    let sun = estimate_sun(&footage, &solve, &probe).expect("read the sun");
    let [x, y, z] = sun.direction;
    println!(
        "sun ({x:.4}, {y:.4}, {z:.4}): elevation {:.2} deg, azimuth {:.2} deg, shadow {:?}, irradiance {:?} in {:.2} s",
        y.asin().to_degrees(),
        x.atan2(-z).to_degrees(),
        sun.shadow,
        sun.irradiance,
        start.elapsed().as_secs_f64()
    );
}
