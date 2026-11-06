//! A clear sky from the sun's direction: air (Rayleigh) and haze (Mie)
//! scattering with ozone absorption over a planet the size of the Earth,
//! single scattering of the sun plus Hillaire's multiple scattering term,
//! rendered into an equirectangular probe over a Lambertian ground. Values
//! are radiance per unit of solar irradiance above the atmosphere.

use rayon::prelude::*;
use std::f64::consts::PI;

const GROUND: f64 = 6_360_000.0;
const TOP: f64 = 6_460_000.0;
const RAYLEIGH: [f64; 3] = [5.802e-6, 13.558e-6, 33.1e-6];
const RAYLEIGH_HEIGHT: f64 = 8000.0;
const MIE_SCATTER: f64 = 3.996e-6;
const MIE_ABSORB: f64 = 4.4e-6;
const MIE_HEIGHT: f64 = 1200.0;
const MIE_G: f64 = 0.8;
const OZONE: [f64; 3] = [0.650e-6, 1.881e-6, 0.085e-6];
/// The viewer's height above the ground in metres.
const EYE: f64 = 2.0;
const VIEW_STEPS: usize = 40;
const TRANSMITTANCE: (usize, usize) = (256, 64);
const MULTIPLE: usize = 32;

/// The sky's inputs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyOptions {
    /// The sun's height above the horizon in degrees.
    pub elevation: f64,
    /// The sun's bearing in degrees about +Y, 0 on +Z (towards the framing
    /// camera) and 90 on +X.
    pub azimuth: f64,
    /// Haze density as a multiple of a clear day's.
    pub haze: f64,
    /// The ground's linear albedo.
    pub ground: [f64; 3],
}

/// A rendered sky.
#[derive(Clone, Debug)]
pub struct Sky {
    pub width: usize,
    pub height: usize,
    /// Linear RGB radiance per unit of solar irradiance, row by row from the
    /// zenith, in the direction convention of `director_track::direction`.
    pub texels: Vec<[f32; 3]>,
    /// The unit direction towards the sun.
    pub sun: [f64; 3],
    /// The sun's linear irradiance at the viewer per unit above the
    /// atmosphere, 0 once it sets.
    pub irradiance: [f64; 3],
}

/// The world direction of the centre of texel (x, y) of a `width` by
/// `height` equirectangular map, as `director_track::direction` gives it.
pub fn texel_direction(width: usize, height: usize, x: usize, y: usize) -> [f64; 3] {
    let u = (x as f64 + 0.5) / width as f64;
    let v = (y as f64 + 0.5) / height as f64;
    let (phi, theta) = ((u - 0.5) * 2.0 * PI, v * PI);
    [theta.sin() * phi.sin(), theta.cos(), -theta.sin() * phi.cos()]
}

/// The unit direction towards a sun `elevation` degrees up at `azimuth`.
pub fn sun_direction(elevation: f64, azimuth: f64) -> [f64; 3] {
    let (e, a) = (elevation.to_radians(), azimuth.to_radians());
    [e.cos() * a.sin(), e.sin(), e.cos() * a.cos()]
}

