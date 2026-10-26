//! Document export: a [`FramePool`], or one session on a GPU context, paints
//! the next batch of frames while a writer thread pipes the previous batch to
//! one FFmpeg process that also encodes the premixed audio track. FFmpeg starts
//! from the document alone, so it launches while the writer imports and mixes
//! the audio.

use crate::{set_output_scale, Frame, FramePool, GpuContext, RenderError, Renderer, TargetSession};
use director_core::scene_export::export_scene;
use director_core::scene_import::{import_scene, ImportRegistry};
use director_core::timeline::{audio_samples_per_video_frame, duration_to_frame_count, frame_to_time};
use director_core::video_wrapper::{Encoder, EncoderMode, EncoderSettings};
use director_core::Director;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

/// Pool frame batches in flight: one painting while one encodes.
const BATCHES: usize = 2;
/// Single-frame batches in flight for one session, so a brief FFmpeg stall
/// leaves the painter room to run ahead.
const SESSION_FRAMES: usize = 4;

/// Settings of one export beyond the document.
#[derive(Clone, Default)]
pub struct ExportOptions {
    pub mode: EncoderMode,
    /// Software x264 ultrafast at CRF 30.
    pub draft: bool,
    /// Pool sessions for a CPU export; `None` sizes the pool by [`Renderer::pool_for`].
    pub workers: Option<usize>,
    /// Checked before each batch paints. A set flag ends the export with
    /// [`RenderError::Cancelled`] and leaves nothing at the output path.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Called on the writer thread with `(encoded, total)` frames after each batch.
    pub progress: Option<Arc<dyn Fn(usize, usize) + Send + Sync>>,
}

impl ExportOptions {
    pub fn new(mode: EncoderMode) -> Self {
        Self {
            mode,
            ..Self::default()
        }
    }
}

/// Where an export paints.
enum Target<'a> {
    /// A pool of `Some(workers)` CPU sessions, or one sized by [`Renderer::pool_for`].
    Pool(Option<usize>),
    /// One session on a GPU context, or on a CPU raster when `None`.
    Session(Option<&'a mut GpuContext>),
}

/// One [`TargetSession`] with the export's GPU context, or a CPU raster.
struct SerialSession<'a> {
    session: TargetSession,
    context: Option<&'a mut GpuContext>,
}

impl<'a> SerialSession<'a> {
    fn new(renderer: &Renderer, mut context: Option<&'a mut GpuContext>) -> Result<Self, RenderError> {
        let session = TargetSession::new(renderer, context.as_deref_mut())?;
        Ok(Self { session, context })
    }

    fn render_into(&mut self, time: f64, frame: &mut Frame) -> Result<(), RenderError> {
        self.session.render_into(time, frame, self.context.as_deref_mut())
    }
}

/// The painter of one export, created on the calling thread once FFmpeg is starting.
enum Painter<'a> {
    Pool(FramePool),
    Session(Box<SerialSession<'a>>),
}

