//! Document export: a [`FramePool`] paints the next batch of frames while a
//! writer thread pipes the previous batch to one FFmpeg process that also
//! encodes the premixed audio track.

use crate::{set_output_scale, Frame, RenderError, Renderer};
use director_core::scene_export::export_scene;
use director_core::scene_import::{import_scene, ImportRegistry};
use director_core::timeline::{audio_samples_per_video_frame, duration_to_frame_count, frame_to_time};
use director_core::video_wrapper::{Encoder, EncoderMode, EncoderSettings};
use director_core::Director;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

/// Frame batches in flight: one painting while one encodes.
const BATCHES: usize = 2;

/// Export frame times of `director`: its timeline, or five seconds when it is empty.
pub fn frame_times(director: &Director) -> Vec<f64> {
    let fps = director.fps.max(1e-6);
    let mut duration = director.timeline_duration();
    if duration == 0.0 {
        duration = 5.0;
    }
    (0..duration_to_frame_count(duration, fps)).map(|i| frame_to_time(i, fps)).collect()
}

impl Renderer {
    /// Export the document to `path` as H.264 (VP9 with alpha for a transparent
    /// background) with mixed audio, through a pool sized by [`Renderer::pool_for`].
    pub fn export(&self, path: &Path, mode: EncoderMode) -> Result<(), RenderError> {
        self.export_with(path, mode, None)
    }

    /// Export through `workers` sessions, or a pool sized to the frame count
    /// when `None`. Each pool call paints one frame per worker.
    pub fn export_with(&self, path: &Path, mode: EncoderMode, workers: Option<usize>) -> Result<(), RenderError> {
        self.export_encoded(path, mode, workers, false)
    }

    /// `draft` selects software x264 ultrafast at CRF 30.
    fn export_encoded(
        &self,
        path: &Path,
        mode: EncoderMode,
        workers: Option<usize>,
        draft: bool,
    ) -> Result<(), RenderError> {
        let mut audio = import_scene(&self.document, self.loader.clone(), &self.registry, self.render_mode)?;
        let fps = audio.fps.max(1e-6);
        let times = frame_times(&audio);
        let mut pool = workers.map_or_else(|| self.pool_for(times.len()), |workers| self.pool_with(workers));
        let (width, height) = self.output_size();
        let settings = EncoderSettings::preset_h264_yuv420p(width as usize, height as usize)
            .with_fps(fps)
            .with_encoder_mode(mode)
            .with_alpha(audio.transparent_background)
            .with_draft(draft);
        let block = audio_samples_per_video_frame(audio.audio_mixer.sample_rate, fps, 1);
        let batch = pool.workers();
        let (full_tx, full_rx) = mpsc::sync_channel::<(usize, Vec<Frame>)>(BATCHES);
        let (empty_tx, empty_rx) = mpsc::channel::<Vec<Frame>>();
        for _ in 0..BATCHES {
            let _ = empty_tx.send(Vec::new());
        }
        let failed = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let (times, failed) = (&times, &failed);
            // The writer mixes the whole track and starts FFmpeg while the first batch paints.
            let writer = scope.spawn(move || -> anyhow::Result<()> {
                let samples: Vec<f32> = times.iter().flat_map(|&time| audio.mix_audio(block, time)).collect();
                let mut encoder = Encoder::with_audio(path, settings, &samples)?;
                let written = full_rx.iter().try_for_each(|(start, frames)| {
                    for (frame, &time) in frames.iter().zip(&times[start..]) {
                        encoder.encode(&frame.rgba, time)?;
                    }
                    let _ = empty_tx.send(frames);
                    anyhow::Ok(())
                });
                // A failed paint or write leaves nothing at `path`.
                if written.is_err() || failed.load(Ordering::Acquire) {
                    encoder.abort();
                    return written;
                }
                encoder.finish()
            });
            let painted: Result<(), RenderError> = (|| {
                for (index, chunk) in times.chunks(batch).enumerate() {
                    // The writer hung up, so it failed; its join reports why.
                    let Ok(mut frames) = empty_rx.recv() else { break };
                    pool.render_into(chunk, &mut frames)?;
                    if full_tx.send((index * batch, frames)).is_err() {
                        break;
                    }
                }
                Ok(())
            })();
            if painted.is_err() {
                failed.store(true, Ordering::Release);
            }
            drop(full_tx);
            let written = writer.join().unwrap_or_else(|_| Err(anyhow::anyhow!("the encoder thread panicked")));
            painted?;
            written.map_err(RenderError::Encode)
        })
    }
}

/// Export the timeline of a live `director` to `path` through its scene document
/// and the pipelined [`Renderer::export`]: H.264, or VP9 with alpha when `alpha`
/// is set, at raster `scale` in `(0, 1]`, with `draft` selecting software x264
/// ultrafast at CRF 30. `registry` decodes the document's frontend kinds. The
/// output scale and background return to their previous values afterwards.
pub fn export_director(
    director: &mut Director,
    registry: ImportRegistry,
    path: &Path,
    alpha: bool,
    scale: f32,
    draft: bool,
) -> anyhow::Result<()> {
    let previous = (director.output_scale_x, director.output_scale_y, director.transparent_background);
    let document = set_output_scale(director, scale).map_err(anyhow::Error::msg).and_then(|_| {
        director.transparent_background = alpha;
        // Missing bundled fonts fail the export (plan 06 determinism).
        director.ensure_required_fonts()?;
        Ok(export_scene(director)?)
    });
    (director.output_scale_x, director.output_scale_y, director.transparent_background) = previous;
    Renderer::new(Arc::new(document?), director.assets.loader.clone())
        .with_registry(registry)
        .with_render_mode(director.render_mode)
        .export_encoded(path, director.encoder_mode, None, draft)?;
    Ok(())
}