/// Renders the sky into a `width` by `width / 2` probe.
pub fn sky(options: &SkyOptions, width: usize) -> Sky {
    let height = width / 2;
    let medium = Medium { haze: options.haze.max(0.0) };
    let transmittance = Transmittance::new(&medium);
    let multiple = Multiple::new(&medium, &transmittance, options.ground);
    let sun = sun_direction(options.elevation, options.azimuth);
    let eye = GROUND + EYE;
    let marched: Vec<([f64; 3], [f64; 3])> = (0..width * height)
        .into_par_iter()
        .map(|i| {
            let view = texel_direction(width, height, i % width, i / width);
            march(&medium, &transmittance, &multiple, eye, view, sun, options.ground)
        })
        .collect();
    // The ground's light from the sky above it: the upper hemisphere's
    // irradiance on a horizontal surface.
    let mut sky_light = [0.0; 3];
    for (i, (radiance, _)) in marched.iter().enumerate() {
        let (x, y) = (i % width, i / width);
        let d = texel_direction(width, height, x, y);
        if d[1] > 0.0 {
            let theta = (y as f64 + 0.5) / height as f64 * PI;
            let solid = (2.0 * PI / width as f64) * (PI / height as f64) * theta.sin();
            for c in 0..3 {
                sky_light[c] += radiance[c] * d[1] * solid;
            }
        }
    }
    let texels = marched
        .iter()
        .map(|(radiance, reach)| std::array::from_fn(|c| (radiance[c] + reach[c] * options.ground[c] / PI * sky_light[c]) as f32))
        .collect();
    let irradiance = transmittance.sun(eye, sun[1]);
    Sky { width, height, texels, sun, irradiance }
}

struct Medium {
    haze: f64,
}

/// Scattering and extinction coefficients at an altitude.
struct Sample {
    rayleigh: [f64; 3],
    mie: f64,
    extinction: [f64; 3],
}

impl Medium {
    fn at(&self, altitude: f64) -> Sample {
        let h = altitude.max(0.0);
        let air = (-h / RAYLEIGH_HEIGHT).exp();
        let haze = self.haze * (-h / MIE_HEIGHT).exp();
        let ozone = (1.0 - (h - 25_000.0).abs() / 15_000.0).max(0.0);
        let rayleigh = RAYLEIGH.map(|b| b * air);
        let mie = MIE_SCATTER * haze;
        let extinction = std::array::from_fn(|c| rayleigh[c] + (MIE_SCATTER + MIE_ABSORB) * haze + OZONE[c] * ozone);
        Sample { rayleigh, mie, extinction }
    }
}

/// The distance from radius `r` along a ray at cosine `mu` to the top of
/// the atmosphere.
fn to_top(r: f64, mu: f64) -> f64 {
    (-r * mu + (r * r * (mu * mu - 1.0) + TOP * TOP).max(0.0).sqrt()).max(0.0)
}

/// The distance from radius `r` along a ray at cosine `mu` to the ground,
/// when it reaches the ground.
fn to_ground(r: f64, mu: f64) -> Option<f64> {
    let disc = r * r * (mu * mu - 1.0) + GROUND * GROUND;
    (mu < 0.0 && disc >= 0.0).then(|| (-r * mu - disc.sqrt()).max(0.0))
}

/// Transmittance to the top of the atmosphere, tabulated in Bruneton's
/// parametrization over rays that miss the ground.
struct Transmittance {
    table: Vec<[f64; 3]>,
}

/// The distance from the ground to the top of the atmosphere along the
/// horizon, sqrt(TOP^2 - GROUND^2).
fn horizon() -> f64 {
    (TOP * TOP - GROUND * GROUND).sqrt()
}

impl Transmittance {
    fn new(medium: &Medium) -> Self {
        let (w, h) = TRANSMITTANCE;
        let horizon = horizon();
        let table = (0..w * h)
            .into_par_iter()
            .map(|i| {
                let (xm, xr) = ((i % w) as f64 / (w - 1) as f64, (i / w) as f64 / (h - 1) as f64);
                let rho = horizon * xr;
                let r = (rho * rho + GROUND * GROUND).sqrt();
                let (d_min, d_max) = (TOP - r, rho + horizon);
                let d = d_min + xm * (d_max - d_min);
                let mu = if d <= 0.0 { 1.0 } else { ((horizon * horizon - rho * rho - d * d) / (2.0 * r * d)).clamp(-1.0, 1.0) };
                let length = to_top(r, mu);
                let steps = 40;
                let mut depth = [0.0; 3];
                for s in 0..steps {
                    let t = (s as f64 + 0.5) / steps as f64 * length;
                    let rt = (r * r + t * t + 2.0 * r * mu * t).sqrt();
                    let e = medium.at(rt - GROUND).extinction;
                    for c in 0..3 {
                        depth[c] += e[c] * length / steps as f64;
                    }
                }
                depth.map(|d| (-d).exp())
            })
            .collect();
        Self { table }
    }

