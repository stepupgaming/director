//! A light probe stitched from footage through its solved cameras.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use nalgebra::{Quaternion, UnitQuaternion, Vector3};
use rayon::prelude::*;

use crate::solve::CameraSolve;

/// The most frames a probe stitches; longer footage takes every n-th frame.
const MAX_FRAMES: usize = 64;
/// The width the footage decodes at for stitching.
const DECODE_WIDTH: u32 = 960;
/// How far in from the frame edge, as a fraction of the frame height, a
/// pixel takes its full weight, so seams between frames blend.
const FEATHER: f64 = 0.15;

/// An equirectangular light probe in linear light: texel (x, y) looks along
/// `direction(x, y)`, the convention of Director's environment maps (+Y up,
/// u = 0.5 looking down -Z, u growing towards +X).
pub struct Probe {
    pub width: usize,
    pub height: usize,
    /// Linear RGB, every texel filled.
    pub texels: Vec<[f32; 3]>,
    /// How much footage each texel saw before the fill, 0 for none and 1
    /// or more where a frame's interior covered it.
    pub coverage: Vec<f32>,
}

/// The world direction of the centre of texel (x, y) of a `width` by
/// `height` equirectangular map.
pub fn direction(width: usize, height: usize, x: usize, y: usize) -> Vector3<f64> {
    let u = (x as f64 + 0.5) / width as f64;
    let v = (y as f64 + 0.5) / height as f64;
    let (phi, theta) = ((u - 0.5) * std::f64::consts::TAU, v * std::f64::consts::PI);
    Vector3::new(theta.sin() * phi.sin(), theta.cos(), -theta.sin() * phi.cos())
}

/// sRGB-encoded 8-bit to linear light.
pub fn linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Stitches the frames of the footage at `path` into a `width` by
/// `width / 2` equirectangular probe through the cameras of `solve`. Each
/// texel averages the linear colour every frame sees along its direction,
/// weighted to fade out towards the frame edges; directions no frame saw
/// take the colour of the nearest seen ones by a pull-push fill, so the sky
/// above the shot continues the top of the plate. The footage's directions
/// sit at infinity, so a camera that travels blurs nearby things across the
/// probe, which a light probe tolerates.
pub fn light_probe(path: &Path, solve: &CameraSolve, width: usize) -> Result<Probe, String> {
    if width < 8 || !width.is_multiple_of(2) {
        return Err(format!("a light probe needs an even width of at least 8 texels, got {width}"));
    }
    let height = width / 2;
    let n = solve.frames.len();
    if n == 0 {
        return Err("a light probe needs a solved camera".into());
    }
    let (w, h) = if solve.width > DECODE_WIDTH {
        (DECODE_WIDTH, ((solve.height as f64 * DECODE_WIDTH as f64 / solve.width as f64).round() as u32).max(1))
    } else {
        (solve.width, solve.height)
    };
    let stride = n.div_ceil(MAX_FRAMES);
    let directions: Vec<Vector3<f64>> = (0..width * height).map(|i| direction(width, height, i % width, i / width)).collect();
    let mut sum = vec![[0.0f64; 4]; width * height];
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-vf", &format!("scale={w}:{h}:flags=area"), "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("running ffmpeg: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("ffmpeg gave no output")?;
    let (fw, fh) = (w as usize, h as usize);
    let mut frame = vec![0u8; fw * fh * 3];
    let mut k = 0;
    let mut stitched = 0;
    while k < n && stdout.read_exact(&mut frame).is_ok() {
        if k.is_multiple_of(stride) {
            let pose = &solve.frames[k];
            let r = pose.rotation.map(|v| v as f64);
            let q = UnitQuaternion::from_quaternion(Quaternion::new(r[3], r[0], r[1], r[2])).inverse();
            let yfov = pose.yfov.unwrap_or(solve.yfov);
            let f = (fh as f64 / 2.0) / (yfov / 2.0).tan();
            let (cx, cy) = ((fw as f64 - 1.0) / 2.0, (fh as f64 - 1.0) / 2.0);
            let [k1, k2] = solve.distortion;
            let feather = FEATHER * fh as f64;
            let image = &frame;
            sum.par_iter_mut().zip(directions.par_iter()).for_each(|(s, d)| {
                let c = q * d;
                if c.z >= -1e-6 {
                    return;
                }
                let (a, b) = (c.x / -c.z, -c.y / -c.z);
                let r2 = a * a + b * b;
                if r2 > 9.0 {
                    return;
                }
                let scale = f * (1.0 + k1 * r2 + k2 * r2 * r2);
                let (px, py) = (cx + scale * a, cy + scale * b);
                if !(0.0..=(fw - 1) as f64).contains(&px) || !(0.0..=(fh - 1) as f64).contains(&py) {
                    return;
                }
                let edge = px.min(py).min((fw - 1) as f64 - px).min((fh - 1) as f64 - py);
                let weight = (edge / feather).min(1.0).powi(2);
                if weight <= 0.0 {
                    return;
                }
                let color = bilinear(image, fw, fh, px, py);
                for ch in 0..3 {
                    s[ch] += weight * color[ch] as f64;
                }
                s[3] += weight;
            });
            stitched += 1;
        }
        k += 1;
    }
    drop(stdout);
    let status = child.wait().map_err(|e| format!("running ffmpeg: {e}"))?;
    if stitched == 0 {
        return Err(if status.success() {
            format!("{} decoded no frames", path.display())
        } else {
            format!("ffmpeg could not decode {}", path.display())
        });
    }
    let coverage: Vec<f32> = sum.iter().map(|s| s[3] as f32).collect();
    if coverage.iter().all(|&c| c <= 0.0) {
        return Err("the footage covers no direction of the light probe".into());
    }
    let texels = fill(&sum, width, height);
    Ok(Probe { width, height, texels, coverage })
}

/// The linear colour at a sub-pixel position of an RGB8 frame.
fn bilinear(image: &[u8], w: usize, h: usize, x: f64, y: f64) -> [f32; 3] {
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (ax, ay) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
    let at = |x: usize, y: usize, c: usize| linear(image[(y * w + x) * 3 + c]);
    let mut out = [0.0; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let top = at(x0, y0, c) * (1.0 - ax) + at(x1, y0, c) * ax;
        let bottom = at(x0, y1, c) * (1.0 - ax) + at(x1, y1, c) * ax;
        *o = top * (1.0 - ay) + bottom * ay;
    }
    out
}

/// Pull-push hole filling: each level halves the map and sums the weighted
/// colour and weight of four texels, and on the way back each texel keeps
/// its own average in proportion to its weight (full at 1) and takes the
/// coarser level's for the rest.
fn fill(sum: &[[f64; 4]], width: usize, height: usize) -> Vec<[f32; 3]> {
    let mut levels = vec![(width, height, sum.to_vec())];
    while levels.last().is_some_and(|(w, h, _)| *w > 2 && *h > 1) {
        let (w, h, fine) = levels.last().unwrap();
        let (cw, ch) = (w / 2, h / 2);
        let mut coarse = vec![[0.0; 4]; cw * ch];
        for y in 0..ch {
            for x in 0..cw {
                let c = &mut coarse[y * cw + x];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let s = fine[(2 * y + dy) * w + 2 * x + dx];
                    for k in 0..4 {
                        c[k] += s[k];
                    }
                }
            }
        }
        levels.push((cw, ch, coarse));
    }
    let (cw, ch, top) = levels.pop().unwrap();
    let total = top.iter().fold([0.0; 4], |mut a, s| {
        for k in 0..4 {
            a[k] += s[k];
        }
        a
    });
    let mean = [total[0] / total[3], total[1] / total[3], total[2] / total[3]];
    let mut color: Vec<[f64; 3]> = top.iter().map(|s| blend(s, mean)).collect();
    let mut cur = (cw, ch);
    while let Some((w, h, fine)) = levels.pop() {
        let coarse = color;
        let (pw, ph) = cur;
        color = (0..w * h)
            .map(|i| blend(&fine[i], coarse[(i / w / 2).min(ph - 1) * pw + ((i % w) / 2).min(pw - 1)]))
            .collect();
        cur = (w, h);
    }
    color.into_iter().map(|c| c.map(|v| v as f32)).collect()
}