impl Painter<'_> {
    fn render_into(&mut self, times: &[f64], frames: &mut Vec<Frame>) -> Result<(), RenderError> {
        match self {
            Painter::Pool(pool) => pool.render_into(times, frames),
            Painter::Session(session) => {
                frames.resize_with(times.len(), || Frame {
                    width: 0,
                    height: 0,
                    rgba: Vec::new(),
                });
                times.iter().zip(frames).try_for_each(|(&time, frame)| session.render_into(time, frame))
            }
        }
    }
}

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
    /// Export frame times of the document, equal to [`frame_times`] of its import.
    pub fn frame_times(&self) -> Vec<f64> {
        let document = &self.document;
        let fps = document.fps.max(1e-6);
        let clips = document.timeline.tracks.iter().flat_map(|track| &track.clips);
        let mut duration = clips.map(|clip| clip.start_time + clip.duration).fold(0.0, f64::max);
        if duration == 0.0 {
            duration = 5.0;
        }
        (0..duration_to_frame_count(duration, fps)).map(|i| frame_to_time(i, fps)).collect()
    }

    /// Export the document to `path` as H.264 (VP9 with alpha for a transparent
    /// background) with mixed audio, through a pool sized by [`Renderer::pool_for`].
    pub fn export(&self, path: &Path, mode: EncoderMode) -> Result<(), RenderError> {
        self.export_with(path, mode, None)
    }

    /// Export through `workers` sessions, or a pool sized to the frame count
    /// when `None`. Each pool call paints one frame per worker.
    pub fn export_with(&self, path: &Path, mode: EncoderMode, workers: Option<usize>) -> Result<(), RenderError> {
        let options = ExportOptions {
            workers,
            ..ExportOptions::new(mode)
        };
        self.export_options(path, &options, None)
    }

    /// Export through one session that paints on `context`, a Vulkan context
    /// in a `vulkan` build, or on a CPU raster when `None`. The session paints
    /// on the calling thread while the writer thread encodes.
    pub fn export_on(&self, path: &Path, mode: EncoderMode, context: Option<&mut GpuContext>) -> Result<(), RenderError> {
        self.export_encoded(path, &ExportOptions::new(mode), Target::Session(context))
    }

    /// Export with `options`: on one session on `gpu` when given, otherwise
    /// on a CPU pool of `options.workers`.
    pub fn export_options(&self, path: &Path, options: &ExportOptions, gpu: Option<&mut GpuContext>) -> Result<(), RenderError> {
        let target = gpu.map_or(Target::Pool(options.workers), |gpu| Target::Session(Some(gpu)));
        self.export_encoded(path, options, target)
    }

    fn export_encoded(&self, path: &Path, options: &ExportOptions, target: Target) -> Result<(), RenderError> {
        let fps = self.document.fps.max(1e-6);
        let times = self.frame_times();
        let (batch, in_flight) = match &target {
            Target::Pool(Some(workers)) => ((*workers).max(1), BATCHES),
            Target::Pool(None) => (self.pool_for(times.len()).workers(), BATCHES),
            Target::Session(_) => (1, SESSION_FRAMES),
        };
        let (width, height) = self.output_size();
        let mut settings = EncoderSettings::preset_h264_yuv420p(width as usize, height as usize)
            .with_fps(fps)
            .with_encoder_mode(options.mode)
            .with_alpha(self.document.transparent_background)
            .with_draft(options.draft);
        settings.sample_rate = self.document.audio.sample_rate as i32;
        let (full_tx, full_rx) = mpsc::sync_channel::<(usize, Vec<Frame>)>(in_flight);
        let (empty_tx, empty_rx) = mpsc::channel::<Vec<Frame>>();
        for _ in 0..in_flight {
            let _ = empty_tx.send(Vec::new());
        }
        let failed = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let (times, failed) = (&times, &failed);
            let progress = options.progress.clone();
            // The writer starts FFmpeg, then imports and mixes the whole track while
            // FFmpeg launches and the first batch paints.
            let writer = scope.spawn(move || -> anyhow::Result<()> {
                let mut encoder = Encoder::with_audio_input(path, settings)?;
                let written = (|| {
                    let mut audio = import_scene(&self.document, self.loader.clone(), &self.registry, self.render_mode)?;
                    let block = audio_samples_per_video_frame(audio.audio_mixer.sample_rate, fps, 1);
                    encoder.send_audio(times.iter().flat_map(|&time| audio.mix_audio(block, time)).collect())?;
                    full_rx.iter().try_for_each(|(start, frames)| {
                        for (frame, &time) in frames.iter().zip(&times[start..]) {
                            encoder.encode(&frame.rgba, time)?;
                        }
                        if let Some(progress) = &progress {
                            progress(start + frames.len(), times.len());
                        }
                        let _ = empty_tx.send(frames);
                        anyhow::Ok(())
                    })
                })();
                // A failed paint or write leaves nothing at `path`.
                if written.is_err() || failed.load(Ordering::Acquire) {
                    encoder.abort();
                    return written;
                }
                encoder.finish()
            });
            let painted: Result<(), RenderError> = (|| {
                let mut painter = match target {
                    Target::Pool(_) => Painter::Pool(self.pool_with(batch)),
                    Target::Session(context) => Painter::Session(Box::new(SerialSession::new(self, context)?)),
                };
                for (index, chunk) in times.chunks(batch).enumerate() {
                    if options.cancel.as_ref().is_some_and(|cancel| cancel.load(Ordering::Acquire)) {
                        return Err(RenderError::Cancelled);
                    }
                    // The writer hung up, so it failed; its join reports why.
                    let Ok(mut frames) = empty_rx.recv() else { break };
                    painter.render_into(chunk, &mut frames)?;
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
/// ultrafast at CRF 30. `registry` decodes the document's frontend kinds. A
/// `gpu` context paints through [`Renderer::export_on`], and `None` through a
/// CPU pool. The output scale and background return to their previous values
/// afterwards.
pub fn export_director(
    director: &mut Director,
    registry: ImportRegistry,
    path: &Path,
    alpha: bool,
    scale: f32,
    draft: bool,
    gpu: Option<&mut GpuContext>,
) -> anyhow::Result<()> {
    let options = ExportOptions {
        draft,
        ..ExportOptions::new(director.encoder_mode)
    };
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
        .export_options(path, &options, gpu)?;
    Ok(())
}
