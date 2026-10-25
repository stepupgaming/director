//! Serial export of a live `Director`, on a Vulkan target when the caller
//! supplies a [`GpuContext`], with the timeline audio mixed per frame.

use crate::{FrameState, RenderError};
use director_core::audio::load_audio_bytes;
use director_core::timeline::{audio_samples_per_video_frame, duration_to_frame_count, frame_to_time};
use director_core::video_wrapper::{Encoder, EncoderSettings};
use director_core::Director;
use skia_safe::{AlphaType, ColorSpace, ColorType, ImageInfo, Surface};
use std::path::PathBuf;

/// The GPU context a Vulkan build renders through.
#[cfg(feature = "vulkan")]
pub type GpuContext = skia_safe::gpu::DirectContext;
/// The GPU context a Vulkan build renders through. CPU builds have none.
#[cfg(not(feature = "vulkan"))]
pub type GpuContext = ();

/// Create the requested backend. A supplied GPU context requires a GPU target.
#[cfg_attr(not(feature = "vulkan"), allow(unused_variables))]
fn render_surface(gpu_context: &mut Option<&mut GpuContext>, info: &ImageInfo) -> Option<Surface> {
    #[cfg(feature = "vulkan")]
    if let Some(ctx) = gpu_context.as_deref_mut() {
        return skia_safe::gpu::surfaces::render_target(
            ctx,
            skia_safe::gpu::Budgeted::No,
            info,
            0,
            None,
            None,
            false,
            false,
        );
    }
    skia_safe::surfaces::raster(info, None, None)
}

/// Accept a raster scale in `(0, 1]`.
pub fn validate_scale(scale: f32) -> Result<(), String> {
    if !scale.is_finite() || scale <= 0.0 || scale > 1.0 {
        Err("scale must be finite and in (0, 1]".into())
    } else {
        Ok(())
    }
}

/// Raster size and device scale for `scale` in `(0, 1]`. `1` keeps the layout
/// size, including odd sizes. A smaller scale rounds each side down to an even
/// pixel count so yuv420 export accepts it.
pub fn output_dimensions(width: i32, height: i32, scale: f32) -> Result<(i32, i32, f32, f32), String> {
    if width <= 0 || height <= 0 {
        return Err("source size must be positive".into());
    }
    validate_scale(scale)?;
    if (scale - 1.0).abs() <= f32::EPSILON {
        return Ok((width, height, 1.0, 1.0));
    }
    let out_w = even_output_px(width, scale)?;
    let out_h = even_output_px(height, scale)?;
    Ok((out_w, out_h, out_w as f32 / width as f32, out_h as f32 / height as f32))
}

fn even_output_px(source: i32, scale: f32) -> Result<i32, String> {
    let rounded = (source as f32 * scale).round() as i32;
    let even = if rounded <= 2 {
        2
    } else if rounded % 2 == 0 {
        rounded
    } else {
        rounded - 1
    };
    if even > source {
        return Err("scaled size exceeds the source".into());
    }
    Ok(even)
}

/// Store the device scale for one export. Layout `width` and `height` stay put.
pub fn set_output_scale(director: &mut Director, scale: f32) -> Result<(i32, i32), String> {
    let (width, height, sx, sy) = output_dimensions(director.width, director.height, scale)?;
    director.output_scale_x = sx;
    director.output_scale_y = sy;
    Ok((width, height))
}

/// Export the timeline of `director` to `out_path` as H.264, or VP9 with alpha
/// when `alpha` is set. `audio_track_path` adds a background track to the mix.
pub fn render_export(
    director: &mut Director,
    out_path: PathBuf,
    gpu_context: Option<&mut GpuContext>,
    audio_track_path: Option<PathBuf>,
    alpha: bool,
) -> anyhow::Result<()> {
    render_export_at_scale(director, out_path, gpu_context, audio_track_path, alpha, 1.0, false)
}

