//! Plan 02 acceptance: generational NodeId, mutation policy, nested identity boundary.

use director_core::animation::Animated;
use director_core::compiler::DependencyInput;
use director_core::director::Director;
use director_core::element::Element;
use director_core::node::composition::CompositionNode;
use director_core::node::{BoxNode, EffectNode, EffectType};
use director_core::scripting::utils::apply_effect_to_node;
use director_core::scripting::{self, MovieHandle, NodeHandle};
use director_core::types::NodeId;
use director_core::video_wrapper::RenderMode;
use director_core::DefaultAssetLoader;
use rhai::Engine;
use std::sync::{Arc, Mutex};

fn empty_director() -> Director {
    Director::new(
        320,
        240,
        30.0,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    )
}

#[test]
fn node_id_no_alias_after_destroy() {
    let mut d = empty_director();
    let old = d.scene.add_node(Box::new(BoxNode::new()));
    d.scene.destroy_node(old);
    let fresh = d.scene.add_node(Box::new(BoxNode::new()));

    assert_eq!(old.index, fresh.index, "slot should recycle");
    assert_ne!(old.generation, fresh.generation, "generation must bump");
    assert!(d.scene.get_node(old).is_none());
    assert!(d.scene.get_node(fresh).is_some());
}

#[test]
fn stale_handle_rhai_errors() {
    let director = Arc::new(Mutex::new(empty_director()));
    let node_id = {
        let mut d = director.lock().unwrap();
        d.scene.add_node(Box::new(BoxNode::new()))
    };
    let handle = NodeHandle {
        director: director.clone(),
        id: node_id,
    };
    {
        let d = director.lock().unwrap();
        assert!(handle.ensure_alive(&d).is_ok());
    }
    {
        let mut d = director.lock().unwrap();
        d.scene.destroy_node(node_id);
        // Recycle the slot so a naive index-only check would pass.
        let _ = d.scene.add_node(Box::new(BoxNode::new()));
    }
    let d = director.lock().unwrap();
    let err = handle.ensure_alive(&d).unwrap_err().to_string();
    assert!(
        err.contains("stale"),
        "expected stale-handle error, got: {err}"
    );
}

#[test]
fn apply_effect_handle_policy() {
    let mut d = empty_director();
    let parent = d.scene.add_node(Box::new(BoxNode::new()));
    let target = d.scene.add_node(Box::new(BoxNode::new()));
    assert!(d.scene.try_add_child(parent, target));
    d.push_scene_root(target, 1.0);

    let wrapper = apply_effect_to_node(&mut d, target, EffectType::Blur(Animated::new(4.0)));

    assert_ne!(wrapper, target, "rewrite returns new outer id");
    assert!(
        d.scene.get_node(target).is_some(),
        "inner target stays alive under the wrapper"
    );
    assert!(d.scene.get_node(wrapper).is_some());
    assert!(
        d.scene
            .get_node(wrapper)
            .unwrap()
            .element
            .as_any()
            .is::<EffectNode>(),
        "wrapper must be EffectNode"
    );
    assert_eq!(
        d.scene.get_node(target).unwrap().parent,
        Some(wrapper),
        "target becomes child of effect wrapper"
    );
    assert_eq!(
        d.scene.get_node(parent).unwrap().children,
        vec![wrapper],
        "parent edge rewrites to wrapper"
    );
    assert_eq!(
        d.tracks[0].clips[0].scene_root,
        wrapper,
        "scene_root rewrites to wrapper"
    );

    // apply_effect_to_node always wraps (honest rewrite). Attach-in-place is
    // the Rhai push_or_wrap_effect path when the target is already EffectNode.
    let outer2 =
        apply_effect_to_node(&mut d, wrapper, EffectType::Blur(Animated::new(2.0)));
    assert_ne!(outer2, wrapper);
    assert!(d.scene.get_node(wrapper).is_some());
}

