//! # Compiler API
//!
//! Rhai bindings for the no-op `compile()` phase and dependency invalidation.
//!
//! ## Responsibilities
//! - `compile(movie)` — build the dependency graph before export.
//! - `invalidate_*` — mark nodes dirty by input type.
//! - `dirty_nodes(movie)` / `compiler_edge_count(movie)` — introspection.

use crate::compiler::DependencyInput;
use rhai::Engine;

use super::super::types::MovieHandle;
use super::super::utils::{node_id_from_dynamic, node_id_index_i64};

/// Register compiler-related Rhai functions.
pub fn register(engine: &mut Engine) {
    engine.register_fn(
        "compile",
        |movie: &mut MovieHandle| -> Result<(), Box<rhai::EvalAltResult>> {
            let mut d = movie.lock_director()?;
            d.compile();
            Ok(())
        },
    );

    engine.register_fn(
        "invalidate_entity",
        |movie: &mut MovieHandle, entity_id: &str| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let id = uuid::Uuid::parse_str(entity_id)
                .map_err(|e| format!("invalid entity UUID: {}", e))?;
            let mut d = movie.lock_director()?;
            if d.compiler.is_empty() {
                d.compile();
            }
            let affected = d.invalidate(vec![DependencyInput::Entity(id)]);
            let arr: rhai::Array = affected
                .into_iter()
                .map(|id| rhai::Dynamic::from_int(node_id_index_i64(id)))
                .collect();
            Ok(arr)
        },
    );

    engine.register_fn(
        "invalidate_audio_track",
        |movie: &mut MovieHandle, track_id: i64| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mut d = movie.lock_director()?;
            if d.compiler.is_empty() {
                d.compile();
            }
            let affected = d.invalidate(vec![DependencyInput::AudioTrack(track_id as usize)]);
            let arr: rhai::Array = affected
                .into_iter()
                .map(|id| rhai::Dynamic::from_int(node_id_index_i64(id)))
                .collect();
            Ok(arr)
        },
    );

    engine.register_fn(
        "invalidate_node",
        |movie: &mut MovieHandle, node_id: i64| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mut d = movie.lock_director()?;
            if d.compiler.is_empty() {
                d.compile();
            }
            let id = d
                .scene
                .id_at(node_id as usize)
                .ok_or_else(|| format!("no live node at index {node_id}"))?;
            let affected = d.invalidate(vec![DependencyInput::Node(id)]);
            let arr: rhai::Array = affected
                .into_iter()
                .map(|id| rhai::Dynamic::from_int(node_id_index_i64(id)))
                .collect();
            Ok(arr)
        },
    );

    engine.register_fn(
        "dirty_nodes",
        |movie: &mut MovieHandle| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let d = movie.lock_director()?;
            let arr: rhai::Array = d
                .dirty_nodes
                .iter()
                .map(|&id| rhai::Dynamic::from_int(node_id_index_i64(id)))
                .collect();
            Ok(arr)
        },
    );

    engine.register_fn(
        "compiler_edge_count",
        |movie: &mut MovieHandle| -> Result<i64, Box<rhai::EvalAltResult>> {
            let d = movie.lock_director()?;
            Ok(d.compiler.edge_count() as i64)
        },
    );

    // `compiler_inputs_for_node(movie, node_id)` — list the dependency inputs
    // declared by a node. Auto-compiles the graph if it has not been built yet.
    engine.register_fn(
        "compiler_inputs_for_node",
        |movie: &mut MovieHandle, node_id: i64| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mut d = movie.lock_director()?;
            if d.compiler.is_empty() {
                d.compile();
            }
            let id = d
                .scene
                .id_at(node_id as usize)
                .ok_or_else(|| format!("no live node at index {node_id}"))?;
            let inputs = d.compiler.inputs_of(id);
            Ok(inputs.into_iter().map(dependency_input_to_rhai_map).collect())
        },
    );

    // `compiler_dependents_of_entity(movie, entity_id)` — list node IDs that
    // depend on the given entity. Auto-compiles the graph if needed.
    engine.register_fn(
        "compiler_dependents_of_entity",
        |movie: &mut MovieHandle, entity_id: &str| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let id = uuid::Uuid::parse_str(entity_id)
                .map_err(|e| format!("invalid entity UUID: {}", e))?;
            let mut d = movie.lock_director()?;
            if d.compiler.is_empty() {
                d.compile();
            }
            let dependents = d.compiler.dependents_of(&DependencyInput::Entity(id));
            Ok(dependents
                .into_iter()
                .map(|id| rhai::Dynamic::from_int(node_id_index_i64(id)))
                .collect())
        },
    );

    // `compiler_dependents_of_audio_track(movie, track_id)` — list node IDs that
    // depend on the given audio track. Auto-compiles the graph if needed.
    engine.register_fn(
        "compiler_dependents_of_audio_track",
        |movie: &mut MovieHandle, track_id: i64| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mut d = movie.lock_director()?;
            if d.compiler.is_empty() {
                d.compile();
            }
            let dependents = d.compiler.dependents_of(&DependencyInput::AudioTrack(track_id as usize));
            Ok(dependents
                .into_iter()
                .map(|id| rhai::Dynamic::from_int(node_id_index_i64(id)))
                .collect())
        },
    );

    // `compiler_dependents_of_node(movie, node_id)` — list node IDs that depend
    // on the given node. Auto-compiles the graph if needed.
    engine.register_fn(
        "compiler_dependents_of_node",
        |movie: &mut MovieHandle, node_id: i64| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mut d = movie.lock_director()?;
            if d.compiler.is_empty() {
                d.compile();
            }
            let id = d
                .scene
                .id_at(node_id as usize)
                .ok_or_else(|| format!("no live node at index {node_id}"))?;
            let dependents = d.compiler.dependents_of(&DependencyInput::Node(id));
            Ok(dependents
                .into_iter()
                .map(|id| rhai::Dynamic::from_int(node_id_index_i64(id)))
                .collect())
        },
    );

    // `compiler_graph_json(movie)` — dump the full dependency graph as a JSON
    // string for debugging. Auto-compiles the graph if needed.
    engine.register_fn(
        "compiler_graph_json",
        |movie: &mut MovieHandle| -> Result<String, Box<rhai::EvalAltResult>> {
            let mut d = movie.lock_director()?;
            if d.compiler.is_empty() {
                d.compile();
            }
            d.compiler
                .to_json()
                .map_err(|e| format!("failed to serialize dependency graph: {}", e).into())
        },
    );

    // `compiler_affected_nodes(movie, inputs)` — return the transitive set of
    // node IDs that would be affected by the given dependency inputs, without
    // marking anything dirty. Each input is a map with `type` and `value` keys
    // matching the format produced by `compiler_inputs_for_node`.
    engine.register_fn(
        "compiler_affected_nodes",
        |movie: &mut MovieHandle, inputs: rhai::Array| -> Result<rhai::Array, Box<rhai::EvalAltResult>> {
            let mut parsed = Vec::with_capacity(inputs.len());
            for item in inputs {
                let map = item
                    .try_cast::<rhai::Map>()
                    .ok_or_else(|| "expected array of dependency input maps".to_string())?;
                parsed.push(dependency_input_from_rhai_map(&map)?);
            }

            let d = movie.lock_director()?;
            // Resolve bare integer node/children values against the live scene
            // so generation matches compile-time NodeIds.
            let mut resolved = Vec::with_capacity(parsed.len());
            for input in parsed {
                resolved.push(resolve_dependency_input_live(&d, input)?);
            }
            let affected = d.affected_nodes(resolved);
            Ok(affected
                .into_iter()
                .map(|id| rhai::Dynamic::from_int(node_id_index_i64(id)))
                .collect())
        },
    );
}

