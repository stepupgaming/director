//! # API Module
//!
//! Aggregates all Rhai API sub-modules and provides a single registration point.
//!
//! ## Sub-modules
//! - **lifecycle**: Director creation, scene management, transitions
//! - **nodes**: Node creation (box, text, image, video, lottie, svg, composition)
//! - **animation**: Keyframe, spring, and path animations
//! - **audio**: Audio loading, analysis, and reactivity
//! - **effects**: Visual effects and shaders
//! - **properties**: Node property setters

pub mod animation;
pub mod audio;
pub mod effects;
pub mod http;
pub mod inspect;
pub mod lifecycle;
pub mod nodes;
pub mod properties;
pub mod templates;

use crate::video_wrapper::RenderMode;
use crate::AssetLoader;
use rhai::Engine;
use std::sync::Arc;

/// Register all API functions with a specific default render mode for
/// `new_director(width, height, fps)`.
pub fn register_all_with_default_mode(
    engine: &mut Engine,
    loader: Arc<dyn AssetLoader>,
    default_mode: RenderMode,
) {
    lifecycle::register(engine, loader.clone(), default_mode);
    nodes::register(engine, loader.clone());
    animation::register(engine);
    audio::register(engine);
    effects::register(engine);
    http::register(engine);
    properties::register(engine);
    templates::register(engine);
    inspect::register(engine);
}
