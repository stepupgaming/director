//! Colour grades: 3D LUTs read from `.cube` files or built from the named
//! presets, applied as one runtime colour filter to the BT.709 encoded values
//! the canvas paints (the sRGB transfer of the render surfaces). A grade blends
//! its LUT, or two LUTs crossfading, with the ungraded colour by `intensity`.

use skia_safe::{
    images, AlphaType, ColorFilter, ColorType, Data, FilterMode, ImageInfo, MipmapMode, RuntimeEffect, SamplingOptions,
    TileMode,
};
use std::fmt;
use std::sync::{Arc, LazyLock, OnceLock};
use tracing::error;

/// The built-in grades, all original looks generated in code.
pub const PRESETS: [&str; 6] = ["film-print", "bleach-bypass", "teal-orange", "warm-fade", "night", "mono"];
/// Lattice points per axis of the preset LUTs.
pub const PRESET_SIZE: usize = 33;
/// Smallest and largest `LUT_3D_SIZE` a `.cube` file may declare.
pub const MIN_SIZE: usize = 2;
pub const MAX_SIZE: usize = 65;

/// Where a LUT came from, so a scene document can name or embed it.
#[derive(Clone, Debug, PartialEq)]
pub enum LutSource {
    Preset(&'static str),
    /// The `.cube` file text.
    Cube(Arc<[u8]>),
}

/// A 3D LUT: `size`³ output colours over the input cube from `domain_min`
/// to `domain_max`, red varying fastest, then green, then blue.
pub struct Lut3d {
    size: usize,
    table: Vec<[f32; 3]>,
    domain_min: [f32; 3],
    domain_max: [f32; 3],
    pub source: LutSource,
    /// The table as an `RGBA F16` atlas, `size * size` wide and `size` high:
    /// pixel `(r + b * size, g)` holds entry `(r, g, b)`.
    atlas: OnceLock<Option<skia_safe::Image>>,
}

impl fmt::Debug for Lut3d {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lut3d").field("size", &self.size).field("source", &self.source).finish_non_exhaustive()
    }
}

impl PartialEq for Lut3d {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Lut3d {
    fn new(size: usize, table: Vec<[f32; 3]>, domain_min: [f32; 3], domain_max: [f32; 3], source: LutSource) -> Self {
        Self { size, table, domain_min, domain_max, source, atlas: OnceLock::new() }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// The output colour at lattice point `(r, g, b)`.
    pub fn entry(&self, r: usize, g: usize, b: usize) -> [f32; 3] {
        self.table[r + self.size * (g + self.size * b)]
    }

    /// The graded colour of `rgb`, trilinear between lattice points, the
    /// reference the shader matches.
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let p: [f32; 3] = std::array::from_fn(|k| {
            let span = self.domain_max[k] - self.domain_min[k];
            ((rgb[k] - self.domain_min[k]) / span).clamp(0.0, 1.0) * (n - 1) as f32
        });
        let lo: [usize; 3] = std::array::from_fn(|k| (p[k].floor() as usize).min(n - 1));
        let hi: [usize; 3] = std::array::from_fn(|k| (lo[k] + 1).min(n - 1));
        let t: [f32; 3] = std::array::from_fn(|k| p[k] - lo[k] as f32);
        let mut out = [0.0; 3];
        for corner in 0..8 {
            let pick = |k: usize| if corner >> k & 1 == 1 { (hi[k], t[k]) } else { (lo[k], 1.0 - t[k]) };
            let ((r, wr), (g, wg), (b, wb)) = (pick(0), pick(1), pick(2));
            let weight = wr * wg * wb;
            let value = self.entry(r, g, b);
            for k in 0..3 {
                out[k] += weight * value[k];
            }
        }
        out
    }

    /// The LUT as `.cube` text at full precision.
    pub fn to_cube(&self, title: &str) -> String {
        let mut out = format!("TITLE \"{title}\"\nLUT_3D_SIZE {}\n", self.size);
        if self.domain_min != [0.0; 3] || self.domain_max != [1.0; 3] {
            let [a, b, c] = self.domain_min;
            let [d, e, f] = self.domain_max;
            out.push_str(&format!("DOMAIN_MIN {a} {b} {c}\nDOMAIN_MAX {d} {e} {f}\n"));
        }
        for [r, g, b] in &self.table {
            out.push_str(&format!("{r:.6} {g:.6} {b:.6}\n"));
        }
        out
    }

