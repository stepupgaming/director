//! # Video Export
//!
//! Renders Director timeline to video files (MP4/WebM).
//!
//! ## Responsibilities
//! - **Frame Loop**: Iterates through all frames.
//! - **Encoding**: FFmpeg H.264/AAC encoding.
//! - **Motion Blur**: Shutter angle / multi-sample accumulation.
//! - **Audio Sync**: Mixes audio per frame.
//!
//! ## Key Functions
//! - `render_export`: Main export entry point.

use crate::audio::load_audio_bytes;
use crate::director::Director;
use crate::errors::RenderError;
use crate::systems::layout::LayoutEngine;
use crate::systems::renderer::{render_sampled_at_time, GpuContext};
use crate::video_wrapper::{Encoder, EncoderSettings};
use anyhow::Result;

use skia_safe::{AlphaType, ColorSpace, ColorType, ImageInfo, Surface};
use std::path::PathBuf;
use tracing::instrument;

/// Create the requested backend. A supplied GPU context requires a GPU target.
#[allow(unused_variables)]
fn create_render_surface(
    gpu_context: &mut Option<&mut GpuContext>,
    info: &ImageInfo,
) -> Option<Surface> {
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

/// Raster size for `scale` in `(0, 1]`. `1` keeps the layout size, including odd sizes.
/// A smaller scale rounds each side down to an even pixel count so yuv420 export accepts it.
pub fn validate_scale(scale: f32) -> Result<(), String> {
    if !scale.is_finite() || scale <= 0.0 || scale > 1.0 {
        Err("scale must be finite and in (0, 1]".into())
    } else {
        Ok(())
    }
}

pub fn output_dimensions(
    width: i32,
    height: i32,
    scale: f32,
) -> Result<(i32, i32, f32, f32), String> {
    if width <= 0 || height <= 0 {
        return Err("source size must be positive".into());
    }
    validate_scale(scale)?;
    if (scale - 1.0).abs() <= f32::EPSILON {
        return Ok((width, height, 1.0, 1.0));
    }
    let out_w = even_output_px(width, scale)?;
    let out_h = even_output_px(height, scale)?;
    Ok((
        out_w,
        out_h,
        out_w as f32 / width as f32,
        out_h as f32 / height as f32,
    ))
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

/// Renders the entire movie to a video file (MP4 or WebM with alpha).
///
/// This is a long-running blocking operation that:
/// 1. Initializes the video encoder (FFmpeg).
/// 2. Iterates through every frame of the timeline.
/// 3. Updates the Scene Graph (Update -> Layout -> PostLayout).
/// 4. Rasterizes the scene to a Skia Surface.
/// 5. Handles Motion Blur via accumulation buffer (if configured).
/// 6. Sends pixels to the encoder.
/// 7. Mixes and encodes audio.
///
/// # Arguments
/// * `director` - The director instance containing the movie state.
/// * `out_path` - Destination path for the output video file.
/// * `gpu_context` - Optional GPU context for hardware acceleration.
/// * `audio_track_path` - Optional path to a background audio track (deprecated; use `director.add_global_audio`).
/// * `alpha` - Enable alpha-channel output (uses VP9/WebM instead of H.264/MP4).
pub fn render_export(
    director: &mut Director,
    out_path: PathBuf,
    gpu_context: Option<&mut GpuContext>,
    audio_track_path: Option<PathBuf>,
    alpha: bool,
) -> Result<()> {
    render_export_at_scale(
        director,
        out_path,
        gpu_context,
        audio_track_path,
        alpha,
        1.0,
        false,
    )
}

/// Same export as [`render_export`] with a draft raster scale. Layout pixels stay unchanged.
/// `draft` selects software x264 ultrafast and CRF 30. It does not change the raster size.
#[allow(unused_mut)]
#[instrument(level = "info", skip(director, gpu_context), fields(width = director.width, height = director.height, fps = director.fps, scale = scale))]
pub fn render_export_at_scale(
    director: &mut Director,
    out_path: PathBuf,
    mut gpu_context: Option<&mut GpuContext>,
    audio_track_path: Option<PathBuf>,
    alpha: bool,
    scale: f32,
    draft: bool,
) -> Result<()> {
    let previous_x = director.output_scale_x;
    let previous_y = director.output_scale_y;
    let (out_w, out_h) = set_output_scale(director, scale).map_err(anyhow::Error::msg)?;
    let result = export_scaled(
        director,
        out_path,
        &mut gpu_context,
        audio_track_path,
        alpha,
        out_w,
        out_h,
        draft,
    );
    director.output_scale_x = previous_x;
    director.output_scale_y = previous_y;
    result
}

fn export_scaled(
    director: &mut Director,
    out_path: PathBuf,
    gpu_context: &mut Option<&mut GpuContext>,
    audio_track_path: Option<PathBuf>,
    alpha: bool,
    out_w: i32,
    out_h: i32,
    draft: bool,
) -> Result<()> {
    // Fail closed on missing bundled fonts (Plan 06 determinism).
    director.ensure_required_fonts()?;

    // Build the dependency graph before rendering so incremental
    // invalidation is available during export.
    director.compile();

    let width = out_w;
    let height = out_h;
    let fps = director.fps.max(1e-6);

    let mut max_duration = director.timeline_duration();
    if max_duration == 0.0 {
        max_duration = 5.0;
    }

    let total_frames = crate::timeline::duration_to_frame_count(max_duration, fps) as usize;

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

    let info = skia_safe::ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );

    // Use a GPU target when the caller supplies a Vulkan context.
    let mut surface =
        create_render_surface(gpu_context, &info).ok_or(RenderError::SurfaceFailure)?;

    let mut accumulation_surface = if director.samples_per_frame > 1 {
        Some(
            surface
                .new_surface(&info)
                .ok_or(RenderError::SurfaceFailure)?,
        )
    } else {
        None
    };

    let mut layout_engine = LayoutEngine::new();

    // The mixer takes sample frames and emits two interleaved samples per frame.
    let audio_frames_per_video_frame =
        crate::timeline::audio_samples_per_video_frame(director.audio_mixer.sample_rate, fps, 1);

    // Pre-allocate transition surfaces to avoid per-frame allocation churn
    let mut transition_surfaces = if !director.transitions.is_empty() {
        let surf_a =
            create_render_surface(gpu_context, &info).ok_or(RenderError::SurfaceFailure)?;
        let surf_b =
            create_render_surface(gpu_context, &info).ok_or(RenderError::SurfaceFailure)?;
        Some((surf_a, surf_b))
    } else {
        None
    };

    let row_bytes = width as usize * 4;
    let mut pixels = vec![0u8; height as usize * row_bytes];
    for i in 0..total_frames {
        let frame_start_time = crate::timeline::frame_to_time(i as u64, fps);
        render_sampled_at_time(
            director,
            &mut layout_engine,
            frame_start_time,
            &mut surface,
            &mut accumulation_surface,
            &mut transition_surfaces,
        )?;

        #[cfg(feature = "vulkan")]
        if let Some(ctx) = gpu_context.as_deref_mut() {
            ctx.flush_and_submit();
        }
        anyhow::ensure!(
            surface.read_pixels(&info, &mut pixels, row_bytes, (0, 0)),
            "Failed to read pixels from render surface"
        );
        encoder.encode(&pixels, crate::timeline::frame_to_time(i as u64, fps))?;

        let audio_samples = director.mix_audio(audio_frames_per_video_frame, frame_start_time);
        encoder.encode_audio(&audio_samples, frame_start_time)?;
    }

    encoder.finish()?;

    Ok(())
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
