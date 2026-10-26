use director_core::Director;
use director_host::{import_registry, Session};
use director_render::{Frame, GpuContext};
use std::path::Path;

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

/// Give `session` a required Vulkan context under `gpu`. Its frames then paint
/// on that context, which stays on the calling thread.
pub fn use_gpu(session: &mut Session, gpu: bool) -> Result<(), String> {
    let context = crate::gpu::required_context(gpu)?;
    #[cfg(feature = "vulkan")]
    if let Some(context) = context {
        session.use_gpu(context);
    }
    #[cfg(not(feature = "vulkan"))]
    let _ = (session, context);
    Ok(())
}

/// Paint the frame at `time` once the canvas fits [`MAX_FRAME_PIXELS`].
pub fn paint(session: &mut Session, time: f64) -> Result<Frame, String> {
    let document = session.document();
    frame_within_limit(document.width, document.height)?;
    session.render_frame(time).map_err(|error| error.to_string())
}

/// Encode a straight-alpha frame as an sRGB PNG.
pub fn frame_png(frame: &Frame) -> Result<Vec<u8>, String> {
    let info = skia_safe::ImageInfo::new(
        (frame.width as i32, frame.height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        Some(skia_safe::ColorSpace::new_srgb()),
    );
    encode_png(info, &frame.rgba)
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

/// A session over `html` compiled in memory on a square one-second canvas.
#[cfg(test)]
pub fn html_session(html: &str, size: i32) -> Session {
    let compiled = director_html::compile(
        html,
        director_html::CompileOptions {
            width: size,
            height: size,
            duration: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    let document = director_core::scene_export::export_scene(&compiled.director).unwrap();
    Session::from_document(document, compiled.director.assets.loader.clone()).unwrap()
}
