//! # Inspection API
//!
//! Rhai bindings that expose engine capabilities and live scene-graph
//! introspection so agents (and humans) can harvest bounds, durations,
//! supported node types, and effects without rendering.

use crate::registry;
use crate::scripting::types::{MovieHandle, NodeHandle, SceneHandle};
use rhai::{Array, Engine, Map};

/// Register inspection/introspection functions with the Rhai engine.
pub fn register(engine: &mut Engine) {
    // ---- Live scene-graph introspection ----

    engine.register_fn("node_id", |n: &mut NodeHandle| -> i64 {
        n.id as i64
    });

    engine.register_fn("node_bounds", |n: &mut NodeHandle| -> Map {
        let mut m = Map::new();
        if let Ok(d) = n.lock_director() {
            if let Some(node) = d.scene.get_node(n.id) {
                let r = node.layout_rect;
                m.insert("x".into(), (r.left() as f64).into());
                m.insert("y".into(), (r.top() as f64).into());
                m.insert("w".into(), (r.width() as f64).into());
                m.insert("h".into(), (r.height() as f64).into());
            }
        }
        m
    });

    engine.register_fn("node_children", |n: &mut NodeHandle| -> Array {
        let mut arr = Array::new();
        if let Ok(d) = n.lock_director() {
            if let Some(node) = d.scene.get_node(n.id) {
                for c in &node.children {
                    arr.push((*c as i64).into());
                }
            }
        }
        arr
    });

    engine.register_fn("scene_duration", |s: &mut SceneHandle| -> f64 {
        s.duration
    });

    engine.register_fn("list_scene_roots", |m: &mut MovieHandle| -> Array {
        let mut arr = Array::new();
        if let Ok(d) = m.lock_director() {
            for item in &d.timeline {
                arr.push((item.scene_root as i64).into());
            }
            for track in &d.tracks {
                for clip in &track.clips {
                    arr.push((clip.scene_root as i64).into());
                }
            }
        }
        arr
    });

    // ---- Capability registry reflection (JSON for bulk lists) ----

    engine.register_fn("list_node_types", || -> String {
        serde_json::to_string(&registry::list_node_types()).unwrap_or_default()
    });

    engine.register_fn("list_effects", || -> String {
        serde_json::to_string(&registry::list_effects()).unwrap_or_default()
    });

    engine.register_fn("list_transitions", || -> String {
        serde_json::to_string(&registry::list_transitions()).unwrap_or_default()
    });

    engine.register_fn("list_easings", || -> String {
        serde_json::to_string(&registry::list_easings()).unwrap_or_default()
    });

    engine.register_fn("get_effect_meta", |name: &str| -> String {
        match registry::get_effect(name) {
            Some(info) => serde_json::to_string(&info).unwrap_or_default(),
            None => String::new(),
        }
    });

    engine.register_fn("supports_animation", |node_type: &str, prop: &str| -> bool {
        registry::supports_animation(node_type, prop)
    });

    engine.register_fn("get_animatable_properties", |node_type: &str| -> Array {
        let mut arr = Array::new();
        for p in registry::get_animatable_properties(node_type) {
            arr.push((*p).into());
        }
        arr
    });
}