    fn atlas(&self) -> Option<skia_safe::Image> {
        self.atlas
            .get_or_init(|| {
                let n = self.size;
                let mut bytes = Vec::with_capacity(n * n * n * 8);
                for g in 0..n {
                    for b in 0..n {
                        for r in 0..n {
                            let [x, y, z] = self.entry(r, g, b);
                            for value in [x, y, z, 1.0] {
                                bytes.extend(f16_bits(value).to_le_bytes());
                            }
                        }
                    }
                }
                let info = ImageInfo::new(((n * n) as i32, n as i32), ColorType::RGBAF16, AlphaType::Unpremul, None);
                images::raster_from_data(&info, Data::new_copy(&bytes), n * n * 8)
            })
            .clone()
    }
}

/// IEEE half-precision bits of `value`, rounded to nearest even.
fn f16_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x7f_ffff;
    if exponent == 0xff {
        return sign | 0x7c00 | if mantissa != 0 { 0x200 } else { 0 };
    }
    let unbiased = exponent - 127 + 15;
    if unbiased >= 0x1f {
        return sign | 0x7c00;
    }
    if unbiased <= 0 {
        if unbiased < -10 {
            return sign;
        }
        let full = mantissa | 0x80_0000;
        let shift = (14 - unbiased) as u32;
        let half = full >> shift;
        let rest = full & ((1 << shift) - 1);
        let midpoint = 1 << (shift - 1);
        let round = u32::from(rest > midpoint || (rest == midpoint && half & 1 == 1));
        return sign | (half + round) as u16;
    }
    let half = ((unbiased as u32) << 10) | (mantissa >> 13);
    let rest = mantissa & 0x1fff;
    let round = u32::from(rest > 0x1000 || (rest == 0x1000 && half & 1 == 1));
    sign | (half + round) as u16
}

/// Read a `.cube` 3D LUT: `LUT_3D_SIZE` from 2 to 65, optional `TITLE`,
/// `DOMAIN_MIN`, `DOMAIN_MAX` or `LUT_3D_INPUT_RANGE`, `#` comments, then
/// `size`³ lines of three numbers.
pub fn parse_cube(bytes: &[u8]) -> Result<Lut3d, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "the .cube file is not UTF-8 text".to_string())?;
    let mut size = None;
    let mut domain_min = [0.0f32; 3];
    let mut domain_max = [1.0f32; 3];
    let mut table: Vec<[f32; 3]> = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        let at = |message: String| format!("line {}: {message}", number + 1);
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or_default();
        let numbers = |words: std::str::SplitWhitespace<'_>| -> Result<Vec<f32>, String> {
            words
                .map(|w| w.parse::<f32>().ok().filter(|v| v.is_finite()).ok_or_else(|| at(format!("'{w}' is not a number"))))
                .collect()
        };
        match first {
            "TITLE" => {}
            "LUT_1D_SIZE" => return Err(at("1D LUTs are not supported; use a 3D LUT".into())),
            "LUT_3D_SIZE" => {
                let n: usize = words.next().and_then(|w| w.parse().ok()).ok_or_else(|| at("LUT_3D_SIZE needs a size".into()))?;
                if !(MIN_SIZE..=MAX_SIZE).contains(&n) {
                    return Err(at(format!("LUT_3D_SIZE {n} is outside {MIN_SIZE}..={MAX_SIZE}")));
                }
                size = Some(n);
            }
            "DOMAIN_MIN" | "DOMAIN_MAX" => {
                let values = numbers(words)?;
                let values: [f32; 3] = values.try_into().map_err(|_| at(format!("{first} needs three numbers")))?;
                if first == "DOMAIN_MIN" { domain_min = values } else { domain_max = values }
            }
            "LUT_3D_INPUT_RANGE" => {
                let values = numbers(words)?;
                let [lo, hi]: [f32; 2] = values.try_into().map_err(|_| at("LUT_3D_INPUT_RANGE needs two numbers".into()))?;
                domain_min = [lo; 3];
                domain_max = [hi; 3];
            }
            word if word.starts_with(|c: char| c.is_ascii_alphabetic()) => {}
            _ => {
                let values = numbers(line.split_whitespace())?;
                let rgb: [f32; 3] = values.try_into().map_err(|_| at("a table row needs three numbers".into()))?;
                table.push(rgb);
            }
        }
    }
    let size = size.ok_or("the .cube file has no LUT_3D_SIZE")?;
    if table.len() != size * size * size {
        return Err(format!("LUT_3D_SIZE {size} needs {} rows; the file has {}", size * size * size, table.len()));
    }
    if (0..3).any(|k| domain_max[k] <= domain_min[k]) {
        return Err("DOMAIN_MAX must exceed DOMAIN_MIN on every channel".into());
    }
    Ok(Lut3d::new(size, table, domain_min, domain_max, LutSource::Cube(Arc::from(bytes))))
}

