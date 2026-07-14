//! # Entities API
//!
//! Rhai bindings for the semantic entity registry.
//!
//! ## Responsibilities
//! - **Entity Creation**: `create_entity(kind, label)` returns a UUID string.
//! - **Sample Insertion**: `add_entity_sample(id, time, x, y, w, h, confidence)`.
//! - **Sampling**: `entity_bbox_at(id, time)` returns a map with x/y/w/h/confidence.
//! - **Discovery**: `find_entities(kind)` returns UUID strings array.
//! - **Node Attachment**: `attach_entity(id)` on a node handle.
//!
//! UUIDs are passed as strings in Rhai for simplicity and agent interoperability.

use crate::entity::{BoundingBox, EntityKind};
use rhai::{Engine, EvalAltResult};
use std::str::FromStr;
use uuid::Uuid;

use super::super::types::{MovieHandle, NodeHandle};

/// Register entity-related Rhai functions.
pub fn register(engine: &mut Engine) {
    // ========== MOVIE-LEVEL ENTITY OPERATIONS ==========

    engine.register_fn(
        "create_entity",
        |movie: &mut MovieHandle, kind: &str, label: &str| -> Result<String, Box<EvalAltResult>> {
            let mut d = movie.lock_director()?;
            let kind = EntityKind::from_str(kind).map_err(|e| e.to_string())?;
            let id = d.entities.create(kind, label.to_string());
            Ok(id.to_string())
        },
    );

    engine.register_fn(
        "add_entity_sample",
        |movie: &mut MovieHandle,
         id: &str,
         time: f64,
         x: f64,
         y: f64,
         w: f64,
         h: f64,
         confidence: f64|
         -> Result<bool, Box<EvalAltResult>> {
            let id = parse_entity_id(id)?;
            let mut d = movie.lock_director()?;
            if let Some(track) = d.entities.get_mut(id) {
                track.add_sample(
                    time,
                    BoundingBox::new(x as f32, y as f32, w as f32, h as f32),
                    confidence as f32,
                );
                Ok(true)
            } else {
                Ok(false)
            }
        },
    );

    // Map overload: `add_entity_sample(id, #{ time, x, y, w, h, confidence })`.
    engine.register_fn(
        "add_entity_sample",
        |movie: &mut MovieHandle, id: &str, props: rhai::Map| -> Result<bool, Box<EvalAltResult>> {
            let id = parse_entity_id(id)?;
            let time = props.get("time").and_then(|v| v.as_float().ok()).unwrap_or(0.0);
            let x = props.get("x").and_then(|v| v.as_float().ok()).unwrap_or(0.0);
            let y = props.get("y").and_then(|v| v.as_float().ok()).unwrap_or(0.0);
            let w = props.get("w").and_then(|v| v.as_float().ok()).unwrap_or(0.0);
            let h = props.get("h").and_then(|v| v.as_float().ok()).unwrap_or(0.0);
            let confidence = props.get("confidence").and_then(|v| v.as_float().ok()).unwrap_or(1.0);
            let mut d = movie.lock_director()?;
            if let Some(track) = d.entities.get_mut(id) {
                track.add_sample(
                    time,
                    BoundingBox::new(x as f32, y as f32, w as f32, h as f32),
                    confidence as f32,
                );
                Ok(true)
            } else {
                Ok(false)
            }
        },
    );

    engine.register_fn(
        "entity_bbox_at",
        |movie: &mut MovieHandle, id: &str, time: f64| -> Result<rhai::Map, Box<EvalAltResult>> {
            let id = parse_entity_id(id)?;
            let d = movie.lock_director()?;
            let mut map = rhai::Map::new();
            if let Some(sample) = d.entities.sample_at(id, time) {
                map.insert("x".into(), rhai::Dynamic::from_float(sample.bbox.x as f64));
                map.insert("y".into(), rhai::Dynamic::from_float(sample.bbox.y as f64));
                map.insert("w".into(), rhai::Dynamic::from_float(sample.bbox.width as f64));
                map.insert("h".into(), rhai::Dynamic::from_float(sample.bbox.height as f64));
                map.insert(
                    "confidence".into(),
                    rhai::Dynamic::from_float(sample.confidence as f64),
                );
                map.insert("found".into(), rhai::Dynamic::from(true));
            } else {
                map.insert("found".into(), rhai::Dynamic::from(false));
            }
            Ok(map)
        },
    );

    engine.register_fn(
        "find_entities",
        |movie: &mut MovieHandle, kind: &str| -> Result<rhai::Array, Box<EvalAltResult>> {
            let kind = EntityKind::from_str(kind).map_err(|e| e.to_string())?;
            let d = movie.lock_director()?;
            let ids = d.entities.find_by_kind(&kind);
            let mut arr = rhai::Array::new();
            for id in ids {
                arr.push(rhai::Dynamic::from(id.to_string()));
            }
            Ok(arr)
        },
    );

    engine.register_fn(
        "entity_count",
        |movie: &mut MovieHandle| -> Result<i64, Box<EvalAltResult>> {
            let d = movie.lock_director()?;
            Ok(d.entities.len() as i64)
        },
    );

    engine.register_fn(
        "delete_entity",
        |movie: &mut MovieHandle, id: &str| -> Result<bool, Box<EvalAltResult>> {
            let id = parse_entity_id(id)?;
            let mut d = movie.lock_director()?;
            Ok(d.entities.remove(id).is_some())
        },
    );

    engine.register_fn(
        "entity_kind",
        |movie: &mut MovieHandle, id: &str| -> Result<String, Box<EvalAltResult>> {
            let id = parse_entity_id(id)?;
            let d = movie.lock_director()?;
            match d.entities.get(id) {
                Some(track) => Ok(track.kind.as_str().to_string()),
                None => Err(format!("entity not found: {}", id).into()),
            }
        },
    );

    engine.register_fn(
        "entity_label",
        |movie: &mut MovieHandle, id: &str| -> Result<String, Box<EvalAltResult>> {
            let id = parse_entity_id(id)?;
            let d = movie.lock_director()?;
            match d.entities.get(id) {
                Some(track) => Ok(track.label.clone()),
                None => Err(format!("entity not found: {}", id).into()),
            }
        },
    );

    engine.register_fn(
        "set_entity_label",
        |movie: &mut MovieHandle, id: &str, label: &str| -> Result<bool, Box<EvalAltResult>> {
            let id = parse_entity_id(id)?;
            let mut d = movie.lock_director()?;
            if let Some(track) = d.entities.get_mut(id) {
                track.label = label.to_string();
                Ok(true)
            } else {
                Ok(false)
            }
        },
    );

    // ========== NODE-LEVEL ENTITY OPERATIONS ==========

    engine.register_fn(
        "attach_entity",
        |node: &mut NodeHandle, id: &str| -> Result<bool, Box<EvalAltResult>> {
            let id = parse_entity_id(id)?;
            let mut d = node.lock_director()?;
            node.ensure_alive(&d)?;
            if let Some(n) = d.scene.get_node_mut(node.id) {
                if !n.entity_refs.contains(&id) {
                    n.entity_refs.push(id);
                }
                Ok(true)
            } else {
                Ok(false)
            }
        },
    );

    engine.register_fn(
        "detach_entity",
        |node: &mut NodeHandle, id: &str| -> Result<bool, Box<EvalAltResult>> {
            let id = parse_entity_id(id)?;
            let mut d = node.lock_director()?;
            node.ensure_alive(&d)?;
            if let Some(n) = d.scene.get_node_mut(node.id) {
                n.entity_refs.retain(|&x| x != id);
                Ok(true)
            } else {
                Ok(false)
            }
        },
    );

    engine.register_fn(
        "entity_ids",
        |node: &mut NodeHandle| -> Result<rhai::Array, Box<EvalAltResult>> {
            let d = node.lock_director()?;
            node.ensure_alive(&d)?;
            let mut arr = rhai::Array::new();
            if let Some(n) = d.scene.get_node(node.id) {
                for id in &n.entity_refs {
                    arr.push(rhai::Dynamic::from(id.to_string()));
                }
            }
            Ok(arr)
        },
    );

    engine.register_fn(
        "entity_bboxes_at",
        |node: &mut NodeHandle, time: f64| -> Result<rhai::Array, Box<EvalAltResult>> {
            let d = node.lock_director()?;
            node.ensure_alive(&d)?;
            let mut arr = rhai::Array::new();
            if let Some(n) = d.scene.get_node(node.id) {
                for id in &n.entity_refs {
                    let mut map = rhai::Map::new();
                    map.insert("id".into(), rhai::Dynamic::from(id.to_string()));
                    if let Some(sample) = d.entities.sample_at(*id, time) {
                        map.insert("x".into(), rhai::Dynamic::from_float(sample.bbox.x as f64));
                        map.insert("y".into(), rhai::Dynamic::from_float(sample.bbox.y as f64));
                        map.insert("w".into(), rhai::Dynamic::from_float(sample.bbox.width as f64));
                        map.insert("h".into(), rhai::Dynamic::from_float(sample.bbox.height as f64));
                        map.insert(
                            "confidence".into(),
                            rhai::Dynamic::from_float(sample.confidence as f64),
                        );
                        map.insert("found".into(), rhai::Dynamic::from(true));
                    } else {
                        map.insert("found".into(), rhai::Dynamic::from(false));
                    }
                    arr.push(rhai::Dynamic::from(map));
                }
            }
            Ok(arr)
        },
    );
}

fn parse_entity_id(id: &str) -> Result<Uuid, Box<EvalAltResult>> {
    Uuid::parse_str(id).map_err(|e| format!("invalid entity id '{}': {}", id, e).into())
}
