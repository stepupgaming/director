//! `light_probe` on `pan.mp4`, the yard from a tripod under `sky.hdr`,
//! stitched through the true cameras in `pan.json`. The sky's radiance is
//! known: `sky.hdr` holds the same radiance at every azimuth, and Eevee
//! renders it through the Standard view, so the plate's linear light is the
//! sky's own.

use director_track::{direction, light_probe, linear, CameraSolve, Pose, Probe};
use nalgebra::{Quaternion, UnitQuaternion};
use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../director-3d/tests/fixtures").join(name)
}

/// The tripod shot's true cameras as a solve.
fn pan() -> CameraSolve {
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(fixture("pan.json")).unwrap()).unwrap();
    let f32s = |v: &serde_json::Value| -> Vec<f32> { v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect() };
    let frames = json["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let (p, r) = (f32s(&f["position"]), f32s(&f["rotation"]));
            Pose { position: [p[0], p[1], p[2]], rotation: [r[0], r[1], r[2], r[3]], yfov: None }
        })
        .collect();
    CameraSolve {
        fps: 30.0,
        width: 480,
        height: 270,
        yfov: json["yfov"].as_f64().unwrap(),
        distortion: [0.0, 0.0],
        frames,
        points: Vec::new(),
        error: 0.0,
    }
}