/// The preset named `name`, built once per process.
pub fn preset(name: &str) -> Option<Arc<Lut3d>> {
    static LUTS: LazyLock<Vec<Arc<Lut3d>>> = LazyLock::new(|| PRESETS.iter().map(|name| Arc::new(build_preset(name))).collect());
    PRESETS.iter().position(|preset| *preset == name).map(|i| LUTS[i].clone())
}

fn build_preset(name: &'static str) -> Lut3d {
    let look: fn([f32; 3]) -> [f32; 3] = match name {
        "film-print" => film_print,
        "bleach-bypass" => bleach_bypass,
        "teal-orange" => teal_orange,
        "warm-fade" => warm_fade,
        "night" => night,
        _ => mono,
    };
    let n = PRESET_SIZE;
    let step = 1.0 / (n - 1) as f32;
    let mut table = Vec::with_capacity(n * n * n);
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                let out = look([r as f32 * step, g as f32 * step, b as f32 * step]);
                table.push(out.map(|v| v.clamp(0.0, 1.0)));
            }
        }
    }
    Lut3d::new(n, table, [0.0; 3], [1.0; 3], LutSource::Preset(name))
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Saturation about the BT.709 luma: 0 is grey, 1 unchanged.
fn saturate(c: [f32; 3], amount: f32) -> [f32; 3] {
    let l = luma(c);
    c.map(|v| l + (v - l) * amount)
}

/// An S curve: 0 unchanged, 1 a smoothstep.
fn contrast(x: f32, amount: f32) -> f32 {
    lerp(x, smooth(x), amount)
}

/// Print film: soft S curve, held blacks and whites, cool shadows, warm highlights.
fn film_print(c: [f32; 3]) -> [f32; 3] {
    let c = saturate(c, 0.88);
    let l = luma(c);
    let (shadow, light) = ((1.0 - l).powi(2), l * l);
    let cool = [-0.012, 0.004, 0.022];
    let warm = [0.026, 0.008, -0.024];
    std::array::from_fn(|k| 0.025 + 0.95 * contrast(c[k], 0.55) + cool[k] * shadow + warm[k] * light)
}

/// Bleach bypass: the colour image overlaid with its own luma, low saturation, hard contrast.
fn bleach_bypass(c: [f32; 3]) -> [f32; 3] {
    let l = luma(c);
    let overlay = |a: f32| if a < 0.5 { 2.0 * a * l } else { 1.0 - 2.0 * (1.0 - a) * (1.0 - l) };
    let mixed = c.map(|a| lerp(a, overlay(a), 0.75));
    saturate(mixed, 0.5).map(|v| contrast(v, 0.3))
}

/// Teal shadows and warm highlights at unchanged luma.
fn teal_orange(c: [f32; 3]) -> [f32; 3] {
    let l = luma(c);
    let teal = [0.0, 0.52, 0.58];
    let orange = [1.0, 0.58, 0.28];
    let t = smooth((l - 0.15) / 0.7);
    let tint: [f32; 3] = std::array::from_fn(|k| lerp(teal[k], orange[k], t));
    let shift = luma(tint);
    let strength = 0.22 * (0.35 + 2.6 * l * (1.0 - l));
    let toned: [f32; 3] = std::array::from_fn(|k| c[k] + (tint[k] - shift) * strength);
    let toned = saturate(toned, 1.08);
    let fix = l - luma(toned);
    toned.map(|v| contrast(v + fix, 0.2))
}

/// Faded film: lifted blacks, soft whites, a warm cast and less saturation.
fn warm_fade(c: [f32; 3]) -> [f32; 3] {
    let c = saturate(c, 0.8);
    let gain = [1.04, 1.0, 0.9];
    let add = [0.012, 0.006, 0.0];
    std::array::from_fn(|k| (0.08 + 0.84 * contrast(c[k], 0.2)) * gain[k] + add[k])
}