fn blend(s: &[f64; 4], under: [f64; 3]) -> [f64; 3] {
    if s[3] <= 0.0 {
        return under;
    }
    let a = s[3].min(1.0);
    std::array::from_fn(|k| a * s[k] / s[3] + (1.0 - a) * under[k])
}

impl Probe {
    /// The probe as a Radiance `.hdr` file with run-length scanlines.
    pub fn hdr(&self) -> Vec<u8> {
        let mut out = format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y {} +X {}\n", self.height, self.width).into_bytes();
        let mut channels = vec![[0u8; 4]; self.width];
        for row in self.texels.chunks_exact(self.width) {
            for (rgbe, c) in channels.iter_mut().zip(row) {
                *rgbe = rgbe_of(*c);
            }
            out.extend_from_slice(&[2, 2, (self.width >> 8) as u8, (self.width & 0xff) as u8]);
            for ch in 0..4 {
                let bytes: Vec<u8> = channels.iter().map(|p| p[ch]).collect();
                for chunk in bytes.chunks(128) {
                    out.push(chunk.len() as u8);
                    out.extend_from_slice(chunk);
                }
            }
        }
        out
    }
}

/// A linear colour as shared-exponent RGBE.
fn rgbe_of(c: [f32; 3]) -> [u8; 4] {
    let c = c.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 });
    let v = c[0].max(c[1]).max(c[2]);
    if v < 1e-32 {
        return [0; 4];
    }
    let e = v.log2().floor() as i32 + 1;
    let scale = 256.0 / 2f32.powi(e);
    let byte = |x: f32| (x * scale).min(255.0) as u8;
    [byte(c[0]), byte(c[1]), byte(c[2]), (e + 128).clamp(0, 255) as u8]
}
