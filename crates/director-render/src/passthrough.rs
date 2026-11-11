//! Export over a passthrough base video. When the bottom of every frame is
//! one opaque 8-bit 4:2:0 video filling the output unscaled, FFmpeg decodes
//! that video, blends the painted layers over it in YUV and encodes, so the
//! video never crosses a pipe and keeps its samples and color tags. Workers paint only what lies above the video, on a clear surface,
//! and send the band of rows that paint reaches.
//!
//! A plan samples the timeline: each sampled frame must clear the surface
//! under the same video, one source frame per output frame. The export then
//! checks every frame the same way and checks that nothing paints outside the
//! band. A frame that fails ends the attempt and the caller exports again,
//! over the whole frame for a band miss and through the full paint otherwise.

use crate::{Frame, FrameSession, RenderError, Renderer};
use director_core::passthrough::Punch;
use director_core::timeline::{audio_samples_per_video_frame, frame_to_time};
use director_core::video_wrapper::{Encoder, EncoderSettings, Underlay};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex};

/// Timeline frames a plan paints to find the base video and the band.
const SAMPLES: usize = 24;
/// Rows kept clear above and below the sampled paint, at least.
const MARGIN: usize = 32;
/// Frames a second the painters stay ahead of, above what an encoder takes.
const PAINT_RATE: f64 = 240.0;

/// The base video of an export and the rows the layers above it paint.
pub(crate) struct Base {
    first: Punch,
    /// Top row and row count of the band the workers send.
    band: (usize, usize),
    /// Seconds one sampled frame took to paint.
    paint: f64,
    /// The session that sampled the timeline, for the first worker.
    session: Mutex<Option<FrameSession>>,
}

/// How an export over a base video ended.
pub(crate) enum Outcome {
    Done,
    /// A frame painted outside the band.
    Band,
    /// A frame had no base video or another source frame under it.
    Base,
}

impl Base {
    /// The source frame under output frame `index`.
    fn expects(&self, index: usize, punch: &Punch) -> bool {
        punch.path == self.first.path && punch.index == self.first.index + index as u64
    }

    /// The band holds all `height` rows.
    pub(crate) fn is_whole(&self, height: usize) -> bool {
        self.band == (0, height)
    }

    /// The same base with a band of all `height` rows.
    pub(crate) fn whole(mut self, height: usize) -> Self {
        self.band = (0, height);
        self
    }
}

impl FrameSession {
    /// Paint what lies above a passthrough base video at `time` on a clear
    /// surface. `None` when the frame has no such base.
    fn render_overlay(&mut self, time: f64) -> Result<Option<Punch>, RenderError> {
        let pixels = self.surface.peek_pixels().ok_or(RenderError::Readback)?.addr() as usize;
        let size = (self.info.width(), self.info.height());
        let transparent = std::mem::replace(&mut self.director.transparent_background, true);
        director_core::passthrough::begin(pixels, size);
        let painted = self.state.render(&mut self.director, time, &mut self.surface);
        let punch = director_core::passthrough::end();
        self.director.transparent_background = transparent;
        painted?;
        Ok(punch)
    }

    /// First and one past the last row of the surface holding any paint.
    fn painted_rows(&mut self) -> Option<(usize, usize)> {
        let pixmap = self.surface.peek_pixels()?;
        let (row_bytes, bytes) = (pixmap.row_bytes(), pixmap.bytes()?);
        let painted = |row: &[u8]| row.as_chunks::<4>().0.iter().fold(0, |any, pixel| any | pixel[3]) != 0;
        let first = bytes.chunks_exact(row_bytes).position(painted)?;
        let last = bytes.chunks_exact(row_bytes).rposition(painted)?;
        Some((first, last + 1))
    }

