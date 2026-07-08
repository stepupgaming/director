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

/// Default maximum expression nesting depth for scripts. Bounded to keep
/// autonomous agent loops from hanging or exhausting memory, while still
/// allowing realistically nested motion-graphics logic.
pub const MAX_EXPR_DEPTH: usize = 128;
/// Default maximum total expression depth (call stack) for scripts.
pub const MAX_EXPR_DEPTHS: usize = 128;

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
    // Bound Rhai expression recursion so an autonomous agent loop cannot hang or
    // OOM the render process. These are generous for hand-written scripts but
    // prevent runaway `while`/`for`/recursion. Override via `set_script_limits`.
    engine.set_max_expr_depths(MAX_EXPR_DEPTH, MAX_EXPR_DEPTHS);

    // Register all API functions
    api::register_all_with_default_mode(engine, loader, default_mode);
}

/// Override the Rhai expression-depth limits at runtime (e.g. from a host
/// config). `depth` bounds nesting; `depths` bounds total call-stack depth.
/// Values of `0` are treated as unlimited for that axis.
pub fn set_script_limits(engine: &mut Engine, depth: usize, depths: usize) {
    engine.set_max_expr_depths(depth, depths);
}
