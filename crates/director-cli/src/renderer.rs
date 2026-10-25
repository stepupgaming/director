use director_core::scene_import::ImportRegistry;
use director_core::Director;
use director_render::{FrameState, GpuContext};
use std::path::Path;

/// Core kinds plus the `director-html` kinds.
pub fn import_registry() -> ImportRegistry {
    let mut registry = ImportRegistry::core();
    director_html::register_import(&mut registry);
    registry
}

/// Export `director` to `output` through the pipelined document export: one
/// session on a Vulkan `context`, or a CPU worker pool.
pub fn export_video(
    director: &mut Director,
    output: &Path,
    context: Option<&mut GpuContext>,
    alpha: bool,
    scale: f32,
    draft: bool,
) -> anyhow::Result<()> {
    director_render::export_director(director, import_registry(), output, alpha, scale, draft, context)
}

/// Render targets and the Vulkan context stay on their creating thread.
pub struct Renderer {
    surface: Option<skia_safe::Surface>,
    state: FrameState,
    context: Option<GpuContext>,
    pixels: Vec<u8>,
}

impl Renderer {
    pub fn new(gpu: bool) -> Result<Self, String> {
        Ok(Self {
            surface: None,
            state: FrameState::default(),
            context: crate::gpu::required_context(gpu)?,
            pixels: Vec::new(),
        })
    }
    pub fn backend(&self) -> &'static str {
        if self.context.is_some() {
            "vulkan"
        } else {
            "cpu"
        }
    }
    pub fn reset(&mut self) {
        self.surface = None;
        self.state = FrameState::default();
        self.pixels.clear();
    }

    pub fn png(&mut self, director: &mut Director, time: f64) -> Result<Vec<u8>, String> {
        if !time.is_finite() || time < 0.0 {
            return Err("frame time must be finite and nonnegative".into());
        }
        let dimensions = (director.width, director.height);
        if self
            .surface
            .as_ref()
            .is_none_or(|s| (s.width(), s.height()) != dimensions)
        {
            self.reset();
            self.surface = Some(crate::gpu::render_surface(
                director.width,
                director.height,
                &mut self.context,
            )?);
            self.pixels
                .resize(director.width as usize * director.height as usize * 4, 0);
        }
        let surface = self.surface.as_mut().unwrap();
        self.state
            .render(director, time, surface)
            .map_err(|e| e.to_string())?;
        crate::gpu::read_pixels(surface, &mut self.context, &mut self.pixels)?;
        encode_png(surface.image_info(), &self.pixels)
    }
}

pub const MAX_FRAME_PIXELS: usize = 16 * 1024 * 1024;

pub fn frame_within_limit(width: i32, height: i32) -> Result<(), String> {
    if width <= 0 || height <= 0 {
        return Err("frame size must be positive".into());
    }
    let pixels = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| format!("frame exceeds {MAX_FRAME_PIXELS} pixels"))?;
    if pixels > MAX_FRAME_PIXELS {
        return Err(format!("frame exceeds {MAX_FRAME_PIXELS} pixels"));
    }
    Ok(())
}

pub fn paint_rgba(
    director: &mut Director,
    time: f64,
    gpu: bool,
) -> Result<(Vec<u8>, u32, u32), String> {
    frame_within_limit(director.width, director.height)?;
    let mut renderer = Renderer::new(gpu)?;
    let png = renderer.png(director, time)?;
    let image = image::load_from_memory(&png)
        .map_err(|error| error.to_string())?
        .into_rgba8();
    let (width, height) = image.dimensions();
    frame_within_limit(width as i32, height as i32)?;
    Ok((image.into_raw(), width, height))
}

pub fn encode_png(info: skia_safe::ImageInfo, pixels: &[u8]) -> Result<Vec<u8>, String> {
    let image = skia_safe::images::raster_from_data(
        &info,
        skia_safe::Data::new_copy(pixels),
        info.width() as usize * 4,
    )
    .ok_or("PNG raster allocation failed")?;
    let data = image
        .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
        .ok_or("PNG encoding failed")?;
    Ok(data.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sampled_frame_matches_a_fresh_frame_after_repeated_seeks() {
        let mut compiled = director_html::compile("<style>body{margin:0}#box{width:16px;height:64px;background:red;animation:move 1s linear both}@keyframes move{from{transform:translateX(0px)}to{transform:translateX(48px)}}</style><div id='box'></div>", director_html::CompileOptions { width:64,height:64,duration:1.0,..Default::default() }).unwrap();
        compiled.director.samples_per_frame = 4;
        compiled.director.shutter_angle = 360.0;
        let mut context = None;
        let mut reference = crate::gpu::render_surface(64, 64, &mut context).unwrap();
        director_render::render_frame(&mut compiled.director, 0.5, &mut reference).unwrap();
        let mut pixels = vec![0; 64 * 64 * 4];
        crate::gpu::read_pixels(&mut reference, &mut context, &mut pixels).unwrap();
        let expected = encode_png(reference.image_info(), &pixels).unwrap();
        let mut renderer = Renderer::new(false).unwrap();
        assert_eq!(renderer.png(&mut compiled.director, 0.5).unwrap(), expected);
        renderer.png(&mut compiled.director, 0.1).unwrap();
        assert_eq!(renderer.png(&mut compiled.director, 0.5).unwrap(), expected);
    }
}
