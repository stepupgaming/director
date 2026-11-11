//! # Director Engine
//!
//! `director-engine` is a high-performance, frame-based 2D rendering engine designed for programmatic video generation.
//!
// TODO: Set up Criterion.rs benchmarks for rendering and layout.
//!
//! It combines a Scene Graph architecture, a flexbox-based Layout Engine ([Taffy](https://crates.io/crates/taffy)),
//! and a high-quality Rasterizer ([Skia](https://skia.org/)) to render visual content.
//! It also includes an embedded scripting layer using [Rhai](https://rhai.rs/).
//!
//! ## Core Features
//!
//! *   **Frame-Based Rendering**: Designed for non-realtime video rendering where every frame matters.
//! *   **Scene Graph**: Hierarchical node structure with parent-child relationships.
//! *   **Layout**: Full implementation of Flexbox and CSS Grid layout via Taffy.
//! *   **Scripting**: Built-in bindings for Rhai to drive the engine from scripts.
//! *   **Video I/O**: FFmpeg encoder and decoder wrappers. `director-render` paints and exports frames.
//! *   **Rich Text**: Advanced typography support using `skia_safe::textlayout`.
//! *   **Animation**: Physics-based springs and keyframe animations.
//!
//! ## Usage
//!
//! The core entry point is the [`Director`] struct, which manages the timeline, scenes, and rendering resources.
//!
//! ```rust,no_run
//! use director_core::{Director, DefaultAssetLoader, video_wrapper::RenderMode};
//! use std::sync::Arc;
//!
//! // Initialize the Director
//! let mut director = Director::new(
//!     1920,
//!     1080,
//!     30.0,
//!     Arc::new(DefaultAssetLoader),
//!     RenderMode::Preview,
//!     None
//! );
//! ```

/// The Scene Graph Data Structure.
pub mod scene;
pub mod scene_export;
pub mod scene_import;

/// Defines the base `Element` trait that all visual nodes must implement.
pub mod element;

/// Contains the concrete implementations of visual nodes (Box, Text, Image, etc.).
pub mod node;

/// The core coordinator of the engine, managing the scene graph and timeline.
pub mod director;

/// Animation primitives, including `Animated<T>` and easing functions.
pub mod animation;

/// Rhai scripting API bindings.
pub mod scripting;

/// Video encoding and wrapping utilities.
pub mod video_wrapper;

/// Process-wide work counters for waste reports.
pub mod counters;

/// Passthrough of a base video that FFmpeg composites under the painted layers.
pub mod passthrough;

/// Audio mixing and processing.
pub mod audio;

/// MP4 and QuickTime edit lists that place a sound track on the file timeline.
pub mod mp4_edit;

/// Audio effects (reverb, delay, EQ, compressor).
pub mod audio_effects;

/// Beat grids: tempo, beats, bars and the drop of a music track.
pub mod beats;

/// Timeline track and clip types.
pub mod timeline;

/// Design system tokens (spacing, colors, typography).
pub mod tokens;

/// Colour grades: `.cube` 3D LUTs and the named presets as one colour filter.
pub mod grade;

/// Shared data structures used across the engine.
pub mod types;

/// Text direction detection and resolution for SkParagraph layout.
pub mod text_direction;

pub mod errors;
/// Core systems (Asset Management, etc.).
pub mod systems;

/// Canonical API catalog (effects, transitions, nodes) shared with schema generation.
pub mod api_catalog;

/// Capability registry for live reflection of nodes, effects, and transitions.
pub mod registry;

/// Versioned project (de)serialization (save/load a Director to JSON).
pub mod project;

/// Layout-time render manifest for agent introspection.
pub mod render_manifest;

/// Persistent semantic entity registry (faces, speakers, objects, caption phrases).
pub mod entity;

/// Semantic manifest and temporal constraint solver for smart nodes.
pub mod semantic;

/// Compile-phase dependency graph and incremental invalidation.
pub mod compiler;

