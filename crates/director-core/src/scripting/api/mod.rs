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
//! - **compositions**: Named compositions + typed props registry

pub mod animation;
pub mod audio;
pub mod compiler;
pub mod compositions;
pub mod effects;
pub mod entities;
pub mod http;
pub mod inspect;
pub mod lifecycle;
pub mod manifest;
pub mod nodes;
pub mod properties;
pub mod smart_caption;
pub mod style_policy;
pub mod templates;

use crate::video_wrapper::RenderMode;
use crate::AssetLoader;
use rhai::Engine;
use std::sync::Arc;

/// Register all API functions with a specific default render mode for
/// `new_director(width, height, fps)`.
///
/// Does **not** register named compositions — hosts that need
/// `register_composition` must also call [`compositions::register`] with a
/// shared [`compositions::CompositionRegistry`].
pub fn register_all_with_default_mode(
    engine: &mut Engine,
    loader: Arc<dyn AssetLoader>,
    default_mode: RenderMode,
) {
    lifecycle::register(engine, loader.clone(), default_mode);
    nodes::register(engine, loader.clone());
    animation::register(engine);
    audio::register(engine);
    compiler::register(engine);
    effects::register(engine);
    entities::register(engine);
    http::register(engine);
    properties::register(engine);
    smart_caption::register(engine);
    templates::register(engine);
    inspect::register(engine);
    manifest::register(engine);
    style_policy::register(engine);
}
