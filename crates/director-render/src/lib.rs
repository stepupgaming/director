//! Paints frames of a [`SceneDocument`].
//!
//! A [`Renderer`] holds a shared document and imports it into one live
//! `Director` per [`FrameSession`]. Each session owns its surfaces and layout
//! engine, so [`Renderer::render_frames`] paints frames on a rayon pool with
//! one session per worker.

mod frame;

use director_core::scene_import::{import_scene, ImportError, ImportRegistry};
use director_core::systems::layout::LayoutEngine;
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
    #[error("the worker session failed to start: {0}")]
    Session(String),
}

/// One painted frame in straight-alpha RGBA, row-major, `width * 4` bytes per row.
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

    /// Paint frames at `times` in parallel, one session per worker thread.
    /// Results keep the order of `times`.
    pub fn render_frames(&self, times: &[f64]) -> Vec<Result<Frame, RenderError>> {
        times
            .par_iter()
            .map_init(
                || self.session(),
                |session, &time| match session {
                    Ok(session) => session.render(time),
                    Err(err) => Err(RenderError::Session(err.to_string())),
                },
            )
            .collect()
    }
}

/// An imported `Director` with its own surfaces, reused across frames.
pub struct FrameSession {
    director: Director,
    layout: LayoutEngine,
    info: ImageInfo,
    surface: Surface,
    scratch: frame::Scratch,
}

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
            layout: LayoutEngine::new(),
            info,
            surface,
            scratch: frame::Scratch::default(),
        })
    }

    pub fn director(&self) -> &Director {
        &self.director
    }

    /// Paint the frame at `time` seconds.
    pub fn render(&mut self, time: f64) -> Result<Frame, RenderError> {
        frame::render_sampled(&mut self.director, &mut self.layout, time, &mut self.surface, &mut self.scratch)?;
        let width = self.info.width() as usize;
        let height = self.info.height() as usize;
        let mut rgba = vec![0u8; width * height * 4];
        // Opaque frames hold the same bytes either way; skip the unpremultiply pass.
        let read_info = if self.director.transparent_background {
            self.info.clone().with_alpha_type(AlphaType::Unpremul)
        } else {
            self.info.clone()
        };
        if !self.surface.read_pixels(&read_info, &mut rgba, width * 4, (0, 0)) {
            return Err(RenderError::Readback);
        }
        Ok(Frame {
            width: width as u32,
            height: height as u32,
            rgba,
        })
    }
}