    /// Read `rows` rows from `top` into `frame` with straight alpha.
    fn read_rows(&mut self, (top, rows): (usize, usize), frame: &mut Frame) -> Result<(), RenderError> {
        let width = self.info.width() as usize;
        frame.width = width as u32;
        frame.height = rows as u32;
        frame.rgba.resize(width * rows * 4, 0);
        let info = self.info.with_dimensions((width as i32, rows as i32)).with_alpha_type(skia_safe::AlphaType::Unpremul);
        director_core::counters::count(director_core::counters::Counter::readbacks);
        if !self.surface.read_pixels(&info, &mut frame.rgba, width * 4, (0, top as i32)) {
            return Err(RenderError::Readback);
        }
        Ok(())
    }
}

impl Renderer {
    /// The base video FFmpeg can composite for the frames at `times`, found
    /// by painting a sample of them. `Err` sends the export through the full
    /// paint, with the session that painted the sample for its first worker.
    pub(crate) fn passthrough_base(&self, times: &[f64]) -> Result<Result<Base, Option<FrameSession>>, RenderError> {
        let document = &self.document;
        let has_video = document.assets.iter().any(|asset| asset.media_type.starts_with("video/"));
        if times.is_empty() || !has_video || document.transparent_background || document.motion_blur.samples_per_frame > 1 {
            return Ok(Err(None));
        }
        let mut session = self.session()?;
        let (_, height) = self.output_size();
        let started = std::time::Instant::now();
        let mut samples: Vec<usize> = (0..SAMPLES).map(|i| i * (times.len() - 1) / (SAMPLES - 1)).collect();
        samples.dedup();
        let mut first = None;
        let mut rows: Option<(usize, usize)> = None;
        for index in samples {
            let Some(punch) = session.render_overlay(times[index])? else { return Ok(Err(Some(session))) };
            let first = first.get_or_insert_with(|| punch.clone());
            // Source frames pair with output frames one to one.
            if (first.fps - document.fps).abs() > 1e-6 || punch.path != first.path || punch.index != first.index + index as u64 {
                return Ok(Err(Some(session)));
            }
            if let Some((top, bottom)) = session.painted_rows() {
                rows = Some(rows.map_or((top, bottom), |(a, b)| (a.min(top), b.max(bottom))));
            }
        }
        let Some(first) = first else { return Ok(Err(Some(session))) };
        let paint = started.elapsed().as_secs_f64() / SAMPLES.min(times.len()) as f64;
        let height = height as usize;
        // Leave room for motion between the samples, on even rows.
        let band = match rows {
            Some((top, bottom)) => {
                let margin = MARGIN.max((bottom - top) / 4);
                let top = top.saturating_sub(margin) & !1;
                let bottom = (bottom + margin + 1).min(height) & !1;
                if (bottom - top) * 5 > height * 3 {
                    (0, height)
                } else {
                    (top, (bottom - top).max(2))
                }
            }
            None => (0, 2.min(height)),
        };
        Ok(Ok(Base { first, band, paint, session: Mutex::new(Some(session)) }))
    }

