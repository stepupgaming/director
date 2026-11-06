//! The sun read from the shadows the solved scene casts on the ground.

use std::path::Path;

use nalgebra::Vector3;
use rayon::prelude::*;

use crate::probe::{bilinear, stream, Probe, View};
use crate::solve::CameraSolve;

/// Scene points lower than this many metres stand on the ground and cast
/// no shadow worth reading.
const MIN_HEIGHT: f64 = 0.15;
/// The fewest standing points a sun estimate reads.
const MIN_POINTS: usize = 20;
/// The lowest sun the search tries, in degrees; lower suns throw shadows
/// beyond the ground the footage maps.
const MIN_ELEVATION: f64 = 10.0;
/// The most texels along the longer side of the ground map.
const MAP: usize = 2048;
/// The finest ground map texel in metres.
const TEXEL: f64 = 0.03;
/// A ground texel whose samples' luma deviates by more than this fraction
/// of their mean saw something other than one patch of ground.
const SPREAD: f32 = 0.3;
/// The width in metres of the column each standing point stands for.
const SPLAT: f64 = 0.3;
/// A standing point whose luma spreads by more than this fraction of its
/// mean across the frames is a phantom of crossing edges.
const CONSISTENT: f64 = 0.15;
/// A ground sample counts as hidden when a standing column lies this
/// fraction of its depth nearer the camera.
const OCCLUDED: f64 = 0.05;
/// The texel in metres of the map the search reads.
const SEARCH_TEXEL: f64 = 0.08;
/// The width in metres of the lit ground around each shadow the search
/// compares it with.
const BAND: f64 = 0.5;
/// The width in metres of the ring around each column's foot that reads
/// as neither shadow nor lit ground: the contact shade there darkens
/// under every sun.
const CONTACT: f64 = 0.2;
/// The shaded area in square metres at which the search trusts a shadow's
/// contrast by half.
const SHRINK: f64 = 1.28;
/// The radius in metres of the shadow core and the gap around the shadow
/// the strength reads, purer than the search's regions.
const CORE: f64 = SPLAT / 4.0;
const GAP: f64 = 0.3;

/// The sun's direction and strength in the solve's world.
#[derive(Clone, Debug)]
pub struct Sun {
    /// Unit vector towards the sun.
    pub direction: [f64; 3],
    /// Linear RGB irradiance on a surface facing the sun, in the plate's
    /// linear light: a white matte card facing it reads `irradiance / pi`
    /// brighter than in its shadow.
    pub irradiance: [f64; 3],
    /// How much darker the ground at the shadows the solve's points cast
    /// reads than the ground around them, as a ratio from 0 (black) to 1
    /// (no shadow).
    pub shadow: [f64; 3],
}

/// A top-down map of the ground (y = 0) in linear light: each texel is the
/// mean colour the frames saw there, `None` where they disagree.
struct Ground {
    min: [f64; 2],
    texel: f64,
    width: usize,
    height: usize,
    /// Log linear RGB.
    texels: Vec<Option<[f32; 3]>>,
}

/// A solid column standing on the ground at (x, z), `SPLAT` metres wide.
struct Column {
    x: f64,
    z: f64,
    height: f64,
}

