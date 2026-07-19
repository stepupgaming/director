use director_core::compiler::{DependencyGraph, DependencyInput};
use director_core::entity::EntityKind;
use director_core::node::BoxNode;
use director_core::node::text::TextNode;
use director_core::scene::AudioBinding;
use director_core::scripting::register_rhai_api;
use director_core::{DefaultAssetLoader, Director, video_wrapper::RenderMode};
use rhai::Engine;
use skia_safe::textlayout::FontCollection;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

fn empty_director() -> Director {
    Director::new(
        1920,
        1080,
        30.0,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    )
}

#[test]
fn compile_records_entity_dependency() {
    let mut d = empty_director();
    let id = d.entities.create(EntityKind::Face, "mara".to_string());

    let root = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(root, child);

    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.entity_refs.push(id);
    }

    d.compile();

    assert!(d.compiler.edge_count() > 0);
    let dependents = d.compiler.dependents_of(&DependencyInput::Entity(id));
    assert!(dependents.contains(&child));
}

#[test]
fn compile_records_audio_dependency() {
    let mut d = empty_director();
    let root = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(root, child);

    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.audio_bindings.push(AudioBinding {
            track_id: 3,
            band: "bass".to_string(),
            property: "scale".to_string(),
            min_value: 0.0,
            max_value: 1.0,
            smoothing: 0.0,
            prev_value: 0.0,
        });
    }

    d.compile();

    let dependents = d.compiler.dependents_of(&DependencyInput::AudioTrack(3));
    assert!(dependents.contains(&child));
}

#[test]
fn compile_records_hierarchy_dependency() {
    let mut d = empty_director();
    let root = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(root, child);

    d.compile();

    let dependents = d.compiler.dependents_of(&DependencyInput::Node(root));
    assert!(dependents.contains(&child));
}

#[test]
fn compile_records_mask_dependency() {
    let mut d = empty_director();
    let masked = d.scene.add_node(Box::new(BoxNode::new()));
    let mask = d.scene.add_node(Box::new(BoxNode::new()));

    {
        let node = d.scene.get_node_mut(masked).unwrap();
        node.mask_node = Some(mask);
    }

    d.compile();

    let dependents = d.compiler.dependents_of(&DependencyInput::Node(mask));
    assert!(dependents.contains(&masked));
}

#[test]
fn invalidate_stores_dirty_nodes_when_input_changes() {
    let mut d = empty_director();
    let id = d.entities.create(EntityKind::Face, "mara".to_string());

    let root = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(root, child);

    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.entity_refs.push(id);
    }

    d.compile();

    // Change the child's structural input so the hash differs from the
    // baseline recorded during compile.
    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.entity_refs.clear();
        child_node.entity_refs.push(d.entities.create(EntityKind::Face, "other".to_string()));
    }

    let affected = d.invalidate(vec![DependencyInput::Entity(id)]);
    assert!(affected.contains(&child));
    assert!(d.dirty_nodes.contains(&child));
}

#[test]
fn dependency_graph_transitive_propagation() {
    let mut graph = DependencyGraph::new();
    let entity = Uuid::new_v4();

    use director_core::types::NodeId;
    let n = |i| NodeId::at(i);
    graph.record(n(1), DependencyInput::Entity(entity));
    graph.record(n(2), DependencyInput::Node(n(1)));
    graph.record(n(3), DependencyInput::Node(n(2)));

    let affected = graph.affected_nodes(&[DependencyInput::Entity(entity)]);
    assert!(affected.contains(&n(1)));
    assert!(affected.contains(&n(2)));
    assert!(affected.contains(&n(3)));
}

