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
                .map(|id| rhai::Dynamic::from_int(id as i64))
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
                .map(|id| rhai::Dynamic::from_int(id as i64))
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
            let affected = d.invalidate(vec![DependencyInput::Node(node_id as usize)]);
            let arr: rhai::Array = affected
                .into_iter()
                .map(|id| rhai::Dynamic::from_int(id as i64))
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
                .map(|&id| rhai::Dynamic::from_int(id as i64))
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
            let inputs = d.compiler.inputs_of(node_id as usize);
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
            Ok(dependents.into_iter().map(|id| rhai::Dynamic::from_int(id as i64)).collect())
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
            Ok(dependents.into_iter().map(|id| rhai::Dynamic::from_int(id as i64)).collect())
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
            let dependents = d.compiler.dependents_of(&DependencyInput::Node(node_id as usize));
            Ok(dependents.into_iter().map(|id| rhai::Dynamic::from_int(id as i64)).collect())
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
            map.insert("value".into(), (id as i64).into());
        }
    }
    rhai::Dynamic::from_map(map)
}