#[test]
fn apply_effect_rhai_returns_wrapper_handle() {
    let mut engine = Engine::new();
    scripting::register_rhai_api(&mut engine, Arc::new(DefaultAssetLoader));

    let script = r#"
        let movie = new_director(320, 240, 30);
        let scene = movie.add_scene(1.0);
        let box = scene.add_box(#{ width: 50.0, height: 50.0 });
        let before = node_id(box);
        let wrapped = box.apply_effect("grayscale");
        let after = node_id(wrapped);
        // Old handle still addresses the inner content node.
        let old_still = node_id(box);
        #{
            before: before,
            after: after,
            old_still: old_still,
            same_handle: before == after,
            old_equals_before: old_still == before,
        }
    "#;

    let map = engine
        .eval::<rhai::Map>(script)
        .expect("apply_effect script must evaluate");
    let before = map.get("before").unwrap().as_int().unwrap();
    let after = map.get("after").unwrap().as_int().unwrap();
    let old_still = map.get("old_still").unwrap().as_int().unwrap();
    let same_handle = map.get("same_handle").unwrap().as_bool().unwrap();
    let _old_equals_before = map.get("old_equals_before").unwrap().as_bool().unwrap();

    assert!(!same_handle, "returned handle must be the wrapper");
    assert_eq!(old_still, before, "caller's original handle keeps inner id");
    assert_ne!(after, before);
}

#[test]
fn set_mask_preserves_mask_id() {
    let mut d = empty_director();
    let owner = d.scene.add_node(Box::new(BoxNode::new()));
    let mask = d.scene.add_node(Box::new(BoxNode::new()));
    let other = d.scene.add_node(Box::new(BoxNode::new()));
    assert!(d.scene.try_add_child(other, mask));

    // Mirror Rhai set_mask ownership: detach from children list, exclusive parent.
    let old_parent = d.scene.get_node(mask).and_then(|n| n.parent);
    if let Some(p) = old_parent {
        d.scene.remove_child(p, mask);
    }
    if let Some(m) = d.scene.get_node_mut(mask) {
        m.parent = Some(owner);
    }
    if let Some(n) = d.scene.get_node_mut(owner) {
        n.mask_node = Some(mask);
    }

    assert_eq!(d.scene.get_node(owner).unwrap().mask_node, Some(mask));
    assert!(
        d.scene.get_node(mask).is_some(),
        "mask NodeId still resolves"
    );
    assert_eq!(d.scene.get_node(mask).unwrap().parent, Some(owner));
    assert!(
        !d.scene
            .get_node(other)
            .unwrap()
            .children
            .contains(&mask),
        "mask removed from previous parent's children"
    );

    // Destroy owner must destroy exclusive mask (generational stale).
    d.scene.destroy_node(owner);
    assert!(d.scene.get_node(owner).is_none());
    assert!(
        d.scene.get_node(mask).is_none(),
        "exclusive mask destroyed with owner"
    );
}

#[test]
fn reparent_keeps_id() {
    let mut d = empty_director();
    let p1 = d.scene.add_node(Box::new(BoxNode::new()));
    let p2 = d.scene.add_node(Box::new(BoxNode::new()));
    let child = d.scene.add_node(Box::new(BoxNode::new()));
    assert!(d.scene.try_add_child(p1, child));
    let before = child;
    assert!(d.scene.try_add_child(p2, child));
    assert_eq!(before, child);
    assert_eq!(d.scene.get_node(child).unwrap().parent, Some(p2));
}