#[test]
fn compile_clears_dirty_nodes() {
    let mut d = empty_director();
    let id = d.entities.create(EntityKind::Face, "mara".to_string());
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    {
        d.scene.get_node_mut(child).unwrap().entity_refs.push(id);
    }

    d.compile();

    // Change the entity reference so the next invalidation marks the node dirty.
    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.entity_refs.clear();
        child_node.entity_refs.push(d.entities.create(EntityKind::Face, "other".to_string()));
    }

    d.invalidate(vec![DependencyInput::Entity(id)]);
    assert!(!d.dirty_nodes.is_empty());

    d.compile();
    assert!(d.dirty_nodes.is_empty());
}

#[test]
fn transitive_nodes_only_dirty_if_their_own_inputs_changed() {
    let mut d = empty_director();
    let id = d.entities.create(EntityKind::Face, "mara".to_string());

    let parent = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    let grandchild = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(parent, child);
    d.scene.try_add_child(child, grandchild);

    // grandchild depends on the entity; child depends on parent; grandchild
    // depends on child.
    {
        let grandchild_node = d.scene.get_node_mut(grandchild).unwrap();
        grandchild_node.entity_refs.push(id);
    }

    d.compile();

    // Change only the grandchild's entity reference.
    {
        let grandchild_node = d.scene.get_node_mut(grandchild).unwrap();
        grandchild_node.entity_refs.clear();
        grandchild_node.entity_refs.push(d.entities.create(EntityKind::Face, "other".to_string()));
    }

    // Invalidating the parent propagates through the hierarchy to the child
    // and grandchild, but only the grandchild's own inputs changed.
    let affected = d.invalidate(vec![DependencyInput::Node(parent)]);
    assert!(affected.contains(&grandchild));
    assert!(!affected.contains(&child), "child should not be dirty when its own inputs are unchanged");
    assert!(!affected.contains(&parent), "parent should not be dirty when its own inputs are unchanged");
}

#[test]
fn compile_stores_input_hash_on_each_node() {
    let mut d = empty_director();
    let root = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(root, child);

    d.compile();

    assert!(d.input_hashes.contains_key(&root));
    assert!(d.input_hashes.contains_key(&child));
}

#[test]
fn invalidate_skips_unchanged_nodes_by_hash() {
    let mut d = empty_director();
    let id = d.entities.create(EntityKind::Face, "mara".to_string());

    let root = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(root, child);

    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.entity_refs.push(id);
    }

    d.compile();

    // Invalidating with the same entity should not mark the node dirty
    // because its structural inputs have not changed.
    let affected = d.invalidate(vec![DependencyInput::Entity(id)]);
    assert!(!affected.contains(&child));
}

#[test]
fn invalidate_marks_dirty_when_entity_hash_changes() {
    let mut d = empty_director();
    let id = d.entities.create(EntityKind::Face, "mara".to_string());

    let root = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(root, child);

    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.entity_refs.push(id);
    }

    d.compile();

    // Change the node's structural input.
    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.entity_refs.clear();
        child_node.entity_refs.push(d.entities.create(EntityKind::Face, "other".to_string()));
    }

    let affected = d.invalidate(vec![DependencyInput::Entity(id)]);
    assert!(affected.contains(&child));
}

#[test]
fn invalidate_marks_dirty_when_audio_binding_changes() {
    let mut d = empty_director();
    let root = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(root, child);

    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.audio_bindings.push(AudioBinding {
            track_id: 3,
            band: "bass".to_string(),
            property: "scale".to_string(),
            min_value: 0.0,
            max_value: 1.0,
            smoothing: 0.0,
            prev_value: 0.0,
        });
    }

    d.compile();

    // Change the audio binding parameters (not the track id).
    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.audio_bindings[0].band = "mids".to_string();
    }

    let affected = d.invalidate(vec![DependencyInput::AudioTrack(3)]);
    assert!(affected.contains(&child));
}

