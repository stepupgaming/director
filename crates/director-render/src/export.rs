//! Document export: workers each take the next frame on a CPU raster or on
//! their own GPU context, or one session on a given GPU context paints frames
//! in order, while a writer thread puts painted
//! frames back in order and pipes them to one FFmpeg process that also encodes
//! the premixed audio track. FFmpeg starts from the document alone, so it
//! launches while the writer imports and mixes the audio.

use crate::{set_output_scale, Frame, FrameSession, GpuContext, RenderError, Renderer, TargetSession};
use director_core::scene_export::export_scene;
use director_core::scene_import::{import_scene, ImportRegistry};
use director_core::timeline::{audio_samples_per_video_frame, duration_to_frame_count, frame_to_time};
use director_core::video_wrapper::{Encoder, EncoderMode, EncoderSettings};
use director_core::Director;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};

/// Frame buffers per CPU worker: one painting while another waits to encode.
const WORKER_FRAMES: usize = 2;
/// Frame buffers for one session, so a brief FFmpeg stall leaves the painter
/// room to run ahead.
const SESSION_FRAMES: usize = 4;
/// GPU pool workers when [`ExportOptions::workers`] is unset. More contexts
/// keep the GPU busy while others record or read back.
const GPU_WORKERS: usize = 3;

/// Makes a GPU context on the calling thread, which must then own it.
pub type GpuContexts = Arc<dyn Fn() -> Option<GpuContext> + Send + Sync>;

/// Settings of one export beyond the document.
#[derive(Clone, Default)]
pub struct ExportOptions {
    pub mode: EncoderMode,
    /// Software x264 ultrafast at CRF 30.
    pub draft: bool,
    /// Pool workers; `None` sizes a CPU pool by [`Renderer::pool_for`] and a
    /// GPU pool to three.
    pub workers: Option<usize>,
    /// Gives each pool worker its own GPU context. A GPU export with this set
    /// paints on a pool of contexts in place of the one passed in.
    pub gpu_contexts: Option<GpuContexts>,
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
    /// `Some(workers)` threads, or a default count, on CPU rasters or on GPU
    /// contexts each worker makes for itself.
    Pool(Option<usize>, Option<GpuContexts>),
    /// One session on a GPU context, or on a CPU raster when `None`.
    Session(Option<&'a mut GpuContext>),
}

/// The session of one pool worker, with the GPU context it made.
enum PoolSession {
    Cpu(FrameSession),
    Gpu(TargetSession, GpuContext),
}

impl PoolSession {
    fn new(renderer: &Renderer, contexts: Option<&GpuContexts>) -> Result<Self, RenderError> {
        let Some(contexts) = contexts else { return Ok(Self::Cpu(renderer.session()?)) };
        let mut context = contexts().ok_or(RenderError::GpuContext)?;
        Ok(Self::Gpu(TargetSession::new(renderer, Some(&mut context))?, context))
    }

    fn render_into(&mut self, time: f64, frame: &mut Frame) -> Result<(), RenderError> {
        match self {
            Self::Cpu(session) => session.render_into(time, frame),
            Self::Gpu(session, context) => session.render_into(time, frame, Some(context)),
        }
    }
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

    /// Export with `options`: on a pool of `options.gpu_contexts` when `gpu`
    /// is given with them, on one session on `gpu` without them, and otherwise
    /// on a CPU pool of `options.workers`.
    pub fn export_options(&self, path: &Path, options: &ExportOptions, gpu: Option<&mut GpuContext>) -> Result<(), RenderError> {
        let target = match (gpu, &options.gpu_contexts) {
            (Some(_), Some(contexts)) => Target::Pool(options.workers, Some(contexts.clone())),
            (Some(gpu), None) => Target::Session(Some(gpu)),
            (None, _) => Target::Pool(options.workers, None),
        };
        self.export_encoded(path, options, target)
    }

