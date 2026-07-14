//! # Smart Caption API
//!
//! Rhai bindings for creating `SmartCaptionNode`s from a `SemanticManifest`
//! and the movie's entity registry.
//!
//! ## Responsibilities
//! - `create_semantic_manifest(movie, props)` — build a manifest from a Rhai map.
//! - `create_smart_caption_config(movie, props)` — build a solver config.
//! - Helper utilities for converting Rhai maps to solver configs.
//!
//! ## Manifest map keys
//! - `entity_refs`: array of entity UUID strings
//! - `start_time`: shot start in seconds
//! - `end_time`: shot end in seconds
//! - `clearance`, `smoothing_window`, `motion_penalty`, `overlap_penalty`,
//!   `safe_zone_penalty`, `grid_steps`, `negative_space_weight`,
//!   `smoothness_weight`: solver tuning
//!
//! ## Config map keys
//! - `canvas_w`, `canvas_h`: output dimensions
//! - `caption_w`, `caption_h`: expected caption block size
//! - `safe_top`, `safe_bottom`, `safe_left`, `safe_right`: safe-zone margins
//! - `sample_rate`: solver samples per second
//! - `continuous`: use continuous (x, y) solver instead of discrete anchors

use crate::node::smart_caption::SmartCaptionConfig;
use crate::scripting::types::MovieHandle;
use crate::semantic::{SemanticManifest, SolverConfig};
use rhai::{Engine, EvalAltResult};

/// Parse a Rhai map into a `SolverConfig`.
pub(crate) fn parse_solver_config(props: &rhai::Map) -> SolverConfig {
    let mut solver = SolverConfig::default();
    if let Some(v) = props.get("clearance").and_then(|v| v.as_float().ok()) {
        solver.clearance = v as f32;
    }
    if let Some(v) = props.get("smoothing_window").and_then(|v| v.as_int().ok()) {
        solver.smoothing_window = v as usize;
    }
    if let Some(v) = props.get("motion_penalty").and_then(|v| v.as_float().ok()) {
        solver.motion_penalty = v as f32;
    }
    if let Some(v) = props.get("overlap_penalty").and_then(|v| v.as_float().ok()) {
        solver.overlap_penalty = v as f32;
    }
    if let Some(v) = props.get("safe_zone_penalty").and_then(|v| v.as_float().ok()) {
        solver.safe_zone_penalty = v as f32;
    }
    if let Some(v) = props.get("grid_steps").and_then(|v| v.as_int().ok()) {
        solver.grid_steps = v as usize;
    }
    if let Some(v) = props.get("negative_space_weight").and_then(|v| v.as_float().ok()) {
        solver.negative_space_weight = v as f32;
    }
    if let Some(v) = props.get("smoothness_weight").and_then(|v| v.as_float().ok()) {
        solver.smoothness_weight = v as f32;
    }
    solver
}

/// Parse a Rhai map into a `SmartCaptionConfig`.
pub(crate) fn parse_smart_caption_config(props: &rhai::Map) -> SmartCaptionConfig {
    let mut config = SmartCaptionConfig::default();
    if let Some(v) = props.get("canvas_w").and_then(|v| v.as_float().ok()) {
        config.canvas_w = v as f32;
    }
    if let Some(v) = props.get("canvas_h").and_then(|v| v.as_float().ok()) {
        config.canvas_h = v as f32;
    }
    if let Some(v) = props.get("caption_w").and_then(|v| v.as_float().ok()) {
        config.caption_w = v as f32;
    }
    if let Some(v) = props.get("caption_h").and_then(|v| v.as_float().ok()) {
        config.caption_h = v as f32;
    }
    if let Some(v) = props.get("safe_top").and_then(|v| v.as_float().ok()) {
        config.safe_top = v as f32;
    }
    if let Some(v) = props.get("safe_bottom").and_then(|v| v.as_float().ok()) {
        config.safe_bottom = v as f32;
    }
    if let Some(v) = props.get("safe_left").and_then(|v| v.as_float().ok()) {
        config.safe_left = v as f32;
    }
    if let Some(v) = props.get("safe_right").and_then(|v| v.as_float().ok()) {
        config.safe_right = v as f32;
    }
    if let Some(v) = props.get("sample_rate").and_then(|v| v.as_float().ok()) {
        config.sample_rate = v;
    }
    if let Some(v) = props.get("continuous").and_then(|v| v.as_bool().ok()) {
        config.continuous = v;
    }
    config
}

/// Register smart-caption-related Rhai functions.
pub fn register(engine: &mut Engine) {
    engine.register_fn(
        "create_semantic_manifest",
        |movie: &mut MovieHandle,
         props: rhai::Map|
         -> Result<SemanticManifest, Box<EvalAltResult>> {
            let mut entity_refs = Vec::new();
            if let Some(arr) = props.get("entity_refs").and_then(|v| v.clone().into_array().ok()) {
                for item in arr {
                    if let Ok(s) = item.clone().into_string() {
                        let id = uuid::Uuid::parse_str(&s)
                            .map_err(|e| format!("invalid entity UUID: {}", e))?;
                        entity_refs.push(id);
                    }
                }
            }

            let start_time = props
                .get("start_time")
                .and_then(|v| v.as_float().ok())
                .unwrap_or(0.0);
            let end_time = props
                .get("end_time")
                .and_then(|v| v.as_float().ok())
                .unwrap_or(start_time + 1.0);

            let solver = parse_solver_config(&props);

            Ok(SemanticManifest {
                entity_refs,
                start_time,
                end_time,
                solver,
            })
        },
    );

    engine.register_fn(
        "create_smart_caption_config",
        |_movie: &mut MovieHandle, props: rhai::Map| -> SmartCaptionConfig {
            parse_smart_caption_config(&props)
        },
    );
}