#[test]
fn invalidate_marks_dirty_when_mask_changes() {
    let mut d = empty_director();
    let masked = d.scene.add_node(Box::new(BoxNode::new()));
    let mask = d.scene.add_node(Box::new(BoxNode::new()));

    {
        let node = d.scene.get_node_mut(masked).unwrap();
        node.mask_node = Some(mask);
    }

    d.compile();

    // Change the mask target.
    let other_mask = d.scene.add_node(Box::new(BoxNode::new()));
    {
        let node = d.scene.get_node_mut(masked).unwrap();
        node.mask_node = Some(other_mask);
    }

    let affected = d.invalidate(vec![DependencyInput::Node(mask)]);
    assert!(affected.contains(&masked));
}

#[test]
fn invalidate_keeps_node_dirty_until_compile_updates_baseline() {
    let mut d = empty_director();
    let id = d.entities.create(EntityKind::Face, "mara".to_string());

    let child = d.scene.add_node(Box::new(BoxNode::new()));
    {
        d.scene.get_node_mut(child).unwrap().entity_refs.push(id);
    }

    d.compile();

    // Change the entity reference so the hash differs from the baseline.
    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.entity_refs.clear();
        child_node.entity_refs.push(d.entities.create(EntityKind::Face, "other".to_string()));
    }

    // First invalidation marks the node dirty.
    let affected1 = d.invalidate(vec![DependencyInput::Entity(id)]);
    assert!(affected1.contains(&child));

    // Second invalidation (without compile) should still mark it dirty because
    // the baseline hash has not been updated yet.
    let affected2 = d.invalidate(vec![DependencyInput::Entity(id)]);
    assert!(affected2.contains(&child));

    // After compile, the baseline matches the current hash.
    d.compile();
    let affected3 = d.invalidate(vec![DependencyInput::Entity(id)]);
    assert!(!affected3.contains(&child));
}

#[test]
fn invalidate_marks_dirty_when_structural_property_changes() {
    let mut d = empty_director();
    let id = d.entities.create(EntityKind::Face, "mara".to_string());

    let child = d.scene.add_node(Box::new(BoxNode::new()));
    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.entity_refs.push(id);
    }

    d.compile();

    // Change a structural property that is not a dependency input (z-index).
    // The node must be invalidated directly because z-index has no external
    // dependency input.
    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.z_index = 42;
    }

    let affected = d.invalidate(vec![DependencyInput::Node(child)]);
    assert!(affected.contains(&child));
}

#[test]
fn invalidating_children_input_marks_parent_dirty_when_child_added() {
    let mut d = empty_director();
    let parent = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(parent, child);

    d.compile();

    // Add another child to the parent after compile.
    let new_child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(parent, new_child);

    // Invalidating the parent's children collection should mark the parent
    // dirty because its structural input hash (which includes children) changed.
    let affected = d.invalidate(vec![DependencyInput::Children(parent)]);
    assert!(affected.contains(&parent), "parent should be dirty when children change");
}

#[test]
fn invalidating_children_input_marks_parent_dirty_when_child_removed() {
    let mut d = empty_director();
    let parent = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(parent, child);

    d.compile();

    // Remove the child from the parent after compile.
    d.scene.remove_child(parent, child);

    let affected = d.invalidate(vec![DependencyInput::Children(parent)]);
    assert!(affected.contains(&parent), "parent should be dirty when a child is removed");
}

#[test]
fn invalidating_children_input_does_not_mark_parent_dirty_when_unchanged() {
    let mut d = empty_director();
    let parent = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(parent, child);

    d.compile();

    // No hierarchy changes.
    let affected = d.invalidate(vec![DependencyInput::Children(parent)]);
    assert!(!affected.contains(&parent), "parent should not be dirty when children are unchanged");
}

#[test]
fn director_add_child_invalidates_parent_automatically() {
    let mut d = empty_director();
    let parent = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));

    d.compile();

    // Adding a child through Director::add_child should automatically
    // invalidate the parent's children dependency.
    d.add_child(parent, child);
    assert!(d.dirty_nodes.contains(&parent), "parent should be dirty after adding a child");
}

