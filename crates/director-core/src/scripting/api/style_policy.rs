//! # Style Policy API
//!
//! Rhai bindings for creating production `StylePolicy`s and scoring a
//! `Director` scene against them from scripts.
//!
//! ## Responsibilities
//! - `style_policy(config)` — build a `StylePolicy` from a Rhai map.
//! - `movie.set_style_policy(policy)` — attach a policy to a Director.
//! - `movie.score_style_policy(start, end, samples)` — evaluate the scene and
//!   return a score map.
//! - `movie.style_policy_score()` — retrieve the score produced by the last
//!   `compile()` or `score_style_policy()` call.
//!
//! ## Policy config map keys (for `style_policy(#{ ... })`)
//! - `typography`: map with `max_text_area_ratio`, `max_lines_per_caption`,
//!   `min_font_size`, `max_font_size`, `safe_zone_margin`, `weight`.
//! - `motion`: map with `max_pixels_per_second`, `max_rotation_velocity`,
//!   `max_scale_velocity`, `avg_motion_displacement_threshold`, `weight`.
//! - `transitions`: map with `min_seconds_between_transitions`,
//!   `max_transitions_per_minute`, `min_transition_duration`,
//!   `max_transition_duration`, `weight`.
//! - `audio`: map with `target_loudness_db`, `loudness_tolerance_db`,
//!   `max_true_peak_db`, `min_loudness_range`, `weight`.

use crate::scripting::types::MovieHandle;
use crate::style_policy::{
    AudioPolicy, MotionPolicy, StylePolicy, TransitionPolicy, TypographyPolicy,
};
use rhai::{Engine, Map};

/// Register style-policy-related Rhai functions.
pub fn register(engine: &mut Engine) {
    engine.register_fn(
        "style_policy",
        |config: Map| -> Result<StylePolicy, Box<rhai::EvalAltResult>> {
            parse_style_policy(&config)
        },
    );

    engine.register_fn(
        "set_style_policy",
        |movie: &mut MovieHandle, policy: StylePolicy| -> Result<(), Box<rhai::EvalAltResult>> {
            let mut d = movie.lock_director()?;
            d.style_policy = policy;
            Ok(())
        },
    );

    engine.register_fn(
        "score_style_policy",
        |movie: &mut MovieHandle,
         start: f64,
         end: f64,
         samples: i64|
         -> Result<Map, Box<rhai::EvalAltResult>> {
            let mut d = movie.lock_director()?;
            let score = d.score_style_policy((start, end), samples.max(1) as usize);
            d.last_policy_score = Some(score.clone());
            Ok(policy_score_to_map(score))
        },
    );

    engine.register_fn(
        "style_policy_score",
        |movie: &mut MovieHandle| -> Result<Map, Box<rhai::EvalAltResult>> {
            let d = movie.lock_director()?;
            let score = d
                .last_policy_score
                .clone()
                .ok_or("no style policy score available; run compile() first")?;
            Ok(policy_score_to_map(score))
        },
    );
}

fn parse_style_policy(config: &Map) -> Result<StylePolicy, Box<rhai::EvalAltResult>> {
    let mut policy = StylePolicy::default();

    if let Some(typography) = config.get("typography").and_then(|v| v.clone().try_cast::<Map>()) {
        policy.typography = parse_typography_policy(&typography)?;
    }

    if let Some(motion) = config.get("motion").and_then(|v| v.clone().try_cast::<Map>()) {
        policy.motion = parse_motion_policy(&motion)?;
    }

    if let Some(transitions) = config.get("transitions").and_then(|v| v.clone().try_cast::<Map>()) {
        policy.transitions = parse_transition_policy(&transitions)?;
    }

    if let Some(audio) = config.get("audio").and_then(|v| v.clone().try_cast::<Map>()) {
        policy.audio = parse_audio_policy(&audio)?;
    }

    Ok(policy)
}

fn parse_typography_policy(map: &Map) -> Result<TypographyPolicy, Box<rhai::EvalAltResult>> {
    let mut policy = TypographyPolicy::default();
    if let Some(v) = map.get("max_text_area_ratio").and_then(|v| v.as_float().ok()) {
        policy.max_text_area_ratio = v;
    }
    if let Some(v) = map.get("max_lines_per_caption").and_then(|v| v.as_int().ok()) {
        policy.max_lines_per_caption = v as u32;
    }
    if let Some(v) = map.get("min_font_size").and_then(|v| v.as_float().ok()) {
        policy.min_font_size = v as f32;
    }
    if let Some(v) = map.get("max_font_size").and_then(|v| v.as_float().ok()) {
        policy.max_font_size = v as f32;
    }
    if let Some(v) = map.get("safe_zone_margin").and_then(|v| v.as_float().ok()) {
        policy.safe_zone_margin = v as f32;
    }
    if let Some(v) = map.get("weight").and_then(|v| v.as_float().ok()) {
        policy.weight = v;
    }
    Ok(policy)
}