/// Day for night: two stops under, desaturated and pushed blue.
fn night(c: [f32; 3]) -> [f32; 3] {
    let c = saturate(c, 0.35);
    let tint = [0.72, 0.9, 1.28];
    let lift = [0.0, 0.004, 0.018];
    std::array::from_fn(|k| (c[k] * 0.42).powf(1.12) * tint[k] + lift[k])
}

/// Neutral black and white with a gentle S curve.
fn mono(c: [f32; 3]) -> [f32; 3] {
    [contrast(luma(c), 0.35); 3]
}

/// A grade on one element or frame: `from`, crossfading toward `to` by
/// `mix`, blended with the ungraded colour by `intensity`.
#[derive(Clone, Debug, PartialEq)]
pub struct Grade {
    pub from: Arc<Lut3d>,
    pub to: Option<Arc<Lut3d>>,
    /// 0 is `from`, 1 is `to`. Clamped to `0..=1`.
    pub mix: f32,
    /// 0 is ungraded, 1 is the full grade. Clamped to `0..=1`.
    pub intensity: f32,
}

const SKSL: &str = r#"
uniform shader lut_a;
uniform shader lut_b;
uniform float3 u_min_a;
uniform float3 u_scale_a;
uniform float u_size_a;
uniform float3 u_min_b;
uniform float3 u_scale_b;
uniform float u_size_b;
uniform float u_mix;
uniform float u_amount;

float3 grade_a(float3 c) {
    float3 p = clamp((c - u_min_a) * u_scale_a, 0.0, 1.0) * (u_size_a - 1.0);
    float b0 = min(floor(p.b), u_size_a - 1.0);
    float b1 = min(b0 + 1.0, u_size_a - 1.0);
    float2 xy = p.rg + 0.5;
    float3 lo = float3(lut_a.eval(float2(xy.x + b0 * u_size_a, xy.y)).rgb);
    float3 hi = float3(lut_a.eval(float2(xy.x + b1 * u_size_a, xy.y)).rgb);
    return mix(lo, hi, p.b - b0);
}

float3 grade_b(float3 c) {
    float3 p = clamp((c - u_min_b) * u_scale_b, 0.0, 1.0) * (u_size_b - 1.0);
    float b0 = min(floor(p.b), u_size_b - 1.0);
    float b1 = min(b0 + 1.0, u_size_b - 1.0);
    float2 xy = p.rg + 0.5;
    float3 lo = float3(lut_b.eval(float2(xy.x + b0 * u_size_b, xy.y)).rgb);
    float3 hi = float3(lut_b.eval(float2(xy.x + b1 * u_size_b, xy.y)).rgb);
    return mix(lo, hi, p.b - b0);
}

half4 main(half4 color) {
    if (color.a <= 0.0) {
        return color;
    }
    float a = float(color.a);
    float3 c = float3(color.rgb) / a;
    float3 graded = grade_a(c);
    if (u_mix > 0.0) {
        graded = mix(graded, grade_b(c), u_mix);
    }
    float3 result = clamp(mix(c, graded, u_amount), 0.0, 1.0);
    return half4(half3(result * a), color.a);
}
"#;

thread_local! {
    static EFFECT: Option<RuntimeEffect> = RuntimeEffect::make_for_color_filter(SKSL, None)
        .map_err(|message| error!("grade shader compilation error: {message}"))
        .ok();
}

impl Grade {
    /// One LUT at full strength.
    pub fn new(from: Arc<Lut3d>) -> Self {
        Self { from, to: None, mix: 0.0, intensity: 1.0 }
    }

