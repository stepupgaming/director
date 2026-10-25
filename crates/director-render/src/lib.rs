//! Paints frames of a live `Director` or a [`SceneDocument`].
//!
//! [`FrameState::render`] paints one frame of a live `Director` into any
//! surface, and [`render_export`] exports a live `Director` serially, on a
//! Vulkan target when given a [`GpuContext`].
//!
//! A [`Renderer`] holds a shared document and imports it into one live
//! `Director` per [`FrameSession`]. Each session owns its surface and frame
//! state. A [`FramePool`] keeps warm sessions on the rayon pool and paints
//! each worker's contiguous run of frames into reused buffers.
//! [`Renderer::export`] pipes pool batches to FFmpeg on a writer thread while
//! the next batch paints.

mod export;
mod frame;
mod live;
mod paint;

pub use export::{export_director, frame_times};
pub use frame::{paint_root, render_frame, FrameState};
pub use live::{output_dimensions, render_export, render_export_at_scale, set_output_scale, validate_scale, GpuContext};

use director_core::scene_import::{import_scene, ImportError, ImportRegistry};
use director_core::video_wrapper::RenderMode;
use director_core::{AssetLoader, Director};
use director_scene::SceneDocument;
use rayon::prelude::*;
use skia_safe::{AlphaType, ColorSpace, ColorType, ImageInfo, Surface};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error(transparent)]
    Import(#[from] ImportError),
    #[error("render failed: {0}")]
    Render(#[from] director_core::RenderError),
    #[error("the document has no positive output size")]
    EmptyOutput,
    #[error("failed to allocate a {0}x{1} surface")]
    Surface(i32, i32),
    #[error("failed to read frame pixels")]
    Readback,
    #[error("encoding failed: {0}")]
    Encode(anyhow::Error),
}

/// Frames a session paints before another session pays its import and cold
/// first frame. `Renderer::pool_for` sizes pools by it.
pub const FRAMES_PER_SESSION: usize = 15;

/// One painted frame in RGBA, row-major, `width * 4` bytes per row. Alpha is
/// straight; opaque frames carry the same bytes premultiplied.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Paints frames of one document.
#[derive(Clone)]
pub struct Renderer {
    document: Arc<SceneDocument>,
    loader: Arc<dyn AssetLoader>,
    registry: ImportRegistry,
    render_mode: RenderMode,
}

impl Renderer {
    /// Render with the core registry in export mode.
    pub fn new(document: Arc<SceneDocument>, loader: Arc<dyn AssetLoader>) -> Self {
        Self {
            document,
            loader,
            registry: ImportRegistry::core(),
            render_mode: RenderMode::Export,
        }
    }

    /// Decode node kinds through `registry`, for frontends that add their own.
    pub fn with_registry(mut self, registry: ImportRegistry) -> Self {
        self.registry = registry;
        self
    }

    pub fn with_render_mode(mut self, render_mode: RenderMode) -> Self {
        self.render_mode = render_mode;
        self
    }

    pub fn document(&self) -> &SceneDocument {
        &self.document
    }

    /// Output size after the document's output scale.
    pub fn output_size(&self) -> (i32, i32) {
        let document = &self.document;
        (
            (document.width as f32 * document.output_scale_x).round() as i32,
            (document.height as f32 * document.output_scale_y).round() as i32,
        )
    }

    /// Import the document into a fresh session.
    pub fn session(&self) -> Result<FrameSession, RenderError> {
        let director = import_scene(&self.document, self.loader.clone(), &self.registry, self.render_mode)?;
        FrameSession::new(director, self.output_size())
    }

    /// Paint one frame at `time` seconds.
    pub fn render_frame(&self, time: f64) -> Result<Frame, RenderError> {
        self.session()?.render(time)
    }

    /// A pool of warm sessions, one per rayon worker.
    pub fn pool(&self) -> FramePool {
        self.pool_with(rayon::current_num_threads())
    }

    /// A pool of `workers` warm sessions, at least one.
    pub fn pool_with(&self, workers: usize) -> FramePool {
        FramePool {
            renderer: self.clone(),
            sessions: (0..workers.max(1)).map(|_| None).collect(),
        }
    }

    /// A pool for `frames` frames: one session per [`FRAMES_PER_SESSION`]
    /// frames, up to half the rayon workers so the encoder keeps cores.
    pub fn pool_for(&self, frames: usize) -> FramePool {
        let cap = (rayon::current_num_threads() / 2).max(1);
        self.pool_with(frames.div_ceil(FRAMES_PER_SESSION).min(cap))
    }

    /// Paint frames at `times` in parallel through a fresh [`FramePool`].
    /// Frames keep the order of `times`.
    pub fn render_frames(&self, times: &[f64]) -> Result<Vec<Frame>, RenderError> {
        let mut frames = Vec::new();
        self.pool().render_into(times, &mut frames)?;
        Ok(frames)
    }
}

/// One warm [`FrameSession`] per rayon worker, kept across calls so each
/// session holds its static layer cache and incremental frame state.
pub struct FramePool {
    renderer: Renderer,
    sessions: Vec<Option<FrameSession>>,
}

impl FramePool {
    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    pub fn workers(&self) -> usize {
        self.sessions.len()
    }

    /// Paint `times` into `frames`, resized to match and in the same order.
    /// Each worker paints one contiguous run of `times` into the existing
    /// pixel buffers. Sessions import on first use.
    pub fn render_into(&mut self, times: &[f64], frames: &mut Vec<Frame>) -> Result<(), RenderError> {
        let (width, height) = self.renderer.output_size();
        frames.resize_with(times.len(), || Frame {
            width: width as u32,
            height: height as u32,
            rgba: Vec::new(),
        });
        if times.is_empty() {
            return Ok(());
        }
        let run = times.len().div_ceil(self.sessions.len());
        let renderer = &self.renderer;
        self.sessions
            .par_iter_mut()
            .zip(times.par_chunks(run).zip(frames.par_chunks_mut(run)))
            .try_for_each(|(slot, (times, frames))| {
                let session = match slot {
                    Some(session) => session,
                    None => slot.insert(renderer.session()?),
                };
                times.iter().zip(frames).try_for_each(|(&time, frame)| session.render_into(time, frame))
            })
    }
}

/// An imported `Director` with its own surfaces, reused across frames.
pub struct FrameSession {
    director: Director,
    info: ImageInfo,
    surface: Surface,
    state: FrameState,
}

// SAFETY: a session exclusively owns its `Director` (itself `Send`), frame
// state and CPU raster surfaces, none of which hold a thread-bound context. A
// `FramePool` hands each session to one worker at a time between calls.
unsafe impl Send for FrameSession {}

impl FrameSession {
    /// Wrap a live `Director` that paints into a `width` x `height` raster.
    pub fn new(director: Director, (width, height): (i32, i32)) -> Result<Self, RenderError> {
        if width <= 0 || height <= 0 {
            return Err(RenderError::EmptyOutput);
        }
        let info = ImageInfo::new(
            (width, height),
            ColorType::RGBA8888,
            AlphaType::Premul,
            Some(ColorSpace::new_srgb()),
        );
        let surface = skia_safe::surfaces::raster(&info, None, None).ok_or(RenderError::Surface(width, height))?;
        Ok(Self {
            director,
            info,
            surface,
            state: FrameState::default(),
        })
    }

    pub fn director(&self) -> &Director {
        &self.director
    }

    /// Paint the frame at `time` seconds.
    pub fn render(&mut self, time: f64) -> Result<Frame, RenderError> {
        let mut frame = Frame {
            width: 0,
            height: 0,
            rgba: Vec::new(),
        };
        self.render_into(time, &mut frame)?;
        Ok(frame)
    }

    /// Paint the frame at `time` seconds into `frame`, reusing its pixel buffer.
    pub fn render_into(&mut self, time: f64, frame: &mut Frame) -> Result<(), RenderError> {
        self.state.render(&mut self.director, time, &mut self.surface)?;
        let width = self.info.width() as usize;
        let height = self.info.height() as usize;
        frame.width = width as u32;
        frame.height = height as u32;
        frame.rgba.resize(width * height * 4, 0);
        // Opaque frames hold the same bytes either way; skip the unpremultiply pass.
        let read_info = if self.director.transparent_background {
            self.info.clone().with_alpha_type(AlphaType::Unpremul)
        } else {
            self.info.clone()
        };
        if !self.surface.read_pixels(&read_info, &mut frame.rgba, width * 4, (0, 0)) {
            return Err(RenderError::Readback);
        }
        Ok(())
    }
}