    /// Transmittance from radius `r` towards cosine `mu`, ground or not.
    fn top(&self, r: f64, mu: f64) -> [f64; 3] {
        let (w, h) = TRANSMITTANCE;
        let r = r.clamp(GROUND, TOP);
        let rho = (r * r - GROUND * GROUND).max(0.0).sqrt();
        let (d_min, d_max) = (TOP - r, rho + horizon());
        let xm = ((to_top(r, mu) - d_min) / (d_max - d_min)).clamp(0.0, 1.0);
        let xr = (rho / horizon()).clamp(0.0, 1.0);
        bilinear(&self.table, w, h, xm, xr)
    }

    /// Sunlight reaching radius `r` from a sun at cosine `mu`: zero when
    /// the ground stands between.
    fn sun(&self, r: f64, mu: f64) -> [f64; 3] {
        match to_ground(r, mu) {
            Some(_) => [0.0; 3],
            None => self.top(r, mu),
        }
    }
}

/// Hillaire's multiple scattering term Ψ over altitude and the sun's cosine.
struct Multiple {
    table: Vec<[f64; 3]>,
}

impl Multiple {
    fn new(medium: &Medium, transmittance: &Transmittance, ground: [f64; 3]) -> Self {
        let n = MULTIPLE;
        let directions: Vec<[f64; 3]> = (0..64)
            .map(|i| {
                // A spherical Fibonacci set, uniform over the sphere.
                let y = 1.0 - (i as f64 + 0.5) / 32.0;
                let s = (1.0 - y * y).max(0.0).sqrt();
                let phi = i as f64 * PI * (3.0 - 5f64.sqrt());
                [s * phi.cos(), y, s * phi.sin()]
            })
            .collect();
        let table = (0..n * n)
            .into_par_iter()
            .map(|i| {
                let mu_s = (i % n) as f64 / (n - 1) as f64 * 2.0 - 1.0;
                let r = GROUND + (i / n) as f64 / (n - 1) as f64 * (TOP - GROUND - 1.0);
                let sun = [(1.0 - mu_s * mu_s).max(0.0).sqrt(), mu_s, 0.0];
                let (mut second, mut transfer) = ([0.0; 3], [0.0; 3]);
                for d in &directions {
                    let ground_hit = to_ground(r, d[1]);
                    let length = ground_hit.unwrap_or_else(|| to_top(r, d[1]));
                    let steps = 20;
                    let mut throughput = [1.0; 3];
                    for s in 0..steps {
                        let dt = length / steps as f64;
                        let t = (s as f64 + 0.5) * dt;
                        let p = [d[0] * t, r + d[1] * t, d[2] * t];
                        let rt = norm(p);
                        let sample = medium.at(rt - GROUND);
                        let lit = transmittance.sun(rt, dot(p, sun) / rt);
                        for c in 0..3 {
                            let scatter = sample.rayleigh[c] + sample.mie;
                            let e = sample.extinction[c];
                            let step = (-e * dt).exp();
                            let integral = if e > 0.0 { (1.0 - step) / e } else { dt };
                            second[c] += throughput[c] * scatter * lit[c] / (4.0 * PI) * integral;
                            transfer[c] += throughput[c] * scatter * integral;
                            throughput[c] *= step;
                        }
                    }
                    if let Some(t) = ground_hit {
                        let p = [d[0] * t, r + d[1] * t, d[2] * t];
                        let up = p.map(|v| v / norm(p));
                        let cos = dot(up, sun);
                        let lit = transmittance.sun(GROUND, cos);
                        for c in 0..3 {
                            second[c] += throughput[c] * lit[c] * cos.max(0.0) * ground[c] / PI;
                        }
                    }
                }
                std::array::from_fn(|c| {
                    let (l2, f) = (second[c] / 64.0, transfer[c] / 64.0);
                    l2 / (1.0 - f).max(1e-3)
                })
            })
            .collect();
        Self { table }
    }