    /// The grade as a colour filter on premultiplied colour. `None` when it
    /// leaves colours unchanged or the shader is unavailable.
    pub fn color_filter(&self) -> Option<ColorFilter> {
        let amount = self.intensity.clamp(0.0, 1.0);
        if amount <= 0.0 {
            return None;
        }
        let mix = if self.to.is_some() { self.mix.clamp(0.0, 1.0) } else { 0.0 };
        let b = self.to.as_ref().unwrap_or(&self.from);
        let effect = EFFECT.with(Clone::clone)?;
        let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);
        let shader = |lut: &Lut3d| lut.atlas()?.to_raw_shader((TileMode::Clamp, TileMode::Clamp), sampling, None);
        let children = [shader(&self.from)?.into(), shader(b)?.into()];
        let scale = |lut: &Lut3d| -> [f32; 3] { std::array::from_fn(|k| 1.0 / (lut.domain_max[k] - lut.domain_min[k])) };
        let values: [(&str, Vec<f32>); 8] = [
            ("u_min_a", self.from.domain_min.to_vec()),
            ("u_scale_a", scale(&self.from).to_vec()),
            ("u_size_a", vec![self.from.size as f32]),
            ("u_min_b", b.domain_min.to_vec()),
            ("u_scale_b", scale(b).to_vec()),
            ("u_size_b", vec![b.size as f32]),
            ("u_mix", vec![mix]),
            ("u_amount", vec![amount]),
        ];
        let mut uniforms = vec![0u8; effect.uniform_size()];
        for uniform in effect.uniforms() {
            let (_, data) = values.iter().find(|(name, _)| *name == uniform.name())?;
            let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_ne_bytes()).collect();
            uniforms[uniform.offset()..uniform.offset() + bytes.len()].copy_from_slice(&bytes);
        }
        effect.make_color_filter(Data::new_copy(&uniforms), &children[..])
    }

    /// The grade after `input` in an image filter chain.
    pub fn image_filter(&self, input: Option<skia_safe::ImageFilter>) -> Option<skia_safe::ImageFilter> {
        match self.color_filter() {
            Some(filter) => skia_safe::image_filters::color_filter(filter, input, None),
            None => input,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(size: usize) -> String {
        let mut text = format!("# identity\nTITLE \"id\"\nLUT_3D_SIZE {size}\n");
        let step = 1.0 / (size - 1) as f32;
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    text.push_str(&format!("{} {} {}\n", r as f32 * step, g as f32 * step, b as f32 * step));
                }
            }
        }
        text
    }

    #[test]
    fn cube_files_parse_and_identity_luts_change_nothing() {
        let lut = parse_cube(identity(5).as_bytes()).unwrap();
        assert_eq!(lut.size(), 5);
        for rgb in [[0.0, 0.0, 0.0], [0.3, 0.6, 0.9], [1.0, 0.5, 0.25]] {
            let out = lut.apply(rgb);
            assert!((0..3).all(|k| (out[k] - rgb[k]).abs() < 1e-5), "{rgb:?} -> {out:?}");
        }
        let errors = [
            ("LUT_1D_SIZE 4\n", "1D"),
            ("0 0 0\n", "no LUT_3D_SIZE"),
            ("LUT_3D_SIZE 2\n0 0 0\n", "needs 8 rows"),
            ("LUT_3D_SIZE 99\n", "outside"),
            ("LUT_3D_SIZE 2\n0 0 x\n", "not a number"),
        ];
        for (text, expected) in errors {
            let error = parse_cube(text.as_bytes()).unwrap_err();
            assert!(error.contains(expected), "{text:?}: {error}");
        }
    }

    #[test]
    fn domains_rescale_input_before_lookup() {
        let text = identity(3).replace("LUT_3D_SIZE 3\n", "LUT_3D_SIZE 3\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n");
        let lut = parse_cube(text.as_bytes()).unwrap();
        let out = lut.apply([1.0, 0.5, 2.0]);
        assert!((out[0] - 0.5).abs() < 1e-5 && (out[1] - 0.25).abs() < 1e-5 && (out[2] - 1.0).abs() < 1e-5, "{out:?}");
    }

    #[test]
    fn presets_round_trip_through_cube_text_and_keep_their_character() {
        for name in PRESETS {
            let lut = preset(name).unwrap();
            let again = parse_cube(lut.to_cube(name).as_bytes()).unwrap();
            assert_eq!(again.size(), PRESET_SIZE);
            assert!(lut.table.iter().zip(&again.table).all(|(a, b)| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-6)));
            let (black, white) = (luma(lut.apply([0.0; 3])), luma(lut.apply([1.0; 3])));
            assert!(black < 0.1 && white > 0.25 && white > black + 0.2, "{name}: {black} {white}");
        }
        let grey = preset("mono").unwrap().apply([0.9, 0.2, 0.1]);
        assert!((grey[0] - grey[1]).abs() < 1e-5 && (grey[1] - grey[2]).abs() < 1e-5);
        let [r, _, b] = preset("teal-orange").unwrap().apply([0.8, 0.7, 0.6]);
        assert!(r > b + 0.2, "warm highlights stay warm");
        let [r, _, b] = preset("night").unwrap().apply([0.5, 0.5, 0.5]);
        assert!(b > r && b < 0.4, "night is dark and blue: {r} {b}");
        assert!(preset("unknown").is_none());
    }

    #[test]
    fn half_floats_round_like_the_hardware() {
        assert_eq!(f16_bits(0.0), 0);
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(0.5), 0x3800);
        assert_eq!(f16_bits(-2.0), 0xc000);
        assert_eq!(f16_bits(1e-8), 0);
        assert_eq!(f16_bits(65536.0), 0x7c00);
        assert_eq!(f16_bits(0.1), 0x2e66);
    }

    /// Paint `pixels` (straight RGBA8) through `grade` on a raster surface.
    pub(crate) fn graded(grade: &Grade, pixels: &[[u8; 4]]) -> Vec<[u8; 4]> {
        let width = pixels.len() as i32;
        let info = ImageInfo::new((width, 1), ColorType::RGBA8888, AlphaType::Unpremul, skia_safe::ColorSpace::new_srgb());
        let bytes: Vec<u8> = pixels.iter().flatten().copied().collect();
        let image = images::raster_from_data(&info, Data::new_copy(&bytes), width as usize * 4).unwrap();
        let mut surface = skia_safe::surfaces::raster(
            &ImageInfo::new((width, 1), ColorType::RGBA8888, AlphaType::Premul, skia_safe::ColorSpace::new_srgb()),
            None,
            None,
        )
        .unwrap();
        let mut paint = skia_safe::Paint::default();
        paint.set_color_filter(grade.color_filter());
        surface.canvas().draw_image(&image, (0, 0), Some(&paint));
        let out_info = ImageInfo::new((width, 1), ColorType::RGBA8888, AlphaType::Unpremul, skia_safe::ColorSpace::new_srgb());
        let mut out = vec![0u8; bytes.len()];
        assert!(surface.read_pixels(&out_info, &mut out, width as usize * 4, (0, 0)));
        out.as_chunks::<4>().0.to_vec()
    }

    #[test]
    fn the_shader_matches_the_reference_lookup_on_the_cpu() {
        let lut = preset("teal-orange").unwrap();
        let pixels: Vec<[u8; 4]> = (0..64u32)
            .map(|i| [(i * 4) as u8, (255 - i * 3) as u8, ((i * 37) % 256) as u8, 255])
            .chain([[200, 120, 60, 128]])
            .collect();
        let out = graded(&Grade::new(lut.clone()), &pixels);
        for (pixel, got) in pixels.iter().zip(&out) {
            let rgb = [pixel[0], pixel[1], pixel[2]].map(|v| v as f32 / 255.0);
            let want = lut.apply(rgb).map(|v| (v * 255.0).round());
            for k in 0..3 {
                // Premultiplied 8-bit storage costs about one level at half alpha.
                let slack = if pixel[3] == 255 { 1.0 } else { 3.0 };
                assert!((got[k] as f32 - want[k]).abs() <= slack, "{pixel:?}: {got:?} vs {want:?}");
            }
            assert_eq!(got[3], pixel[3]);
        }
        let none = Grade { intensity: 0.0, ..Grade::new(lut.clone()) };
        assert!(none.color_filter().is_none());
        let half = graded(&Grade { intensity: 0.5, ..Grade::new(lut.clone()) }, &pixels[..1]);
        let full = lut.apply([0.0, 1.0, 0.0]).map(|v| v * 255.0);
        let expected = [0.0, 255.0, 0.0].iter().zip(full).map(|(a, b)| (a + b) / 2.0).collect::<Vec<_>>();
        assert!((0..3).all(|k| (half[0][k] as f32 - expected[k]).abs() <= 1.0), "{half:?} vs {expected:?}");
    }

    #[test]
    fn crossfades_blend_two_luts_by_mix() {
        let (a, b) = (preset("mono").unwrap(), preset("night").unwrap());
        let pixel = [[180u8, 90, 40, 255]];
        let rgb = [180.0f32, 90.0, 40.0].map(|v| v / 255.0);
        for mix in [0.0, 0.25, 1.0] {
            let out = graded(&Grade { from: a.clone(), to: Some(b.clone()), mix, intensity: 1.0 }, &pixel);
            let (x, y) = (a.apply(rgb), b.apply(rgb));
            for k in 0..3 {
                let want = (x[k] + (y[k] - x[k]) * mix) * 255.0;
                assert!((out[0][k] as f32 - want).abs() <= 1.0, "mix {mix}: {out:?} vs {want}");
            }
        }
    }
}
