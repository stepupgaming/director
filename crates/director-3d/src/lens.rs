//! Footage lens distortion on the model layer. A tracked model renders
//! through a pinhole camera wide enough to hold every ray the distorted
//! frame shows, at the same focal length in pixels, and a Skia shader bends
//! that render into the footage's lens as it paints.

use director_core::RenderError;
use skia_safe::{Canvas, Data, FilterMode, Image, MipmapMode, Paint, Rect, RuntimeEffect, SamplingOptions, TileMode};

/// Each output pixel's ray on the z = 1 plane, undistorted by fixed point
/// iteration, picks its pixel in the wide render.
const SKSL: &str = r#"
uniform shader frame;
uniform float2 centre;
uniform float2 focal;
uniform float2 source;
uniform float2 k;
uniform float source_focal;

half4 main(float2 p) {
    float2 d = (p - centre) / focal;
    float2 u = d;
    for (int i = 0; i < 20; i++) {
        float r2 = dot(u, u);
        u = d / (1.0 + k.x * r2 + k.y * r2 * r2);
    }
    return frame.eval(source + u * source_focal);
}
"#;

thread_local! {
    static EFFECT: Option<RuntimeEffect> = RuntimeEffect::make_for_shader(SKSL, None)
        .map_err(|message| eprintln!("lens shader compilation error: {message}"))
        .ok();
}

/// The wide pinhole render behind a distorted frame of `width` by `height`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overscan {
    /// The wide render's size in pixels, centred on the frame's centre.
    pub width: u32,
    pub height: u32,
    /// Its vertical field of view in radians.
    pub fov: f32,
    /// The frame's size in pixels and vertical field of view in radians.
    frame: (u32, u32),
    frame_fov: f32,
    k: [f32; 2],
}

fn undistort(d: [f64; 2], k: [f64; 2]) -> [f64; 2] {
    let mut u = d;
    for _ in 0..50 {
        let r2 = u[0] * u[0] + u[1] * u[1];
        let s = 1.0 + k[0] * r2 + k[1] * r2 * r2;
        u = [d[0] / s, d[1] / s];
    }
    u
}

/// The wide render a frame of `width` by `height` pixels with vertical field
/// of view `fov` (radians) needs under distortion `k`, or `None` for a
/// pinhole lens. A lens that pulls the edges in (barrel, k1 < 0) widens the
/// render up to three times; one that pushes them out keeps the frame's size.
pub fn overscan(width: u32, height: u32, fov: f32, k: [f32; 2]) -> Option<Overscan> {
    if k == [0.0, 0.0] {
        return None;
    }
    let (w, h) = (width as f64, height as f64);
    let f = (h / 2.0) / (fov as f64 / 2.0).tan();
    let kd = [k[0] as f64, k[1] as f64];
    let (mut sx, mut sy) = (1.0f64, 1.0f64);
    for i in 0..=64 {
        let t = i as f64 / 64.0 * 2.0 - 1.0;
        for (x, y) in [(t * w / 2.0, h / 2.0), (t * w / 2.0, -h / 2.0), (w / 2.0, t * h / 2.0), (-w / 2.0, t * h / 2.0)] {
            let u = undistort([x / f, y / f], kd);
            if u.iter().all(|v| v.is_finite()) {
                sx = sx.max(u[0].abs() * f / (w / 2.0));
                sy = sy.max(u[1].abs() * f / (h / 2.0));
            }
        }
    }
    // Two pixels more each side for the bilinear filter, and both margins
    // whole so the centres coincide.
    let margin = |size: f64, scale: f64| ((size * (scale.min(3.0) - 1.0) / 2.0).ceil() as u32 + 2).min(8192);
    let (wide, high) = ((width + 2 * margin(w, sx)).min(8192), (height + 2 * margin(h, sy)).min(8192));
    let fov_wide = 2.0 * ((fov as f64 / 2.0).tan() * high as f64 / h).atan();
    Some(Overscan { width: wide, height: high, fov: fov_wide as f32, frame: (width, height), frame_fov: fov, k })
}

impl Overscan {
    /// Paints the wide render `image` bent into the lens over `rect`.
    pub fn draw(&self, canvas: &Canvas, image: &Image, rect: Rect, paint: &Paint) -> Result<(), RenderError> {
        let effect = EFFECT.with(Clone::clone).ok_or_else(|| RenderError::SkiaError("lens shader".into()))?;
        let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::None);
        let frame = image
            .to_shader(Some((TileMode::Decal, TileMode::Decal)), sampling, None)
            .ok_or_else(|| RenderError::SkiaError("lens frame shader".into()))?;
        // The frame's focal length in pixels, in the canvas's units along
        // each axis of the box, and the wide render's in its pixels.
        let (w, h) = (self.frame.0 as f32, self.frame.1 as f32);
        let focal = h / 2.0 / (self.frame_fov / 2.0).tan();
        let (focal_x, focal_y) = (focal * rect.width() / w, focal * rect.height() / h);
        let source_focal = self.height as f32 / 2.0 / (self.fov / 2.0).tan();
        let centre = rect.center();
        let values: [(&str, Vec<f32>); 5] = [
            ("centre", vec![centre.x, centre.y]),
            ("focal", vec![focal_x, focal_y]),
            ("source", vec![self.width as f32 / 2.0, self.height as f32 / 2.0]),
            ("k", self.k.to_vec()),
            ("source_focal", vec![source_focal]),
        ];
        let mut uniforms = vec![0u8; effect.uniform_size()];
        for uniform in effect.uniforms() {
            let Some((_, data)) = values.iter().find(|(name, _)| *name == uniform.name()) else { continue };
            let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_ne_bytes()).collect();
            uniforms[uniform.offset()..uniform.offset() + bytes.len()].copy_from_slice(&bytes);
        }
        let shader = effect
            .make_shader(Data::new_copy(&uniforms), &[frame.into()], None)
            .ok_or_else(|| RenderError::SkiaError("lens shader".into()))?;
        let mut paint = paint.clone();
        paint.set_shader(shader);
        canvas.draw_rect(rect, &paint);
        Ok(())
    }
}