/// Estimates the sun from the footage at `path` through the cameras and
/// points of `solve`, with `probe` lighting the shade. The solve's world
/// must stand on the ground at y = 0 in metres, as a moving camera's solve
/// does. Every frame's ground is mapped from above, and the points standing
/// off the ground stand for solid columns. The sun's direction is the one
/// whose shadows of those columns fall on ground darkest against the ground
/// around them, so shadows cast too long or too short both read weaker. Its
/// strength is what brightens the lit ground over those shadows above the
/// sky's own irradiance from `probe`. Shadowed ground still sees most of
/// the sky, so the strength reads a little high where tall things crowd
/// the shade.
pub fn estimate_sun(path: &Path, solve: &CameraSolve, probe: &Probe) -> Result<Sun, String> {
    let standing: Vec<Vector3<f64>> = solve
        .points
        .iter()
        .map(|p| Vector3::new(p[0] as f64, p[1] as f64, p[2] as f64))
        .filter(|p| p.y > MIN_HEIGHT && p.y < 50.0 && p.x.hypot(p.z) < 100.0)
        .collect();
    if standing.len() < MIN_POINTS {
        return Err(format!(
            "reading the sun needs {MIN_POINTS} solved points standing off the ground, the solve has {}; a tripod shot has none",
            standing.len()
        ));
    }
    let spread = consistency(path, solve, &standing)?;
    let hiding = columns(&standing);
    let standing: Vec<Vector3<f64>> = standing
        .into_iter()
        .zip(spread)
        .filter(|(_, (spread, seen))| *spread < CONSISTENT && *seen >= 3)
        .map(|(p, _)| p)
        .collect();
    if standing.len() < MIN_POINTS {
        return Err(format!(
            "reading the sun needs {MIN_POINTS} solved points off the ground that keep one colour across the footage, the solve has {}",
            standing.len()
        ));
    }
    let columns = columns(&standing);
    let fine = map_ground(path, solve, &hiding)?;
    let ground = fine.coarsen((SEARCH_TEXEL / fine.texel).round().max(1.0) as usize);
    if ground.texels.iter().flatten().count() < 100 {
        return Err("the footage shows too little steady ground to read the sun's shadows".into());
    }
    let towards = |azimuth: f64, elevation: f64| {
        let (a, e) = (azimuth.to_radians(), elevation.to_radians());
        Vector3::new(e.cos() * a.sin(), e.sin(), -e.cos() * a.cos())
    };
    let score = |map: &Ground, s: &Vector3<f64>| {
        shrunk(&map.shade(&columns, &[], s, SPLAT / 2.0, 0.0), map.texel)
    };
    let search =
        |map: &Ground, azimuths: Vec<f64>, elevations: Vec<f64>| -> Option<(f64, f64, f64)> {
            azimuths
                .par_iter()
                .flat_map_iter(|&a| elevations.iter().map(move |&e| (a, e)))
                .filter_map(|(a, e)| score(map, &towards(a, e)).map(|t| (a, e, t)))
                .min_by(|x, y| x.2.total_cmp(&y.2))
        };
    let steps = |from: f64, to: f64, step: f64| -> Vec<f64> {
        (0..=((to - from) / step).round() as usize)
            .map(|i| from + i as f64 * step)
            .collect()
    };
    let (a, e, _) = search(
        &ground,
        steps(0.0, 357.0, 3.0),
        steps(MIN_ELEVATION, 87.0, 3.0),
    )
    .ok_or("the shadows of the solved points fall off the mapped ground for every sun")?;
    let (a, e, _) = search(
        &fine,
        steps(a - 3.0, a + 3.0, 0.5),
        steps((e - 3.0).max(MIN_ELEVATION), (e + 3.0).min(89.0), 0.5),
    )
    .expect("the coarse best is among the fine candidates");
    let direction = towards(a, e);
    let shadow = ratio(&fine.shade(&columns, &hiding, &direction, CORE, GAP))
        .ok_or("the best sun's shadows fall on too little mapped ground")?;
    let sky = probe.irradiance(Vector3::y());
    let irradiance = std::array::from_fn(|c| {
        (sky[c] * (1.0 / shadow[c].max(1e-3) - 1.0) / direction.y).max(0.0)
    });
    Ok(Sun {
        direction: [direction.x, direction.y, direction.z],
        irradiance,
        shadow,
    })
}

/// Each point's luma spread over its mean across the sampled frames that
/// frame it, and how many frames did. A point on a surface keeps its colour
/// from frame to frame; a track caught where a near edge crosses the ground
/// behind it triangulates to a phantom in the air, which shows whatever lies
/// along each frame's ray.
fn consistency(path: &Path, solve: &CameraSolve, points: &[Vector3<f64>]) -> Result<Vec<(f64, u32)>, String> {
    let mut sums = vec![[0.0f64; 3]; points.len()];
    stream(path, solve, |view, image| {
        sums.par_iter_mut().zip(points.par_iter()).for_each(|(s, p)| {
            let c = view.rotation * (p - view.position);
            let Some((px, py, _)) = view.at(&c, 2.0) else {
                return;
            };
            let color = bilinear(image, view.width, view.height, px, py);
            let luma = (0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2]) as f64;
            *s = [s[0] + luma, s[1] + luma * luma, s[2] + 1.0];
        });
    })?;
    Ok(sums
        .iter()
        .map(|s| {
            let n = s[2].max(1.0);
            let mean = s[0] / n;
            ((s[1] / n - mean * mean).max(0.0).sqrt() / mean.max(1e-6), s[2] as u32)
        })
        .collect())
}