#[test]
fn director_remove_child_invalidates_parent_automatically() {
    let mut d = empty_director();
    let parent = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(parent, child);

    d.compile();

    // Removing a child through Director::remove_child should automatically
    // invalidate the parent's children dependency.
    d.remove_child(parent, child);
    assert!(d.dirty_nodes.contains(&parent), "parent should be dirty after removing a child");
}

#[test]
fn invalidating_parent_does_not_dirty_siblings_with_unchanged_inputs() {
    let mut d = empty_director();
    let parent = d.scene.add_node(Box::new(BoxNode::new()));
    let child_a = d.scene.add_node(Box::new(BoxNode::new()));
    let child_b = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(parent, child_a);
    d.scene.try_add_child(parent, child_b);

    d.compile();

    // Invalidating the parent affects both children via the hierarchy, but
    // neither child's inputs changed, so neither is dirty.
    let affected = d.invalidate(vec![DependencyInput::Node(parent)]);
    assert!(!affected.contains(&child_a));
    assert!(!affected.contains(&child_b));
}

#[test]
fn invalidate_without_compile_is_no_op() {
    let mut d = empty_director();
    let id = d.entities.create(EntityKind::Face, "mara".to_string());

    let child = d.scene.add_node(Box::new(BoxNode::new()));
    {
        d.scene.get_node_mut(child).unwrap().entity_refs.push(id);
    }

    // invalidate() before compile() has no dependency graph to traverse, so it
    // returns an empty set and does not snapshot a hash.
    let affected = d.invalidate(vec![DependencyInput::Entity(id)]);
    assert!(affected.is_empty());
    assert!(!d.input_hashes.contains_key(&child));
}

#[test]
fn rust_introspection_lists_inputs_for_node() {
    let mut d = empty_director();
    let id = d.entities.create(EntityKind::Face, "mara".to_string());

    let root = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(root, child);

    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.entity_refs.push(id);
        child_node.audio_bindings.push(AudioBinding {
            track_id: 2,
            band: "bass".to_string(),
            property: "scale".to_string(),
            min_value: 0.0,
            max_value: 1.0,
            smoothing: 0.0,
            prev_value: 0.0,
        });
    }

    d.compile();

    let inputs = d.compiler.inputs_of(child);
    assert!(inputs.contains(&DependencyInput::Entity(id)));
    assert!(inputs.contains(&DependencyInput::AudioTrack(2)));
    assert!(inputs.contains(&DependencyInput::Node(root)));
}

#[test]
fn rust_introspection_lists_dependents_of_input() {
    let mut d = empty_director();
    let id = d.entities.create(EntityKind::Face, "mara".to_string());

    let root = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(root, child);

    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.entity_refs.push(id);
    }

    d.compile();

    let dependents = d.compiler.dependents_of(&DependencyInput::Entity(id));
    assert!(dependents.contains(&child));
}

#[test]
fn rust_introspection_dumps_graph_json() {
    let mut d = empty_director();
    let id = d.entities.create(EntityKind::Face, "mara".to_string());

    let root = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.try_add_child(root, child);

    {
        let child_node = d.scene.get_node_mut(child).unwrap();
        child_node.entity_refs.push(id);
    }

    d.compile();

    let json = d.compiler.to_json().expect("graph JSON must serialize");
    assert!(json.contains("edges"));
    assert!(json.contains("node_inputs"));
    assert!(json.contains(&id.to_string()));

    // Verify it is valid JSON and follows the expected array-of-pairs shape.
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("graph JSON must parse");
    let edges = parsed.get("edges").and_then(|v| v.as_array()).expect("edges must be an array");
    assert!(!edges.is_empty(), "edges array must not be empty");
    assert!(edges[0].get("input").is_some(), "each edge must have an input");
    assert!(edges[0].get("dependents").is_some(), "each edge must have dependents");

    let node_inputs = parsed.get("node_inputs").and_then(|v| v.as_array()).expect("node_inputs must be an array");
    assert!(!node_inputs.is_empty(), "node_inputs array must not be empty");
    assert!(node_inputs[0].get("node").is_some(), "each node_inputs entry must have a node");
    assert!(node_inputs[0].get("inputs").is_some(), "each node_inputs entry must have inputs");
}