fn parse_motion_policy(map: &Map) -> Result<MotionPolicy, Box<rhai::EvalAltResult>> {
    let mut policy = MotionPolicy::default();
    if let Some(v) = map.get("max_pixels_per_second").and_then(|v| v.as_float().ok()) {
        policy.max_pixels_per_second = v as f32;
    }
    if let Some(v) = map.get("max_rotation_velocity").and_then(|v| v.as_float().ok()) {
        policy.max_rotation_velocity = v as f32;
    }
    if let Some(v) = map.get("max_scale_velocity").and_then(|v| v.as_float().ok()) {
        policy.max_scale_velocity = v as f32;
    }
    if let Some(v) = map.get("avg_motion_displacement_threshold").and_then(|v| v.as_float().ok()) {
        policy.avg_motion_displacement_threshold = v as f32;
    }
    if let Some(v) = map.get("weight").and_then(|v| v.as_float().ok()) {
        policy.weight = v;
    }
    Ok(policy)
}

fn parse_transition_policy(map: &Map) -> Result<TransitionPolicy, Box<rhai::EvalAltResult>> {
    let mut policy = TransitionPolicy::default();
    if let Some(v) = map.get("min_seconds_between_transitions").and_then(|v| v.as_float().ok()) {
        policy.min_seconds_between_transitions = v;
    }
    if let Some(v) = map.get("max_transitions_per_minute").and_then(|v| v.as_float().ok()) {
        policy.max_transitions_per_minute = v;
    }
    if let Some(v) = map.get("min_transition_duration").and_then(|v| v.as_float().ok()) {
        policy.min_transition_duration = v;
    }
    if let Some(v) = map.get("max_transition_duration").and_then(|v| v.as_float().ok()) {
        policy.max_transition_duration = v;
    }
    if let Some(v) = map.get("weight").and_then(|v| v.as_float().ok()) {
        policy.weight = v;
    }
    Ok(policy)
}

fn parse_audio_policy(map: &Map) -> Result<AudioPolicy, Box<rhai::EvalAltResult>> {
    let mut policy = AudioPolicy::default();
    if let Some(v) = map.get("target_loudness_db").and_then(|v| v.as_float().ok()) {
        policy.target_loudness_db = v as f32;
    }
    if let Some(v) = map.get("loudness_tolerance_db").and_then(|v| v.as_float().ok()) {
        policy.loudness_tolerance_db = v as f32;
    }
    if let Some(v) = map.get("max_true_peak_db").and_then(|v| v.as_float().ok()) {
        policy.max_true_peak_db = v as f32;
    }
    if let Some(v) = map.get("min_loudness_range").and_then(|v| v.as_float().ok()) {
        policy.min_loudness_range = v as f32;
    }
    if let Some(v) = map.get("weight").and_then(|v| v.as_float().ok()) {
        policy.weight = v;
    }
    Ok(policy)
}

fn policy_score_to_map(score: crate::style_policy::PolicyScore) -> Map {
    let mut map = Map::new();
    map.insert("total_score".into(), score.total_score.into());

    let dimension_scores: rhai::Array = score
        .dimension_scores
        .into_iter()
        .map(|(name, value)| {
            let mut m = Map::new();
            m.insert("name".into(), name.into());
            m.insert("score".into(), value.into());
            rhai::Dynamic::from_map(m)
        })
        .collect();
    map.insert("dimension_scores".into(), dimension_scores.into());

    let violations: rhai::Array = score
        .violations
        .into_iter()
        .map(|v| {
            let mut m = Map::new();
            m.insert("rule".into(), v.rule.into());
            m.insert("severity".into(), v.severity.as_str().into());
            if let Some(id) = v.node_id {
                m.insert("node_id".into(), (id as i64).into());
            }
            if let Some(time) = v.time {
                m.insert("time".into(), time.into());
            }
            m.insert("description".into(), v.description.into());
            rhai::Dynamic::from_map(m)
        })
        .collect();
    map.insert("violations".into(), violations.into());

    map
}

