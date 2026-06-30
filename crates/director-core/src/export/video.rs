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
use crate::systems::renderer::{render_at_time, GpuContext};
use crate::video_wrapper::{Encoder, EncoderSettings};
use anyhow::Result;

use skia_safe::{AlphaType, ColorSpace, ColorType};
use std::path::PathBuf;
use tracing::instrument;

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
#[allow(unused_variables)]
#[allow(unused_mut)]
#[instrument(level = "info", skip(director, gpu_context), fields(width = director.width, height = director.height, fps = director.fps))]
pub fn render_export(
    director: &mut Director,
    out_path: PathBuf,
    gpu_context: Option<&mut GpuContext>,
    audio_track_path: Option<PathBuf>,
    alpha: bool,
) -> Result<()> {
    let width = director.width;
    let height = director.height;
    let fps = director.fps;

    let mut max_duration = 0.0;
    for item in &director.timeline {
        let end = item.start_time + item.duration;
        if end > max_duration {
            max_duration = end;
        }
    }
    if max_duration == 0.0 {
        max_duration = 5.0;
    }

    let total_frames = (max_duration * fps as f64).ceil() as usize;

    if let Some(path) = audio_track_path {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(samples) = load_audio_bytes(&bytes, director.audio_mixer.sample_rate) {
                director.add_global_audio(samples);
            }
        }
    }

    let settings = EncoderSettings::preset_h264_yuv420p(width as usize, height as usize)
        .with_encoder_mode(director.encoder_mode)
        .with_alpha(alpha);
    let mut encoder = Encoder::new(&out_path, settings)?;

    let info = skia_safe::ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );

    // Always render to a CPU raster surface. A GPU (Vulkan) render-target
    // surface complicates pixel readback (`read_pixels_with_context`); a CPU
    // raster surface lets us read pixels directly via `peek_pixels`, which
    // guarantees valid output with or without `--gpu`. The `gpu_context` is
    // still accepted so the `--gpu` flag remains honored (Vulkan is initialized
    // in the CLI) and is available for future shader/acceleration work; it is
    // intentionally unused for the render target here.
    let mut surface = skia_safe::surfaces::raster(&info, None, None)
        .ok_or(RenderError::SurfaceFailure)?;

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

    let samples = director.samples_per_frame.max(1);
    let shutter_angle = director.shutter_angle.clamp(0.0, 360.0);
    let frame_duration = 1.0 / fps as f64;
    let shutter_duration = frame_duration * (shutter_angle as f64 / 360.0);

    let samples_per_frame = (director.audio_mixer.sample_rate as f64 / fps as f64).round() as usize;

    // Pre-allocate transition surfaces to avoid per-frame allocation churn
    let mut transition_surfaces = if !director.transitions.is_empty() {
        let surf_a =
            skia_safe::surfaces::raster(&info, None, None).ok_or(RenderError::SurfaceFailure)?;
        let surf_b =
            skia_safe::surfaces::raster(&info, None, None).ok_or(RenderError::SurfaceFailure)?;
        Some((surf_a, surf_b))
    } else {
        None
    };

    for i in 0..total_frames {
        let frame_start_time = i as f64 / fps as f64;
        let shutter_start_time = frame_start_time - (shutter_duration / 2.0);

        if samples == 1 {
            render_at_time(
                director,
                &mut layout_engine,
                frame_start_time,
                surface.canvas(),
                &mut transition_surfaces,
            )?;
        } else {
            let scratch_surface = accumulation_surface.as_mut().unwrap();

            for s in 0..samples {
                let t_offset = if samples > 1 {
                    (s as f64 / (samples - 1) as f64) * shutter_duration
                } else {
                    0.0
                };
                let sample_time = shutter_start_time + t_offset;

                render_at_time(
                    director,
                    &mut layout_engine,
                    sample_time,
                    scratch_surface.canvas(),
                    &mut transition_surfaces,
                )?;

                let weight = 1.0 / (s as f32 + 1.0);
                let mut paint = skia_safe::Paint::default();
                paint.set_alpha_f(weight);
                let image = scratch_surface.image_snapshot();

                if s == 0 {
                    surface.canvas().clear(skia_safe::Color::BLACK);
                }
                surface.canvas().draw_image(&image, (0, 0), Some(&paint));
            }
        }

        // Read pixels directly from the CPU raster surface. `peek_pixels` is
        // always available on a raster surface, so this works with or without
        // the `--gpu` flag (which only controls Vulkan context initialization).
        if let Some(pixmap) = surface.peek_pixels() {
            if let Some(raw) = pixmap.bytes() {
                if raw.len() == width as usize * height as usize * 4 {
                    encoder.encode(raw, i as f64 / fps as f64)?;
                }
            }
        }

        let audio_samples = director.mix_audio(samples_per_frame, frame_start_time);
        encoder.encode_audio(&audio_samples, frame_start_time)?;
    }

    encoder.finish()?;

    Ok(())
}