    /// Export the frames at `times` over `base` with `settings` for the whole
    /// output. Anything but [`Outcome::Done`] leaves nothing at `path`.
    pub(crate) fn export_over(
        &self,
        path: &Path,
        options: &crate::ExportOptions,
        settings: EncoderSettings,
        times: &[f64],
        base: &Base,
    ) -> Result<Outcome, RenderError> {
        let fps = self.document.fps.max(1e-6);
        let (width, _) = self.output_size();
        // Layers over a base video often paint in a few milliseconds, so the
        // pool holds the workers that keep ahead of the encoder, two at least.
        let enough = ((base.paint * PAINT_RATE).ceil() as usize).max(2);
        let workers = options.workers.unwrap_or_else(|| self.pool_for(times.len()).workers().min(enough)).max(1);
        let settings = EncoderSettings {
            width: width as usize,
            height: base.band.1,
            ..settings
        }
        .with_underlay(Some(Underlay {
            path: base.first.path.clone(),
            color: base.first.color,
            seek: frame_to_time(base.first.index, base.first.fps),
            offset: (0, base.band.0),
            frames: times.len(),
        }));
        let (full_tx, full_rx) = mpsc::channel::<Option<(usize, Frame)>>();
        let (empty_tx, empty_rx) = mpsc::channel::<Frame>();
        for _ in 0..workers * 2 {
            let _ = empty_tx.send(Frame { width: 0, height: 0, rgba: Vec::new() });
        }
        let failed = AtomicBool::new(false);
        // 0 while every frame fits the plan, else the outcome that ended it.
        let broken = AtomicU8::new(0);
        let (empty_rx, next) = (Mutex::new(empty_rx), AtomicUsize::new(0));
        std::thread::scope(|scope| {
            let (failed, broken) = (&failed, &broken);
            let progress = options.progress.clone();
            let writer = scope.spawn(move || -> anyhow::Result<()> {
                let silent = self.document.audio.tracks.is_empty();
                let mut encoder = if silent {
                    Encoder::video_only(path, settings)?
                } else {
                    Encoder::with_audio_input(path, settings)?
                };
                let written = (|| {
                    if !silent {
                        let mut audio = self.import()?;
                        let block = audio_samples_per_video_frame(audio.audio_mixer.sample_rate, fps, 1);
                        encoder.send_audio(times.iter().flat_map(|&time| audio.mix_audio(block, time)).collect())?;
                    }
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
                if written.is_err() || failed.load(Ordering::Acquire) {
                    encoder.abort();
                    return written;
                }
                encoder.finish()
            });
            let cancelled = || options.cancel.as_ref().is_some_and(|cancel| cancel.load(Ordering::Acquire));
            let (next, empty_rx) = (&next, &empty_rx);
            let painters: Vec<_> = (0..workers)
                .map(|_| {
                    let full_tx = full_tx.clone();
                    scope.spawn(move || -> Result<(), RenderError> {
                        let mut session = None;
                        let painted = (|| loop {
                            if failed.load(Ordering::Acquire) {
                                return Ok(());
                            }
                            if cancelled() {
                                return Err(RenderError::Cancelled);
                            }
                            let Ok(mut frame) = empty_rx.lock().unwrap().recv() else { return Ok(()) };
                            let index = next.fetch_add(1, Ordering::Relaxed);
                            let Some(&time) = times.get(index) else { return Ok(()) };
                            let session = match &mut session {
                                Some(session) => session,
                                None => {
                                    let sampled = base.session.lock().unwrap().take();
                                    session.insert(match sampled {
                                        Some(session) => session,
                                        None => self.session()?,
                                    })
                                }
                            };
                            let punch = session.render_overlay(time)?;
                            let (top, rows) = base.band;
                            let outcome = if !punch.is_some_and(|punch| base.expects(index, &punch)) {
                                Outcome::Base as u8
                            } else if session.painted_rows().is_some_and(|(first, last)| first < top || last > top + rows) {
                                Outcome::Band as u8
                            } else {
                                0
                            };
                            if outcome != 0 {
                                broken.store(outcome, Ordering::Release);
                                failed.store(true, Ordering::Release);
                                let _ = full_tx.send(None);
                                return Ok(());
                            }
                            session.read_rows(base.band, &mut frame)?;
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
            let painted = painters
                .into_iter()
                .map(|painter| painter.join().unwrap_or(Err(RenderError::Readback)))
                .fold(Ok(()), Result::and);
            if painted.is_err() {
                failed.store(true, Ordering::Release);
            }
            drop(full_tx);
            let written = writer.join().unwrap_or_else(|_| Err(anyhow::anyhow!("the encoder thread panicked")));
            painted?;
            match broken.load(Ordering::Acquire) {
                0 => written.map(|_| Outcome::Done).map_err(RenderError::Encode),
                outcome if outcome == Outcome::Band as u8 => Ok(Outcome::Band),
                _ => Ok(Outcome::Base),
            }
        })
    }
}