fn resolve_dependency_input_live(
    director: &crate::director::Director,
    input: DependencyInput,
) -> Result<DependencyInput, Box<rhai::EvalAltResult>> {
    match input {
        DependencyInput::Node(id) => {
            let live = director
                .scene
                .id_at(id.index())
                .ok_or_else(|| format!("no live node at index {}", id.index))?;
            Ok(DependencyInput::Node(live))
        }
        DependencyInput::Children(id) => {
            let live = director
                .scene
                .id_at(id.index())
                .ok_or_else(|| format!("no live node at index {}", id.index))?;
            Ok(DependencyInput::Children(live))
        }
        other => Ok(other),
    }
}

fn dependency_input_from_rhai_map(map: &rhai::Map) -> Result<DependencyInput, String> {
    let ty = map
        .get("type")
        .and_then(|v| v.clone().try_cast::<String>())
        .ok_or_else(|| "dependency input map missing 'type' string".to_string())?;
    let value = map
        .get("value")
        .ok_or_else(|| "dependency input map missing 'value'".to_string())?;

    match ty.as_str() {
        "entity" => {
            let s = value
                .clone()
                .try_cast::<String>()
                .ok_or_else(|| "entity input value must be a string UUID".to_string())?;
            let id = uuid::Uuid::parse_str(&s)
                .map_err(|e| format!("invalid entity UUID: {}", e))?;
            Ok(DependencyInput::Entity(id))
        }
        "audio_track" => {
            let id = value
                .as_int()
                .map_err(|_| "audio_track input value must be an integer".to_string())?;
            Ok(DependencyInput::AudioTrack(id as usize))
        }
        "node" => {
            let id = node_id_from_dynamic(value)?;
            Ok(DependencyInput::Node(id))
        }
        "children" => {
            let id = node_id_from_dynamic(value)?;
            Ok(DependencyInput::Children(id))
        }
        _ => Err(format!("unknown dependency input type: {}", ty)),
    }
}

fn dependency_input_to_rhai_map(input: DependencyInput) -> rhai::Dynamic {
    let mut map = rhai::Map::new();
    match input {
        DependencyInput::Entity(id) => {
            map.insert("type".into(), "entity".into());
            map.insert("value".into(), id.to_string().into());
        }
        DependencyInput::AudioTrack(id) => {
            map.insert("type".into(), "audio_track".into());
            map.insert("value".into(), (id as i64).into());
        }
        DependencyInput::Node(id) => {
            map.insert("type".into(), "node".into());
            // Index form keeps existing Rhai scripts working; generation is
            // recovered via live scene resolve on the way back in.
            map.insert("value".into(), node_id_index_i64(id).into());
        }
        DependencyInput::Children(id) => {
            map.insert("type".into(), "children".into());
            map.insert("value".into(), node_id_index_i64(id).into());
        }
    }
    rhai::Dynamic::from_map(map)
}