    fn at(&self, r: f64, mu_s: f64) -> [f64; 3] {
        let x = ((mu_s + 1.0) / 2.0).clamp(0.0, 1.0);
        let y = ((r - GROUND) / (TOP - GROUND)).clamp(0.0, 1.0);
        bilinear(&self.table, MULTIPLE, MULTIPLE, x, y)
    }
}

/// The radiance reaching the eye at radius `eye` along `view`, and the
/// transmittance to the ground it reaches (zero when it reaches none).
/// The ground's sky light is added after, once the sky above is known.
fn march(
    medium: &Medium,
    transmittance: &Transmittance,
    multiple: &Multiple,
    eye: f64,
    view: [f64; 3],
    sun: [f64; 3],
    ground: [f64; 3],
) -> ([f64; 3], [f64; 3]) {
    let ground_hit = to_ground(eye, view[1]);
    let length = ground_hit.unwrap_or_else(|| to_top(eye, view[1]));
    let nu = dot(view, sun);
    let rayleigh_phase = 3.0 / (16.0 * PI) * (1.0 + nu * nu);
    let g = MIE_G;
    let mie_phase = 3.0 / (8.0 * PI) * (1.0 - g * g) * (1.0 + nu * nu) / ((2.0 + g * g) * (1.0 + g * g - 2.0 * g * nu).powf(1.5));
    let (mut radiance, mut throughput) = ([0.0; 3], [1.0; 3]);
    for s in 0..VIEW_STEPS {
        // Steps grow with the square of the distance, finer near the eye.
        let (a, b) = (s as f64 / VIEW_STEPS as f64, (s + 1) as f64 / VIEW_STEPS as f64);
        let (t0, t1) = (length * a * a, length * b * b);
        let (t, dt) = ((t0 + t1) / 2.0, t1 - t0);
        let p = [view[0] * t, eye + view[1] * t, view[2] * t];
        let rt = norm(p);
        let mu_s = dot(p, sun) / rt;
        let sample = medium.at(rt - GROUND);
        let lit = transmittance.sun(rt, mu_s);
        let psi = multiple.at(rt, mu_s);
        for c in 0..3 {
            let scatter = sample.rayleigh[c] + sample.mie;
            let source = (sample.rayleigh[c] * rayleigh_phase + sample.mie * mie_phase) * lit[c] + scatter * psi[c];
            let e = sample.extinction[c];
            let step = (-e * dt).exp();
            let integral = if e > 0.0 { (1.0 - step) / e } else { dt };
            radiance[c] += throughput[c] * source * integral;
            throughput[c] *= step;
        }
    }
    let Some(t) = ground_hit else { return (radiance, [0.0; 3]) };
    let p = [view[0] * t, eye + view[1] * t, view[2] * t];
    let up = p.map(|v| v / norm(p));
    let cos = dot(up, sun);
    let lit = transmittance.sun(GROUND, cos);
    for c in 0..3 {
        radiance[c] += throughput[c] * ground[c] / PI * lit[c] * cos.max(0.0);
    }
    (radiance, throughput)
}

fn bilinear(table: &[[f64; 3]], w: usize, h: usize, x: f64, y: f64) -> [f64; 3] {
    let (fx, fy) = (x * (w - 1) as f64, y * (h - 1) as f64);
    let (x0, y0) = ((fx as usize).min(w - 2), (fy as usize).min(h - 2));
    let (ax, ay) = (fx - x0 as f64, fy - y0 as f64);
    let at = |x: usize, y: usize| table[y * w + x];
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
    std::array::from_fn(|k| (a[k] * (1.0 - ax) + b[k] * ax) * (1.0 - ay) + (c[k] * (1.0 - ax) + d[k] * ax) * ay)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
