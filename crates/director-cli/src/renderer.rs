use director_core::{
    systems::{
        layout::LayoutEngine,
        renderer::{render_sampled_at_time, GpuContext},
    },
    Director,
};

/// Render targets and the Vulkan context stay on their creating thread.
pub struct Renderer {
    surface: Option<skia_safe::Surface>,
    accumulation: Option<skia_safe::Surface>,
    transitions: Option<(skia_safe::Surface, skia_safe::Surface)>,
    context: Option<GpuContext>,
    layout: LayoutEngine,
    pixels: Vec<u8>,
}

impl Renderer {
    pub fn new(gpu: bool) -> Result<Self, String> {
        Ok(Self {
            surface: None,
            accumulation: None,
            transitions: None,
            context: crate::gpu::required_context(gpu)?,
            layout: LayoutEngine::new(),
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
        self.accumulation = None;
        self.transitions = None;
        self.layout = LayoutEngine::new();
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
        if self.transitions.is_none() && !director.transitions.is_empty() {
            self.transitions = Some((
                crate::gpu::render_surface(director.width, director.height, &mut self.context)?,
                crate::gpu::render_surface(director.width, director.height, &mut self.context)?,
            ));
        }
        render_sampled_at_time(
            director,
            &mut self.layout,
            time,
            surface,
            &mut self.accumulation,
            &mut self.transitions,
        )
        .map_err(|e| e.to_string())?;
        crate::gpu::read_pixels(surface, &mut self.context, &mut self.pixels)?;
        encode_png(surface.image_info(), &self.pixels)
    }
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
    fn sampled_frame_matches_core_motion_blur_and_repeated_seeks() {
        let mut compiled = director_html::compile("<style>body{margin:0}#box{width:16px;height:64px;background:red;animation:move 1s linear both}@keyframes move{from{transform:translateX(0px)}to{transform:translateX(48px)}}</style><div id='box'></div>", director_html::CompileOptions { width:64,height:64,duration:1.0,..Default::default() }).unwrap();
        compiled.director.samples_per_frame = 4;
        compiled.director.shutter_angle = 360.0;
        let mut context = None;
        let mut reference = crate::gpu::render_surface(64, 64, &mut context).unwrap();
        director_core::systems::renderer::render_frame(
            &mut compiled.director,
            0.5,
            reference.canvas(),
        )
        .unwrap();
        let mut pixels = vec![0; 64 * 64 * 4];
        crate::gpu::read_pixels(&mut reference, &mut context, &mut pixels).unwrap();
        let expected = encode_png(reference.image_info(), &pixels).unwrap();
        let mut renderer = Renderer::new(false).unwrap();
        assert_eq!(renderer.png(&mut compiled.director, 0.5).unwrap(), expected);
        renderer.png(&mut compiled.director, 0.1).unwrap();
        assert_eq!(renderer.png(&mut compiled.director, 0.5).unwrap(), expected);
    }
}