/// The standing points as columns: each `SPLAT`-wide cell of the ground
/// holds one column up to its highest point.
fn columns(standing: &[Vector3<f64>]) -> Vec<Column> {
    let mut cells: std::collections::HashMap<(i64, i64), Column> = std::collections::HashMap::new();
    for p in standing {
        let key = ((p.x / SPLAT).floor() as i64, (p.z / SPLAT).floor() as i64);
        let column = cells.entry(key).or_insert(Column {
            x: p.x,
            z: p.z,
            height: 0.0,
        });
        if p.y > column.height {
            *column = Column {
                x: p.x,
                z: p.z,
                height: p.y,
            };
        }
    }
    let mut columns: Vec<Column> = cells.into_values().collect();
    columns.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.z.total_cmp(&b.z)));
    columns
}

impl Ground {
    /// The map averaged over `k` by `k` blocks, each block kept where at
    /// least half its texels are.
    fn coarsen(&self, k: usize) -> Ground {
        let (width, height) = (self.width.div_ceil(k), self.height.div_ceil(k));
        let texels = (0..width * height)
            .into_par_iter()
            .map(|i| {
                let (bx, by) = (i % width * k, i / width * k);
                let mut sum = [0.0f32; 3];
                let mut n = 0;
                for y in by..(by + k).min(self.height) {
                    for x in bx..(bx + k).min(self.width) {
                        if let Some(c) = self.texels[y * self.width + x] {
                            for ch in 0..3 {
                                sum[ch] += c[ch];
                            }
                            n += 1;
                        }
                    }
                }
                (2 * n >= k * k).then(|| sum.map(|v| v / n as f32))
            })
            .collect();
        Ground {
            min: self.min,
            texel: self.texel * k as f64,
            width,
            height,
            texels,
        }
    }

    /// The mapped ground around the columns under a sun towards `s`, each
    /// texel flagged true where the columns shade it and false in the band
    /// `BAND` metres wide around those shadows. The feet of the columns and
    /// of `feet`, with the contact shade around them, count as neither.
    /// Empty when less than a quarter of the shadows falls on mapped ground.
    fn shade(
        &self,
        columns: &[Column],
        feet: &[Column],
        s: &Vector3<f64>,
        radius: f64,
        gap: f64,
    ) -> Vec<(bool, [f32; 3])> {
        const LIT: u8 = 1;
        const DARK: u8 = 2;
        const FOOT: u8 = 4;
        const GAP: u8 = 8;
        let mut labels = vec![0u8; self.width * self.height];
        let mut touched = Vec::new();
        let (dx, dz) = (-s.x / s.y, -s.z / s.y);
        for c in columns {
            let (base, tip) = ((c.x, c.z), (c.x + c.height * dx, c.z + c.height * dz));
            self.capsule(
                base,
                tip,
                SPLAT / 2.0 + gap + BAND,
                LIT,
                &mut labels,
                &mut touched,
            );
            if gap > 0.0 {
                self.capsule(base, tip, SPLAT / 2.0 + gap, GAP, &mut labels, &mut touched);
            }
            self.capsule(base, tip, radius, DARK, &mut labels, &mut touched);
            self.capsule(
                base,
                base,
                SPLAT / 2.0 + CONTACT,
                FOOT,
                &mut labels,
                &mut touched,
            );
        }
        for c in feet {
            self.capsule(
                (c.x, c.z),
                (c.x, c.z),
                SPLAT / 2.0 + CONTACT,
                FOOT,
                &mut labels,
                &mut touched,
            );
        }
        let (mut texels, mut cast) = (Vec::new(), 0usize);
        for &i in &touched {
            let label = labels[i];
            if label & FOOT != 0 || label & (GAP | DARK) == GAP {
                continue;
            }
            cast += usize::from(label & DARK != 0);
            if let Some(color) = self.texels[i] {
                texels.push((label & DARK != 0, color));
            }
        }
        if texels.iter().filter(|t| t.0).count() * 4 < cast {
            texels.clear();
        }
        texels
    }

