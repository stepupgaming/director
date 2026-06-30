//! # Scripting Module
//!
//! Rhai scripting API bindings for Director Engine.
//!
//! ## Responsibilities
//! - **Engine Setup**: Registers all types and functions with Rhai.
//! - **Node Creation**: `create_box`, `create_text`, `create_image`, etc.
//! - **Animation**: `animate_*` property setters.
//! - **Director Control**: `set_duration`, `add_scene`, timeline manipulation.
//!
//! ## Pattern
//! All bindings follow: `engine.register_fn("name", |ctx, ...| { ... })`
//!
//! ## Module Structure
//! - `types`: Handle types (MovieHandle, SceneHandle, NodeHandle, AudioTrackHandle)
//! - `utils`: Parsing helpers (colors, layout, text, easing)
//! - `theme`: Design system token API
//! - `api/`: Sub-modules for lifecycle, nodes, animation, audio, effects, properties

mod api;
mod theme;
pub mod types;
pub mod utils;

pub use theme::create_theme_api;
pub use types::{AudioTrackHandle, MovieHandle, NodeHandle, SceneHandle};

use crate::tokens::DesignSystem;
use crate::video_wrapper::RenderMode;
use crate::AssetLoader;
use rhai::Engine;
use std::sync::Arc;

/// Registers the Director Engine API into the provided Rhai `Engine`.
///
/// This exposes `Movie`, `Scene`, `Node`, `AudioTrack` types and their methods.
pub fn register_rhai_api(engine: &mut Engine, loader: Arc<dyn AssetLoader>) {
    register_rhai_api_with_default_mode(engine, loader, RenderMode::Preview);
}

/// Registers the Director Engine API with a caller-selected default render mode.
///
/// This lets the preview server keep fast preview semantics while the export CLI
/// can make `new_director(width, height, fps)` create export-ready video nodes.
pub fn register_rhai_api_with_default_mode(
    engine: &mut Engine,
    loader: Arc<dyn AssetLoader>,
    default_mode: RenderMode,
) {
    // Register theme module
    let theme_module = create_theme_api(DesignSystem::new());
    engine.register_static_module("theme", theme_module.into());
    engine.set_max_expr_depths(0, 0);

    // Register all API functions
    api::register_all_with_default_mode(engine, loader, default_mode);
}