/// Production style policy schema and scoring for the compiler.
pub mod style_policy;

/// True IT-R BS.1770 loudness analysis (LUFS, true peak, loudness range).
pub mod loudness;

/// Speech to text hook that word-timed captions from media run through.
pub mod transcribe;

pub use director::Director;
pub use render_manifest::{
    build_render_manifest, render_manifest_json, RenderManifest, RENDER_MANIFEST_VERSION,
};
pub use element::Element;
pub use errors::RenderError;

use anyhow::Result;
use tracing::instrument;

/// A trait for abstracting file system access.
///
/// This allows the engine to be embedded in environments where direct file system access
/// might be restricted or virtualized (e.g., loading assets from a network or an archive).
pub trait AssetLoader: Send + Sync {
    /// Loads the raw bytes of an asset from the given path.
    ///
    /// # Arguments
    /// * `path` - The string path or identifier for the asset.
    ///
    /// # Returns
    /// * `Result<Vec<u8>>` - The bytes of the file or an error.
    fn load_bytes(&self, path: &str) -> Result<Vec<u8>>;

    /// A local file holding asset `path`, for decoders that read from disk such
    /// as video. The default returns `path` itself when it names a file.
    fn media_path(&self, path: &str) -> Option<std::path::PathBuf> {
        let file = std::path::Path::new(path);
        file.is_file().then(|| file.to_path_buf())
    }

    /// Optionally loads a fallback font (e.g., Emoji) to be used when glyphs are missing.
    ///
    /// The default implementation returns `None`.
    fn load_font_fallback(&self) -> Option<Vec<u8>> {
        None
    }
}

/// A font Director ships inside the library, by its `fonts/<file>` path, so
/// text renders the same wherever the binary runs.
pub fn bundled_asset(path: &str) -> Option<&'static [u8]> {
    macro_rules! fonts {
        ($($file:literal),* $(,)?) => {
            match path {
                $(concat!("fonts/", $file) => Some(include_bytes!(concat!("../assets/fonts/", $file)).as_slice()),)*
                _ => None,
            }
        };
    }
    fonts!(
        "Roboto-Regular.ttf",
        "Roboto-Bold.ttf",
        "Roboto-Italic.ttf",
        "Inter-Regular.ttf",
        "Inter-Bold.ttf",
        "Poppins-Bold.ttf",
        "Archivo-Variable.ttf",
        "JetBrainsMono-Variable.ttf",
    )
}

/// The default implementation of `AssetLoader` using the standard `std::fs` filesystem.
pub struct DefaultAssetLoader;

impl AssetLoader for DefaultAssetLoader {
    /// Loads bytes directly from the local filesystem.
    #[instrument(level = "debug", skip(self), fields(path = path))]
    fn load_bytes(&self, path: &str) -> Result<Vec<u8>> {
        if let Ok(bytes) = std::fs::read(path) {
            return Ok(bytes);
        }
        if let Ok(bytes) = std::fs::read(format!("assets/{}", path)) {
            return Ok(bytes);
        }
        if let Some(bytes) = bundled_asset(path) {
            return Ok(bytes.to_vec());
        }
        anyhow::bail!("Asset not found: {} (checked '{}' and 'assets/{}'): file not found", path, path, path)
    }

    /// Attempts to load an emoji font.
    ///
    /// It first checks the `DIRECTOR_EMOJI_FONT` environment variable.
    /// If not set, it checks for `assets/fonts/emoji.ttf` in the current directory.
    fn load_font_fallback(&self) -> Option<Vec<u8>> {
        if let Ok(path) = std::env::var("DIRECTOR_EMOJI_FONT") {
            if let Ok(bytes) = std::fs::read(path) {
                return Some(bytes);
            }
        }
        // Default fallback path
        if let Ok(bytes) = std::fs::read("assets/fonts/emoji.ttf") {
            return Some(bytes);
        }
        None
    }
}