    /// Marks with `label` every texel within `radius` metres of the segment
    /// from `a` to `b`, noting each texel it marks first in `touched`.
    fn capsule(
        &self,
        a: (f64, f64),
        b: (f64, f64),
        radius: f64,
        label: u8,
        labels: &mut [u8],
        touched: &mut Vec<usize>,
    ) {
        let u = |x: f64| (x - self.min[0]) / self.texel;
        let v = |z: f64| (z - self.min[1]) / self.texel;
        let clamp = |t: f64, n: usize| (t.max(0.0) as usize).min(n);
        let (u0, u1) = (
            clamp(u(a.0.min(b.0) - radius).floor(), self.width),
            clamp(u(a.0.max(b.0) + radius).ceil(), self.width),
        );
        let (v0, v1) = (
            clamp(v(a.1.min(b.1) - radius).floor(), self.height),
            clamp(v(a.1.max(b.1) + radius).ceil(), self.height),
        );
        let (ex, ez) = (b.0 - a.0, b.1 - a.1);
        let length = ex * ex + ez * ez;
        for tv in v0..v1 {
            let z = self.min[1] + (tv as f64 + 0.5) * self.texel;
            for tu in u0..u1 {
                let x = self.min[0] + (tu as f64 + 0.5) * self.texel;
                let t = if length > 0.0 {
                    (((x - a.0) * ex + (z - a.1) * ez) / length).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (px, pz) = (a.0 + t * ex - x, a.1 + t * ez - z);
                if px * px + pz * pz <= radius * radius {
                    let i = tv * self.width + tu;
                    if labels[i] == 0 {
                        touched.push(i);
                    }
                    labels[i] |= label;
                }
            }
        }
    }
}

fn luma(c: [f32; 3]) -> f64 {
    0.2126 * c[0] as f64 + 0.7152 * c[1] as f64 + 0.0722 * c[2] as f64
}

/// The mean log colour of the shaded texels less that of the lit ones;
/// `None` when either side holds fewer than 20 texels.
fn contrast(texels: &[(bool, [f32; 3])]) -> Option<[f64; 3]> {
    let mean = |dark: bool| {
        let (mut sum, mut n) = ([0.0f64; 3], 0usize);
        for t in texels.iter().filter(|t| t.0 == dark) {
            for (s, v) in sum.iter_mut().zip(t.1) {
                *s += v as f64;
            }
            n += 1;
        }
        (sum.map(|v| v / n.max(1) as f64), n)
    };
    let ((dark, nd), (lit, nl)) = (mean(true), mean(false));
    (nd >= 20 && nl >= 20).then(|| std::array::from_fn(|c| dark[c] - lit[c]))
}

/// How much darker the shaded texels read than the lit ones per channel,
/// at most 1; `None` when either side holds fewer than 20 texels. The
/// median log luma of each side sets the strength, which holds where the
/// predicted shadows stray onto lit ground, and the mean log colours set
/// the tint, which the ground's own colours would skew in a per-channel
/// median.
fn ratio(texels: &[(bool, [f32; 3])]) -> Option<[f64; 3]> {
    let median = |dark: bool| {
        let mut v: Vec<f64> = texels
            .iter()
            .filter(|t| t.0 == dark)
            .map(|t| luma(t.1))
            .collect();
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let mean = contrast(texels)?;
    let tint = luma(mean.map(|v| v as f32));
    let strength = median(true) - median(false);
    Some(mean.map(|c| (strength + c - tint).exp().min(1.0)))
}

/// The contrast's luma shrunk towards no shadow by the shaded area against
/// `SHRINK`, so a few dark texels under a high sun weigh less than
/// a long shadow.
fn shrunk(texels: &[(bool, [f32; 3])], texel: f64) -> Option<f64> {
    let n = texels.iter().filter(|t| t.0).count() as f64 * texel * texel;
    contrast(texels).map(|c| luma(c.map(|v| v as f32)) * n / (n + SHRINK))
}

/// The depth of the nearest standing column over each pixel of `view`, so
/// ground behind the things the points stand for drops out of the map.
fn occluders(view: &View, columns: &[Column]) -> Vec<f64> {
    let (w, h) = (view.width, view.height);
    let mut depth = vec![f64::INFINITY; w * h];
    for p in columns {
        let steps = (p.height / (SPLAT / 2.0)).ceil() as usize;
        for k in 0..=steps {
            let c = view.rotation
                * (Vector3::new(p.x, p.height * k as f64 / steps as f64, p.z) - view.position);
            let Some((px, py, _)) = view.at(&c, -(w.max(h) as f64)) else {
                continue;
            };
            let r = SPLAT / 2.0 * view.f / -c.z;
            let (x0, x1) = (
                (px - r).floor().max(0.0) as usize,
                ((px + r).ceil().max(0.0) as usize).min(w - 1),
            );
            let (y0, y1) = (
                (py - r).floor().max(0.0) as usize,
                ((py + r).ceil().max(0.0) as usize).min(h - 1),
            );
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if (x as f64 - px).hypot(y as f64 - py) <= r {
                        let d = &mut depth[y * w + x];
                        *d = d.min(-c.z);
                    }
                }
            }
        }
    }
    depth
}

