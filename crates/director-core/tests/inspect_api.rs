//! Runtime test for the agent-facing introspection API (finding #1).
//! Exercises `node_id`, `node_bounds`, `node_children`, `scene_duration`,
//! `list_scene_roots`, and the capability-registry reflection functions
//! from a Rhai script to prove they are wired and produce sane results.

use director_core::scripting::register_rhai_api;
use director_core::DefaultAssetLoader;
use rhai::{Array, Engine, Map};
use std::sync::Arc;

#[test]
fn inspect_api_works() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(640, 360, 30);
        let scene = movie.add_scene(2.5);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let inner = root.add_text(#{ content: "hi", size: 24.0 });

        let out = #{
            node_id_root:        node_id(root),
            node_id_inner:       node_id(inner),
            bounds_root:         node_bounds(root),
            bounds_inner:        node_bounds(inner),
            children_root:       node_children(root),
            children_inner:      node_children(inner),
            dur:                 scene_duration(scene),
            scene_roots:         list_scene_roots(movie),
            effects:             list_effects(),
            easings:             list_easings(),
            transitions:         list_transitions(),
            supports_box_op:     supports_animation("Box", "opacity"),
            supports_box_xyz:    supports_animation("Box", "no_such_prop"),
            anim_props_box:      get_animatable_properties("Box"),
            effect_meta:         get_effect_meta("blur"),
            effect_meta_missing: get_effect_meta("not_a_real_effect"),
        };
        out
    "#;

    let result: Map = engine
        .eval::<Map>(script)
        .expect("introspection script must evaluate");

    // node_id returns a non-negative i64, root and inner distinct.
    let id_root: i64 = result
        .get("node_id_root")
        .and_then(|v| v.as_int().ok())
        .expect("node_id_root present");
    let id_inner: i64 = result
        .get("node_id_inner")
        .and_then(|v| v.as_int().ok())
        .expect("node_id_inner present");
    assert!(id_root >= 0 && id_inner >= 0, "node ids must be non-negative");
    assert_ne!(id_root, id_inner, "root and inner must have distinct ids");

    // node_bounds returns a Map with the four keys.
    let bounds: Map = result
        .get("bounds_root")
        .and_then(|v| v.clone().try_cast::<Map>())
        .expect("bounds_root must be a map");
    for k in ["x", "y", "w", "h"] {
        assert!(
            bounds.get(k).is_some(),
            "node_bounds must include key '{}'",
            k
        );
    }

    // node_children returns an Array; root has >=1 child (the text).
    let children: Array = result
        .get("children_root")
        .and_then(|v| v.clone().try_cast::<Array>())
        .expect("children_root must be an array");
    assert!(
        children.len() >= 1,
        "root should have at least 1 child (the text node)"
    );
    let inner_children: Array = result
        .get("children_inner")
        .and_then(|v| v.clone().try_cast::<Array>())
        .expect("children_inner must be an array");
    let _ = inner_children;

    // scene_duration is positive.
    let dur: f64 = result
        .get("dur")
        .and_then(|v| v.as_float().ok())
        .expect("dur present");
    assert!(dur > 0.0, "scene_duration must be positive, got {}", dur);

    // list_scene_roots returns a non-empty Array.
    let roots: Array = result
        .get("scene_roots")
        .and_then(|v| v.clone().try_cast::<Array>())
        .expect("scene_roots must be an array");
    assert!(!roots.is_empty(), "expected at least one scene root");

    // Registry JSON strings are non-empty.
    for key in ["effects", "easings", "transitions"] {
        let s = result
            .get(key)
            .and_then(|v| v.clone().into_string().ok())
            .unwrap_or_default();
        assert!(!s.is_empty(), "{} JSON must be non-empty", key);
    }

    // supports_animation correctness.
    let sup_op: bool = result
        .get("supports_box_op")
        .and_then(|v| v.as_bool().ok())
        .expect("supports_box_op present");
    assert!(sup_op, "box should support 'opacity' animation");
    let sup_xyz: bool = result
        .get("supports_box_xyz")
        .and_then(|v| v.as_bool().ok())
        .expect("supports_box_xyz present");
    assert!(!sup_xyz, "box should NOT support 'no_such_prop'");

    // get_animatable_properties returns a non-empty Array.
    let anim: Array = result
        .get("anim_props_box")
        .and_then(|v| v.clone().try_cast::<Array>())
        .expect("anim_props_box must be an array");
    assert!(
        !anim.is_empty(),
        "box should have at least one animatable property"
    );

    // get_effect_meta: known → non-empty JSON, missing → empty string.
    let meta = result
        .get("effect_meta")
        .and_then(|v| v.clone().into_string().ok())
        .unwrap_or_default();
    assert!(!meta.is_empty(), "blur effect meta must be non-empty JSON");
    let meta_missing = result
        .get("effect_meta_missing")
        .and_then(|v| v.clone().into_string().ok())
        .unwrap_or_default();
    assert!(
        meta_missing.is_empty(),
        "missing effect must return empty string"
    );
}