#[test]
fn rhai_introspection_bindings_work() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(640, 360, 30);
        let scene = movie.add_scene(2.5);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let inner = root.add_box(#{ width: 100.0, height: 100.0 });

        let inputs = compiler_inputs_for_node(movie, node_id(inner));
        let dependents = compiler_dependents_of_node(movie, node_id(root));
        let json = compiler_graph_json(movie);

        #{
            inner_id: node_id(inner),
            inputs: inputs,
            dependents: dependents,
            json: json,
        }
    "#;

    let result = engine.eval::<rhai::Map>(script).expect("introspection script must evaluate");
    let inner_id: i64 = result
        .get("inner_id")
        .and_then(|v| v.as_int().ok())
        .expect("inner_id must be an integer");
    let inputs: rhai::Array = result
        .get("inputs")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("inputs must be an array");
    let dependents: rhai::Array = result
        .get("dependents")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("dependents must be an array");
    let json: String = result
        .get("json")
        .and_then(|v| v.clone().into_string().ok())
        .expect("json must be a string");

    // inner depends on root via the parent hierarchy.
    assert!(!inputs.is_empty(), "inner should have at least one dependency input");
    let has_node_input = inputs.iter().any(|v| {
        v.clone()
            .try_cast::<rhai::Map>()
            .map(|m| m.get("type").and_then(|t| t.clone().into_string().ok()) == Some("node".to_string()))
            .unwrap_or(false)
    });
    assert!(has_node_input, "inner should have a node dependency input");

    // root is a parent, so nodes depending on root include inner.
    assert!(!dependents.is_empty(), "root should have at least one dependent");
    assert!(dependents.iter().any(|v| v.as_int().ok() == Some(inner_id)),
        "inner should be listed as a dependent of root");

    // JSON dump should contain graph structure.
    assert!(json.contains("edges"), "graph JSON should contain edges");
    assert!(json.contains("node_inputs"), "graph JSON should contain node_inputs");
}

#[test]
fn compile_emits_style_policy_violations() {
    let mut d = empty_director();

    // Add a text node whose font size exceeds the default maximum.
    let mut text = TextNode::new(vec![], Arc::new(Mutex::new(FontCollection::new())));
    text.default_font_size.current_value = 250.0;
    let id = d.scene.add_node(Box::new(text));
    d.scene.get_node_mut(id).unwrap().layout_rect = skia_safe::Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

    // Give the timeline a non-zero duration so the scorer has a range.
    d.push_scene_root(id, 2.0);

    d.compile();

    let score = d.last_policy_score.as_ref().expect("compile should produce a policy score");
    assert!(
        score.violations.iter().any(|v| v.rule == "typography.max_font_size"),
        "compile should emit a typography.max_font_size violation"
    );
}

#[test]
fn compile_emits_no_violations_for_empty_scene() {
    let mut d = empty_director();
    d.compile();

    let score = d.last_policy_score.as_ref().expect("compile should produce a policy score");
    assert!(score.violations.is_empty(), "empty scene should have no violations");
    assert_eq!(score.total_score, 1.0);
}