    fn export_encoded(&self, path: &Path, options: &ExportOptions, target: Target) -> Result<(), RenderError> {
        let fps = self.document.fps.max(1e-6);
        let times = self.frame_times();
        let workers = match &target {
            Target::Pool(Some(workers), _) => (*workers).max(1),
            Target::Pool(None, Some(_)) => GPU_WORKERS.min(times.len()).max(1),
            Target::Pool(None, None) => self.pool_for(times.len()).workers(),
            Target::Session(_) => 1,
        };
        let buffers = match &target {
            Target::Pool(..) => workers * WORKER_FRAMES,
            Target::Session(_) => SESSION_FRAMES,
        };
        let (width, height) = self.output_size();
        let mut settings = EncoderSettings::preset_h264_yuv420p(width as usize, height as usize)
            .with_fps(fps)
            .with_encoder_mode(options.mode)
            .with_alpha(self.document.transparent_background)
            .with_draft(options.draft);
        settings.sample_rate = self.document.audio.sample_rate as i32;
        // `None` reports a failed worker, so the writer stops waiting for its frame.
        let (full_tx, full_rx) = mpsc::channel::<Option<(usize, Frame)>>();
        let (empty_tx, empty_rx) = mpsc::channel::<Frame>();
        for _ in 0..buffers {
            let _ = empty_tx.send(Frame {
                width: 0,
                height: 0,
                rgba: Vec::new(),
            });
        }
        let failed = AtomicBool::new(false);
        // Workers share the free buffers and the index of the next frame to paint.
        let (empty_rx, next) = (Mutex::new(empty_rx), AtomicUsize::new(0));
        std::thread::scope(|scope| {
            let (times, failed) = (&times, &failed);
            let progress = options.progress.clone();
            // The writer starts FFmpeg, then imports and mixes the whole track while
            // FFmpeg launches and the first frames paint.
            let writer = scope.spawn(move || -> anyhow::Result<()> {
                // A document without audio tracks exports a video stream alone.
                let silent = self.document.audio.tracks.is_empty();
                let mut encoder = if silent {
                    Encoder::video_only(path, settings)?
                } else {
                    Encoder::with_audio_input(path, settings)?
                };
                let written = (|| {
                    if !silent {
                        let mut audio = import_scene(&self.document, self.loader.clone(), &self.registry, self.render_mode)?;
                        let block = audio_samples_per_video_frame(audio.audio_mixer.sample_rate, fps, 1);
                        encoder.send_audio(times.iter().flat_map(|&time| audio.mix_audio(block, time)).collect())?;
                    }
                    // Workers finish out of order; frames wait here for their turn.
                    let mut waiting = BTreeMap::new();
                    let mut next = 0;
                    for (index, frame) in full_rx.iter().map_while(|painted| painted) {
                        waiting.insert(index, frame);
                        while let Some(frame) = waiting.remove(&next) {
                            encoder.encode(&frame.rgba, times[next])?;
                            next += 1;
                            if let Some(progress) = &progress {
                                progress(next, times.len());
                            }
                            let _ = empty_tx.send(frame);
                        }
                    }
                    anyhow::Ok(())
                })();
                // A failed paint or write leaves nothing at `path`.
                if written.is_err() || failed.load(Ordering::Acquire) {
                    encoder.abort();
                    return written;
                }
                encoder.finish()
            });
            let cancelled = || options.cancel.as_ref().is_some_and(|cancel| cancel.load(Ordering::Acquire));
            let painted: Result<(), RenderError> = match target {
                Target::Pool(_, contexts) => {
                    // A worker holds a buffer before it takes an index, so the
                    // lowest unencoded frame always has a buffer to paint into.
                    let (next, empty_rx) = (&next, &empty_rx);
                    let workers: Vec<_> = (0..workers)
                        .map(|_| {
                            let (full_tx, contexts) = (full_tx.clone(), contexts.clone());
                            scope.spawn(move || -> Result<(), RenderError> {
                                let mut session = None;
                                let painted = (|| loop {
                                    if failed.load(Ordering::Acquire) {
                                        return Ok(());
                                    }
                                    if cancelled() {
                                        return Err(RenderError::Cancelled);
                                    }
                                    // The writer hung up, so it failed; its join reports why.
                                    let Ok(mut frame) = empty_rx.lock().unwrap().recv() else { return Ok(()) };
                                    let index = next.fetch_add(1, Ordering::Relaxed);
                                    let Some(&time) = times.get(index) else { return Ok(()) };
                                    let session = match &mut session {
                                        Some(session) => session,
                                        None => session.insert(PoolSession::new(self, contexts.as_ref())?),
                                    };
                                    session.render_into(time, &mut frame)?;
                                    if full_tx.send(Some((index, frame))).is_err() {
                                        return Ok(());
                                    }
                                })();
                                if painted.is_err() {
                                    failed.store(true, Ordering::Release);
                                    let _ = full_tx.send(None);
                                }
                                painted
                            })
                        })
                        .collect();
                    // A failed worker stops the others at their next frame.
                    workers
                        .into_iter()
                        .map(|worker| worker.join().unwrap_or(Err(RenderError::Readback)))
                        .fold(Ok(()), Result::and)
                }
                Target::Session(context) => (|| {
                    let mut session = SerialSession::new(self, context)?;
                    for (index, &time) in times.iter().enumerate() {
                        if cancelled() {
                            return Err(RenderError::Cancelled);
                        }
                        let Ok(mut frame) = empty_rx.lock().unwrap().recv() else { break };
                        session.render_into(time, &mut frame)?;
                        if full_tx.send(Some((index, frame))).is_err() {
                            break;
                        }
                    }
                    Ok(())
                })(),
            };
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
    export_director_with(director, registry, path, alpha, scale, &options, gpu)
}

/// [`export_director`] with full `options`, whose `gpu_contexts` turn a `gpu`
/// export into a pool of contexts.
pub fn export_director_with(
    director: &mut Director,
    registry: ImportRegistry,
    path: &Path,
    alpha: bool,
    scale: f32,
    options: &ExportOptions,
    gpu: Option<&mut GpuContext>,
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
        .export_options(path, options, gpu)?;
    Ok(())
}