/// Maps the ground around the columns far enough out to hold the shadows
/// of the lowest sun the search tries and the band around them.
fn map_ground(path: &Path, solve: &CameraSolve, columns: &[Column]) -> Result<Ground, String> {
    let reach = columns
        .iter()
        .map(|p| p.height)
        .fold(0.0, f64::max)
        .min(10.0)
        / MIN_ELEVATION.to_radians().tan()
        + BAND
        + SPLAT;
    let (mut min, mut max) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in columns {
        min = [min[0].min(p.x - reach), min[1].min(p.z - reach)];
        max = [max[0].max(p.x + reach), max[1].max(p.z + reach)];
    }
    let texel = ((max[0] - min[0]).max(max[1] - min[1]) / MAP as f64).max(TEXEL);
    let (width, height) = (
        ((max[0] - min[0]) / texel).ceil() as usize,
        ((max[1] - min[1]) / texel).ceil() as usize,
    );
    // Per texel: the sum of linear RGB, the sum of squared luma and the count.
    let mut sums = vec![[0.0f32; 5]; width * height];
    stream(path, solve, |view, image| {
        let depth = occluders(view, columns);
        sums.par_iter_mut().enumerate().for_each(|(i, s)| {
            let world = Vector3::new(
                min[0] + ((i % width) as f64 + 0.5) * texel,
                0.0,
                min[1] + ((i / width) as f64 + 0.5) * texel,
            );
            let c = view.rotation * (world - view.position);
            let Some((px, py, _)) = view.at(&c, 1.0) else {
                return;
            };
            if depth[py.round() as usize * view.width + px.round() as usize]
                < -c.z * (1.0 - OCCLUDED)
            {
                return;
            }
            let color = bilinear(image, view.width, view.height, px, py);
            let luma = 0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2];
            *s = [
                s[0] + color[0],
                s[1] + color[1],
                s[2] + color[2],
                s[3] + luma * luma,
                s[4] + 1.0,
            ];
        });
    })?;
    let texels = sums
        .par_iter()
        .map(|s| {
            let n = s[4];
            if n < 3.0 {
                return None;
            }
            let mean = [s[0] / n, s[1] / n, s[2] / n];
            let luma = 0.2126 * mean[0] + 0.7152 * mean[1] + 0.0722 * mean[2];
            let spread = (s[3] / n - luma * luma).max(0.0).sqrt();
            (luma > 1e-3 && spread / luma < SPREAD).then(|| mean.map(f32::ln))
        })
        .collect();
    Ok(Ground {
        min,
        texel,
        width,
        height,
        texels,
    })
}