#[test]
fn rhai_add_child_invalidates_parent_automatically() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(640, 360, 30);
        let scene = movie.add_scene(2.5);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let inner = root.add_box(#{ width: 100.0, height: 100.0 });

        compile(movie);
        let dirty_before = dirty_nodes(movie);

        let another = root.add_box(#{ width: 50.0, height: 50.0 });
        let dirty_after = dirty_nodes(movie);

        #{
            root_id: node_id(root),
            dirty_before: dirty_before,
            dirty_after: dirty_after,
        }
    "#;

    let result = engine.eval::<rhai::Map>(script).expect("add_child invalidation script must evaluate");
    let root_id: i64 = result
        .get("root_id")
        .and_then(|v| v.as_int().ok())
        .expect("root_id must be an integer");
    let dirty_before: rhai::Array = result
        .get("dirty_before")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("dirty_before must be an array");
    let dirty_after: rhai::Array = result
        .get("dirty_after")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("dirty_after must be an array");

    assert!(dirty_before.is_empty(), "dirty_nodes should be empty immediately after compile");
    assert!(
        dirty_after.iter().any(|v| v.as_int().ok() == Some(root_id)),
        "parent should be dirty after Rhai add_child attaches another child"
    );
}

#[test]
fn rhai_destroy_invalidates_parent_automatically() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(640, 360, 30);
        let scene = movie.add_scene(2.5);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let inner = root.add_box(#{ width: 100.0, height: 100.0 });

        compile(movie);
        let dirty_before = dirty_nodes(movie);

        inner.destroy();
        let dirty_after = dirty_nodes(movie);

        #{
            root_id: node_id(root),
            dirty_before: dirty_before,
            dirty_after: dirty_after,
        }
    "#;

    let result = engine.eval::<rhai::Map>(script).expect("destroy invalidation script must evaluate");
    let root_id: i64 = result
        .get("root_id")
        .and_then(|v| v.as_int().ok())
        .expect("root_id must be an integer");
    let dirty_before: rhai::Array = result
        .get("dirty_before")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("dirty_before must be an array");
    let dirty_after: rhai::Array = result
        .get("dirty_after")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("dirty_after must be an array");

    assert!(dirty_before.is_empty(), "dirty_nodes should be empty immediately after compile");
    assert!(
        dirty_after.iter().any(|v| v.as_int().ok() == Some(root_id)),
        "parent should be dirty after Rhai destroy removes a child from its parent"
    );
}

#[test]
fn rhai_remove_child_invalidates_parent_automatically() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(640, 360, 30);
        let scene = movie.add_scene(2.5);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let inner = root.add_box(#{ width: 100.0, height: 100.0 });

        compile(movie);
        let dirty_before = dirty_nodes(movie);

        root.remove_child(inner);
        let dirty_after = dirty_nodes(movie);

        #{
            root_id: node_id(root),
            dirty_before: dirty_before,
            dirty_after: dirty_after,
        }
    "#;

    let result = engine.eval::<rhai::Map>(script).expect("remove_child invalidation script must evaluate");
    let root_id: i64 = result
        .get("root_id")
        .and_then(|v| v.as_int().ok())
        .expect("root_id must be an integer");
    let dirty_before: rhai::Array = result
        .get("dirty_before")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("dirty_before must be an array");
    let dirty_after: rhai::Array = result
        .get("dirty_after")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("dirty_after must be an array");

    assert!(dirty_before.is_empty(), "dirty_nodes should be empty immediately after compile");
    assert!(
        dirty_after.iter().any(|v| v.as_int().ok() == Some(root_id)),
        "parent should be dirty after Rhai remove_child detaches a child"
    );
}