fn probe(width: usize) -> Probe {
    light_probe(&fixture("pan.mp4"), &pan(), width).unwrap()
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn median(mut v: Vec<f32>) -> f32 {
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

/// The radiance of `sky.hdr` at an elevation in radians.
fn sky(elevation: f64) -> f32 {
    let (_, h, texels) = read_hdr(&std::fs::read(fixture("sky.hdr")).unwrap());
    let w = texels.len() / h;
    let row = ((0.5 - elevation / std::f64::consts::PI) * h as f64 - 0.5).clamp(0.0, (h - 1) as f64);
    let (y0, a) = (row.floor() as usize, (row.fract()) as f32);
    let y1 = (y0 + 1).min(h - 1);
    luma(texels[y0 * w]) * (1.0 - a) + luma(texels[y1 * w]) * a
}

#[test]
fn the_probe_holds_the_sky_the_plate_saw() {
    let probe = probe(512);
    let table: Vec<f32> = (0..=90).map(|d| sky((d as f64).to_radians())).collect();
    let mut ratios = Vec::new();
    for y in 0..probe.height {
        for x in 0..probe.width {
            let i = y * probe.width + x;
            let elevation = direction(probe.width, probe.height, x, y).y.asin();
            if probe.coverage[i] >= 1.0 && elevation > 2f64.to_radians() {
                let sky = table[elevation.to_degrees().round() as usize];
                ratios.push(luma(probe.texels[i]) / sky);
            }
        }
    }
    assert!(ratios.len() > 2000, "the plate saw sky in {} texels", ratios.len());
    let m = median(ratios);
    assert!((m - 1.0).abs() < 0.02, "the probe's sky over the true sky: {m}");
}

#[test]
fn the_probe_fills_what_no_frame_saw() {
    let probe = probe(512);
    let seen = probe.coverage.iter().filter(|&&c| c > 0.0).count();
    assert!(seen > 0 && seen < probe.texels.len() / 2, "the plate covers {seen} texels");
    assert!(probe.texels.iter().all(|c| c.iter().all(|v| v.is_finite() && *v > 0.0)), "every texel filled");
    let zenith = luma(probe.texels[probe.width / 2]);
    let nadir = luma(probe.texels[(probe.height - 1) * probe.width + probe.width / 2]);
    assert!(zenith > 0.5, "the sky above the shot continues the plate's sky: {zenith}");
    assert!(nadir < zenith, "the ground below the shot stays darker than the sky: {nadir} against {zenith}");
}

/// Frame `k` of the plate as linear RGB.
fn plate_frame(k: usize) -> Vec<[f32; 3]> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(fixture("pan.mp4"))
        .args(["-vf", &format!("select=eq(n\\,{k})"), "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .unwrap();
    assert_eq!(out.stdout.len(), 480 * 270 * 3);
    out.stdout.as_chunks::<3>().0.iter().map(|p| p.map(linear)).collect()
}

/// The probe seen through frame `k`'s camera, sampling the nearest texel.
fn reproject(probe: &Probe, solve: &CameraSolve, k: usize) -> Vec<[f32; 3]> {
    let r = solve.frames[k].rotation.map(|v| v as f64);
    let q = UnitQuaternion::from_quaternion(Quaternion::new(r[3], r[0], r[1], r[2]));
    let f = 135.0 / (solve.yfov / 2.0).tan();
    let mut out = Vec::with_capacity(480 * 270);
    for py in 0..270 {
        for px in 0..480 {
            let d = q * nalgebra::Vector3::new((px as f64 - 239.5) / f, -(py as f64 - 134.5) / f, -1.0).normalize();
            let u = 0.5 + d.x.atan2(-d.z) / std::f64::consts::TAU;
            let v = d.y.clamp(-1.0, 1.0).acos() / std::f64::consts::PI;
            let x = ((u * probe.width as f64) as usize).min(probe.width - 1);
            let y = ((v * probe.height as f64) as usize).min(probe.height - 1);
            out.push(probe.texels[y * probe.width + x]);
        }
    }
    out
}

#[test]
fn the_probe_reprojects_onto_its_plate() {
    let probe = probe(2048);
    let solve = pan();
    for k in [10, 45] {
        let (plate, seen) = (plate_frame(k), reproject(&probe, &solve, k));
        let (a, b): (Vec<f64>, Vec<f64>) = plate.iter().zip(&seen).map(|(p, s)| (luma(*p) as f64, luma(*s) as f64)).unzip();
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let (ma, mb) = (mean(&a), mean(&b));
        let cov: f64 = a.iter().zip(&b).map(|(x, y)| (x - ma) * (y - mb)).sum();
        let (va, vb): (f64, f64) = (a.iter().map(|x| (x - ma).powi(2)).sum(), b.iter().map(|y| (y - mb).powi(2)).sum());
        let correlation = cov / (va * vb).sqrt();
        assert!(correlation > 0.95, "frame {k}: the probe's view correlates {correlation:.3} with the plate");
        let mut worst = 0.0f32;
        for by in 0..9 {
            for bx in 0..16 {
                let mut d = [0.0f32; 3];
                for y in by * 30..by * 30 + 30 {
                    for x in bx * 30..bx * 30 + 30 {
                        for c in 0..3 {
                            d[c] += plate[y * 480 + x][c] - seen[y * 480 + x][c];
                        }
                    }
                }
                worst = worst.max(d.iter().map(|v| (v / 900.0).abs()).fold(0.0, f32::max));
            }
        }
        assert!(worst < 0.025, "frame {k}: the worst 30 px block differs by {worst:.4} in linear light");
    }
}

/// Decodes a run-length Radiance file of `Probe::hdr`'s layout.
fn read_hdr(bytes: &[u8]) -> (usize, usize, Vec<[f32; 3]>) {
    let text_end = bytes.windows(2).position(|w| w == b"\n\n").unwrap() + 2;
    let line_end = text_end + bytes[text_end..].iter().position(|&b| b == b'\n').unwrap();
    let size: Vec<usize> =
        std::str::from_utf8(&bytes[text_end..line_end]).unwrap().split_whitespace().filter_map(|s| s.parse().ok()).collect();
    let (h, w) = (size[0], size[1]);
    let mut at = line_end + 1;
    let mut texels = Vec::with_capacity(w * h);
    for _ in 0..h {
        assert_eq!(&bytes[at..at + 4], &[2, 2, (w >> 8) as u8, (w & 0xff) as u8]);
        at += 4;
        let mut channels: [Vec<u8>; 4] = std::array::from_fn(|_| Vec::with_capacity(w));
        for channel in &mut channels {
            while channel.len() < w {
                let n = bytes[at] as usize;
                if n > 128 {
                    channel.extend(std::iter::repeat_n(bytes[at + 1], n - 128));
                    at += 2;
                } else {
                    channel.extend_from_slice(&bytes[at + 1..at + 1 + n]);
                    at += 1 + n;
                }
            }
        }
        for (x, &e) in channels[3].iter().enumerate() {
            let scale = if e == 0 { 0.0 } else { 2f32.powi(e as i32 - 136) };
            texels.push([0, 1, 2].map(|c| (channels[c][x] as f32 + 0.5) * scale));
        }
    }
    assert_eq!(at, bytes.len());
    (w, h, texels)
}

#[test]
fn the_probe_writes_a_radiance_file() {
    let probe = probe(64);
    let (w, h, texels) = read_hdr(&probe.hdr());
    assert_eq!((w, h), (64, 32));
    for (a, b) in probe.texels.iter().zip(&texels) {
        let peak = a.iter().fold(0.0f32, |m, v| m.max(*v));
        for c in 0..3 {
            assert!((a[c] - b[c]).abs() <= peak / 128.0, "{a:?} encodes as {b:?}");
        }
    }
}

#[test]
fn a_probe_from_missing_footage_fails() {
    let error = light_probe(&fixture("no-such.mp4"), &pan(), 64).err().unwrap();
    assert!(error.contains("no-such.mp4"), "{error}");
}