/// [`render_export`] at a draft raster scale. Layout pixels stay unchanged.
/// `draft` selects software x264 ultrafast at CRF 30 and keeps the raster size.
/// The output scale and background return to their previous values afterwards.
pub fn render_export_at_scale(
    director: &mut Director,
    out_path: PathBuf,
    mut gpu_context: Option<&mut GpuContext>,
    audio_track_path: Option<PathBuf>,
    alpha: bool,
    scale: f32,
    draft: bool,
) -> anyhow::Result<()> {
    let previous = (director.output_scale_x, director.output_scale_y, director.transparent_background);
    let (width, height) = set_output_scale(director, scale).map_err(anyhow::Error::msg)?;
    director.transparent_background = alpha;
    let result = export_scaled(director, out_path, &mut gpu_context, audio_track_path, (width, height), draft);
    (director.output_scale_x, director.output_scale_y, director.transparent_background) = previous;
    result
}

fn export_scaled(
    director: &mut Director,
    out_path: PathBuf,
    gpu_context: &mut Option<&mut GpuContext>,
    audio_track_path: Option<PathBuf>,
    (width, height): (i32, i32),
    draft: bool,
) -> anyhow::Result<()> {
    // Missing bundled fonts fail the export (plan 06 determinism).
    director.ensure_required_fonts()?;
    // The dependency graph enables incremental frames during export.
    director.compile();
    let fps = director.fps.max(1e-6);
    let alpha = director.transparent_background;
    let mut duration = director.timeline_duration();
    if duration == 0.0 {
        duration = 5.0;
    }
    if let Some(path) = audio_track_path {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(samples) = load_audio_bytes(&bytes, director.audio_mixer.sample_rate) {
                director.add_global_audio(samples);
            }
        }
    }
    let settings = EncoderSettings::preset_h264_yuv420p(width as usize, height as usize)
        .with_fps(fps)
        .with_encoder_mode(director.encoder_mode)
        .with_alpha(alpha)
        .with_draft(draft);
    let mut encoder = Encoder::new(&out_path, settings)?;
    let info = ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );
    let mut surface = render_surface(gpu_context, &info).ok_or(RenderError::Surface(width, height))?;
    let mut state = FrameState::default();
    let block = audio_samples_per_video_frame(director.audio_mixer.sample_rate, fps, 1);
    // FFmpeg reads `rgba` as straight alpha, so alpha export reads unpremultiplied pixels.
    let read_info = if alpha { info.with_alpha_type(AlphaType::Unpremul) } else { info };
    let row_bytes = width as usize * 4;
    let mut pixels = vec![0u8; height as usize * row_bytes];
    for frame in 0..duration_to_frame_count(duration, fps) {
        let time = frame_to_time(frame, fps);
        state.render(director, time, &mut surface)?;
        #[cfg(feature = "vulkan")]
        if let Some(ctx) = gpu_context.as_deref_mut() {
            ctx.flush_and_submit();
        }
        anyhow::ensure!(surface.read_pixels(&read_info, &mut pixels, row_bytes, (0, 0)), RenderError::Readback);
        encoder.encode(&pixels, time)?;
        encoder.encode_audio(&director.mix_audio(block, time), time)?;
    }
    encoder.finish()
}

#[cfg(test)]
mod tests {
    use super::{even_output_px, output_dimensions};

    #[test]
    fn half_scale_is_even_and_full_scale_keeps_odd_sizes() {
        assert_eq!(output_dimensions(64, 64, 0.5).unwrap(), (32, 32, 0.5, 0.5));
        assert_eq!(output_dimensions(101, 50, 1.0).unwrap(), (101, 50, 1.0, 1.0));
        let (w, h, sx, sy) = output_dimensions(101, 50, 0.5).unwrap();
        assert_eq!((w, h), (50, 24));
        assert!((sx - 50.0 / 101.0).abs() < 1e-6);
        assert!((sy - 24.0 / 50.0).abs() < 1e-6);
        assert!(output_dimensions(64, 64, 0.0).is_err());
        assert!(output_dimensions(64, 64, f32::NAN).is_err());
        assert!(output_dimensions(64, 64, 1.5).is_err());
        assert!(even_output_px(0, 0.5).is_err() || output_dimensions(0, 64, 0.5).is_err());
    }
}