#[test]
fn rhai_compiler_affected_nodes_does_not_dirty() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(640, 360, 30);
        let scene = movie.add_scene(2.5);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let inner = root.add_box(#{ width: 100.0, height: 100.0 });

        compile(movie);

        let affected = compiler_affected_nodes(movie, [
            #{ type: "node", value: node_id(root) }
        ]);
        let dirty = dirty_nodes(movie);

        #{
            root_id: node_id(root),
            inner_id: node_id(inner),
            affected: affected,
            dirty: dirty,
        }
    "#;

    let result = engine.eval::<rhai::Map>(script).expect("affected_nodes script must evaluate");
    let root_id: i64 = result
        .get("root_id")
        .and_then(|v| v.as_int().ok())
        .expect("root_id must be an integer");
    let inner_id: i64 = result
        .get("inner_id")
        .and_then(|v| v.as_int().ok())
        .expect("inner_id must be an integer");
    let affected: rhai::Array = result
        .get("affected")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("affected must be an array");
    let dirty: rhai::Array = result
        .get("dirty")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("dirty must be an array");

    let affected_ids: Vec<i64> = affected.iter().filter_map(|v| v.as_int().ok()).collect();
    assert!(affected_ids.contains(&inner_id), "inner should be affected by parent node input");
    assert!(affected_ids.contains(&root_id), "root should be included because the input is the root node itself");
    assert!(dirty.is_empty(), "compiler_affected_nodes must not mark anything dirty");
}

#[test]
fn rhai_compiler_affected_nodes_supports_all_input_types() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(640, 360, 30);
        let scene = movie.add_scene(2.5);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let inner = root.add_box(#{ width: 100.0, height: 100.0 });

        compile(movie);

        let affected = compiler_affected_nodes(movie, [
            #{ type: "children", value: node_id(root) }
        ]);

        #{
            root_id: node_id(root),
            affected: affected,
        }
    "#;

    let result = engine.eval::<rhai::Map>(script).expect("affected_nodes children script must evaluate");
    let root_id: i64 = result
        .get("root_id")
        .and_then(|v| v.as_int().ok())
        .expect("root_id must be an integer");
    let affected: rhai::Array = result
        .get("affected")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("affected must be an array");

    let affected_ids: Vec<i64> = affected.iter().filter_map(|v| v.as_int().ok()).collect();
    assert!(affected_ids.contains(&root_id), "parent should be affected by its own children input");
}

#[test]
fn rhai_compiler_affected_nodes_parses_entity_and_audio_inputs() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(640, 360, 30);
        let scene = movie.add_scene(2.5);
        let root = scene.add_box(#{ width: "100%", height: "100%" });

        compile(movie);

        let affected = compiler_affected_nodes(movie, [
            #{ type: "entity", value: "550e8400-e29b-41d4-a716-446655440000" },
            #{ type: "audio_track", value: 0 }
        ]);

        #{
            affected: affected,
        }
    "#;

    let result = engine.eval::<rhai::Map>(script).expect("entity/audio input parser script must evaluate");
    let affected: rhai::Array = result
        .get("affected")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("affected must be an array");

    assert!(affected.is_empty(), "no nodes depend on the dummy entity/audio inputs");
}

#[test]
fn rhai_compiler_bindings_work() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(640, 360, 30);
        let scene = movie.add_scene(2.5);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let inner = root.add_box(#{ width: 100.0, height: 100.0 });

        // Auto-compile happens on first invalidate. Because the node's inputs
        // have not changed since compile(), hash-based invalidation keeps the
        // affected/dirty sets empty.
        let affected = invalidate_node(movie, node_id(inner));
        let dirty = dirty_nodes(movie);
        let edges = compiler_edge_count(movie);

        #{
            affected: affected,
            dirty: dirty,
            edges: edges,
        }
    "#;

    let result = engine.eval::<rhai::Map>(script).expect("compiler script must evaluate");
    let affected: rhai::Array = result
        .get("affected")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("affected must be an array");
    let dirty: rhai::Array = result
        .get("dirty")
        .and_then(|v| v.clone().try_cast::<rhai::Array>())
        .expect("dirty must be an array");
    let edges: i64 = result
        .get("edges")
        .and_then(|v| v.as_int().ok())
        .expect("edges must be an integer");

    assert!(affected.is_empty(), "unchanged node should not be affected by hash-based invalidation");
    assert!(dirty.is_empty(), "dirty_nodes should be empty when inputs are unchanged");
    assert!(edges > 0, "dependency graph should have edges after auto-compile");
}
