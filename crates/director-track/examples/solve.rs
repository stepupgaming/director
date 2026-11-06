//! Solves the camera of a clip and prints how the solve went.
//!
//! `solve <footage> [--fov DEG] [--fixed] [--distortion K1 K2] [--height M]
//! [--pitch DEG] [--matte FILE] [--json OUT]` reads the footage at most 960 px wide and
//! the matte of regions to leave out at its size, solves it with the
//! lens starting from `--fov` (default 40, refined unless `--fixed`), the
//! radial distortion fixed by `--distortion` or solved, and frame 0's camera
//! at `--height` metres (default 1.6), and prints the timings, the lens, the
//! reprojection error, the point count and the camera path. `--json` writes
//! the whole `CameraSolve`.

use director_track::{read_footage, solve, SolveOptions};
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let (mut fov, mut fixed, mut height, mut json, mut distortion, mut matte, mut pitch) = (40.0f64, false, 1.6f64, None, None, None, None);
    let number = |v: Option<String>| -> f64 { v.and_then(|v| v.parse().ok()).expect("--distortion K1 K2") };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fov" => fov = args.next().and_then(|v| v.parse().ok()).expect("--fov DEG"),
            "--fixed" => fixed = true,
            "--distortion" => distortion = Some([number(args.next()), number(args.next())]),
            "--height" => height = args.next().and_then(|v| v.parse().ok()).expect("--height M"),
            "--pitch" => pitch = Some(args.next().and_then(|v| v.parse::<f64>().ok()).expect("--pitch DEG").to_radians()),
            "--matte" => matte = Some(PathBuf::from(args.next().expect("--matte FILE"))),
            "--json" => json = Some(PathBuf::from(args.next().expect("--json OUT"))),
            _ => path = Some(PathBuf::from(arg)),
        }
    }
    let path = path.expect("usage: solve <footage> [--fov DEG] [--fixed] [--distortion K1 K2] [--height M] [--pitch DEG] [--matte FILE] [--json OUT]");

    let start = Instant::now();
    let mut footage = read_footage(&path, 960).unwrap_or_else(|e| panic!("{e}"));
    if let Some(matte) = &matte {
        footage.read_matte(matte).unwrap_or_else(|e| panic!("{e}"));
    }
    let read = start.elapsed().as_secs_f64();
    let frames = footage.frames.len();
    let size = footage.frames.first().map(|f| (f.width, f.height)).unwrap_or_default();
    println!("footage {}x{} at {} fps, {frames} frames, decoded at {}x{} in {read:.2} s", footage.width, footage.height, footage.fps, size.0, size.1);

    let start = Instant::now();
    let tracks = director_track::track(&footage.frames, &footage.matte);
    let mut lengths: Vec<usize> = tracks.iter().map(|t| t.points.len()).collect();
    lengths.sort_unstable();
    let live = |k: usize| tracks.iter().filter(|t| t.at(k).is_some()).count();
    println!(
        "tracked in {:.2} s: {} tracks, median length {}, 90th percentile {}, live at frames 0/mid/last {}/{}/{}",
        start.elapsed().as_secs_f64(),
        tracks.len(),
        lengths.get(lengths.len() / 2).copied().unwrap_or(0),
        lengths.get(lengths.len() * 9 / 10).copied().unwrap_or(0),
        live(0),
        live(frames / 2),
        live(frames.saturating_sub(1))
    );

    let options = SolveOptions { yfov: fov.to_radians(), refine_focal: !fixed, distortion, camera_height: height, pitch };
    let start = Instant::now();
    let solved = match solve(&footage, &options) {
        Ok(solved) => solved,
        Err(e) => {
            println!("solve failed after {:.2} s: {e}", start.elapsed().as_secs_f64());
            std::process::exit(1);
        }
    };
    println!(
        "solved in {:.2} s: lens {:.3} deg (from {fov}{}), distortion k1 {:.4} k2 {:.4}, {:.3} px rms, {} points, {} cameras",
        start.elapsed().as_secs_f64(),
        solved.yfov.to_degrees(),
        if fixed { ", fixed" } else { "" },
        solved.distortion[0],
        solved.distortion[1],
        solved.error,
        solved.points.len(),
        solved.frames.len()
    );
    let step = (solved.frames.len() / 10).max(1);
    let mut travelled = 0.0f64;
    for (k, pair) in solved.frames.windows(2).enumerate() {
        let d: f64 = (0..3).map(|i| (pair[1].position[i] - pair[0].position[i]) as f64).map(|v| v * v).sum::<f64>().sqrt();
        travelled += d;
        if (k + 1) % step == 0 {
            let p = pair[1].position;
            println!("  frame {:4}: position ({:8.3}, {:8.3}, {:8.3}), path so far {travelled:.3} m", k + 1, p[0], p[1], p[2]);
        }
    }
    if let Some(out) = json {
        std::fs::write(&out, serde_json::to_string(&solved).unwrap()).unwrap();
        println!("wrote {}", out.display());
    }
}