#[test]
fn nested_comp_id_isolation() {
    let mut outer = empty_director();
    let mut inner = empty_director();
    let inner_root = inner.scene.add_node(Box::new(BoxNode::new()));
    inner.push_scene_root(inner_root, 1.0);

    // Both arenas start at generation 0 index 0 — same NodeId bit pattern,
    // different directors.
    assert_eq!(inner_root, NodeId::at(0));

    let comp = CompositionNode::new(inner);
    let comp_id = outer.scene.add_node(Box::new(comp));
    outer.push_scene_root(comp_id, 2.0);

    // Outer arena has only the composition node at index 0. Fetching
    // `inner_root` on the outer director either misses or hits a different
    // element (the CompositionNode), never the inner box.
    match outer.scene.get_node(inner_root) {
        None => {}
        Some(n) => {
            assert!(
                n.element.as_any().is::<CompositionNode>(),
                "outer slot must not be the inner box"
            );
        }
    }

    // Inner id resolves only on the nested director.
    {
        let nested = outer
            .scene
            .get_node(comp_id)
            .unwrap()
            .element
            .as_any()
            .downcast_ref::<CompositionNode>()
            .unwrap();
        let nested_dir = nested.internal_director.lock().unwrap();
        assert!(nested_dir.scene.get_node(inner_root).is_some());
        assert!(
            nested_dir
                .scene
                .get_node(inner_root)
                .unwrap()
                .element
                .as_any()
                .is::<BoxNode>()
        );
    }

    // Time offset: composition feeds local_time - start_offset into inner update.
    // Inner clip is [0, 1.0); sample mid-clip so the root is active.
    if let Some(node) = outer.scene.get_node_mut(comp_id) {
        let c = node
            .element
            .as_any_mut()
            .downcast_mut::<CompositionNode>()
            .unwrap();
        c.start_offset = 0.25;
        c.update(0.75); // drives inner at t=0.5
        let nested_dir = c.internal_director.lock().unwrap();
        let visited = nested_dir
            .scene
            .get_node(inner_root)
            .map(|n| n.local_time)
            .unwrap_or(-1.0);
        assert!(
            (visited - 0.5).abs() < 1e-6,
            "inner local_time should be outer_time - start_offset, got {visited}"
        );
    }
}

#[test]
fn invalidate_after_destroy_drops_stale_ids() {
    let mut d = empty_director();
    let a = d.scene.add_node(Box::new(BoxNode::new()));
    let b = d.scene.add_node(Box::new(BoxNode::new()));
    assert!(d.scene.try_add_child(a, b));
    d.compile();

    d.dirty_nodes.insert(a);
    d.dirty_nodes.insert(b);
    d.scene.destroy_node(a); // also destroys b

    // Stale ids must not resolve; retain only live.
    d.dirty_nodes.retain(|id| d.scene.get_node(*id).is_some());
    assert!(d.dirty_nodes.is_empty());

    // Fresh node at recycled slot must not match old dirty membership.
    let fresh = d.scene.add_node(Box::new(BoxNode::new()));
    assert_eq!(fresh.index, a.index);
    assert!(!d.dirty_nodes.contains(&a));
    assert!(!d.dirty_nodes.contains(&fresh));

    // Invalidating with a stale id is a no-op for get_node path.
    d.compile();
    let affected = d.invalidate(vec![DependencyInput::Node(a)]);
    assert!(
        !affected.contains(&a),
        "stale id must not appear in dirty set"
    );
    assert!(affected.iter().all(|id| d.scene.get_node(*id).is_some()));
}

#[test]
fn rhai_stale_handle_method_errors() {
    let mut engine = Engine::new();
    scripting::register_rhai_api(&mut engine, Arc::new(DefaultAssetLoader));

    // Destroy via Rust after capturing a handle is hard from pure Rhai;
    // exercise ensure_alive through a property setter after destroy in Rust.
    let movie: MovieHandle = engine
        .eval(
            r#"
            let movie = new_director(320, 240, 30);
            let scene = movie.add_scene(1.0);
            let box = scene.add_box(#{ width: 10.0, height: 10.0 });
            movie
            "#,
        )
        .expect("setup");

    let handle = {
        let d = movie.director.lock().unwrap();
        let id = d.scene.iter_alive().map(|(id, _)| id).last().unwrap();
        NodeHandle {
            director: movie.director.clone(),
            id,
        }
    };
    {
        let mut d = movie.director.lock().unwrap();
        d.scene.destroy_node(handle.id);
    }

    // Calling a mutating method path: lock + ensure_alive inside set_z_index.
    engine.register_fn(
        "poke_stale",
        |n: &mut NodeHandle| -> Result<(), Box<rhai::EvalAltResult>> {
            let d = n.lock_director()?;
            n.ensure_alive(&d)
        },
    );
    // Inject handle into engine scope.
    let mut scope = rhai::Scope::new();
    scope.push("n", handle);
    let err = engine
        .eval_with_scope::<()>(&mut scope, "poke_stale(n)")
        .unwrap_err()
        .to_string();
    assert!(err.contains("stale"), "got: {err}");
}
