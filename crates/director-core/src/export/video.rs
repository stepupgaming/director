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
#[allow(unused_mut)]
#[instrument(level = "info", skip(director, gpu_context), fields(width = director.width, height = director.height, fps = director.fps))]
pub fn render_export(
    director: &mut Director,
    out_path: PathBuf,
    mut gpu_context: Option<&mut GpuContext>,
    audio_track_path: Option<PathBuf>,
    alpha: bool,
) -> Result<()> {
    // Fail closed on missing bundled fonts (Plan 06 determinism).
    director.ensure_required_fonts()?;

    // Build the dependency graph before rendering so incremental
    // invalidation is available during export.
    director.compile();

    let width = director.width;
    let height = director.height;
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
        .with_alpha(alpha);
    let mut encoder = Encoder::new(&out_path, settings)?;

    let info = skia_safe::ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );

    // Use a GPU target when the caller supplies a Vulkan context.
    let mut surface =
        create_render_surface(&mut gpu_context, &info).ok_or(RenderError::SurfaceFailure)?;

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
            create_render_surface(&mut gpu_context, &info).ok_or(RenderError::SurfaceFailure)?;
        let surf_b =
            create_render_surface(&mut gpu_context, &info).ok_or(RenderError::SurfaceFailure)?;
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
