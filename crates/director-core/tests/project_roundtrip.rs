//! Round-trip test for Critical blocker #2: versioned project save/load.
//!
//! Builds a Director (with Box nodes + tracks) via the Rhai API, saves it
//! to a versioned project JSON, loads it back, and asserts that the
//! graph topology, node kinds, transforms, opacity, and tracks survive.

use director_core::node::BoxNode;
use director_core::scripting::register_rhai_api;
use director_core::scripting::utils::style_to_json;
use director_core::DefaultAssetLoader;
use rhai::Engine;
use std::sync::Arc;

#[test]
fn project_roundtrip() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    // Build a small project: a 320x240 director with one scene containing a
    // Box (full-screen black background) with a child Box (red, centered).
    let build = r##"
        let movie = new_director(320, 240, 30);
        let scene = movie.add_scene(2.0);
        let bg = scene.add_box(#{ width: 320, height: 240, bg_color: "#101018" });
        let card = bg.add_box(#{ width: 120, height: 80, bg_color: "#ff0066" });
        movie
    "##;

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(build)
        .expect("build script must evaluate");

    // Save the project to a temp file.
    let tmp = std::env::temp_dir().join("director_project_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }
    assert!(tmp.exists(), "project file must be written");

    // Load it back into a fresh Director.
    let loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    // Settings preserved.
    assert_eq!(loaded.width, 320);
    assert_eq!(loaded.height, 240);
    assert_eq!(loaded.fps, 30);

    // Scene graph topology preserved.
    // The build creates a root (from add_scene) + bg (child of root) + card
    // (child of bg) = at least 3 nodes. We assert the two Box children are
    // present and the loaded graph has the expected structure.
    let mut total = 0usize;
    let mut box_kinds = 0usize;
    for sn in loaded.scene.nodes.iter().flatten() {
        total += 1;
        if sn.element.as_any().is::<director_core::node::BoxNode>() {
            box_kinds += 1;
        }
    }
    assert!(total >= 2, "at least 2 nodes must be loaded, got {}", total);
    assert!(box_kinds >= 2, "at least 2 Box nodes must be loaded");

    // Tracks: the legacy_timeline is preserved (add_scene pushes to timeline).
    assert!(
        !loaded.timeline.is_empty(),
        "at least one legacy timeline item must be loaded"
    );

    // The saved file is valid JSON with the version field.
    let raw = std::fs::read_to_string(&tmp).expect("read saved file");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    assert_eq!(v["version"], 1, "version must be 1");
    assert_eq!(v["width"], 320);
    assert_eq!(v["height"], 240);
    assert_eq!(v["fps"], 30.0);
    let nodes = v["nodes"].as_array().expect("nodes array");
    assert!(nodes.len() >= 2, "at least 2 nodes in JSON, got {}", nodes.len());
    // Every node has a kind string.
    for n in nodes {
        assert!(n["kind"].is_string(), "node.kind must be a string");
    }
    // At least one Box node has bg_color preserved in props.
    let has_bg = nodes.iter().any(|n| {
        n["kind"] == "Box"
            && n["props"]["bg_color"].is_string()
    });
    assert!(has_bg, "at least one Box node must preserve bg_color");
    let tracks = v["tracks"].as_array().expect("tracks array");
    let _ = tracks;

    // Clean up the temp file.
    let _ = std::fs::remove_file(&tmp);
}

/// Verifies that keyframe animation (not just the current value) survives a
/// save/load round-trip: after `update(t)`, an animated box's transform and
/// opacity must match the original director's values at the same time.
#[test]
fn project_roundtrip_preserves_animation() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let build = r##"
        let movie = new_director(320, 240, 30);
        let scene = movie.add_scene(3.0);
        let bg = scene.add_box(#{ width: 320, height: 240, bg_color: "#101018" });
        let card = bg.add_box(#{ width: 120, height: 80, bg_color: "#ff0066" });
        // Animate the card's x position (0 -> 200 over 1s) and opacity (1 -> 0.2 over 2s).
        card.animate("x", 0.0, 200.0, 1.0, "ease_in_out");
        card.animate("opacity", 1.0, 0.2, 2.0, "ease_in_out");
        movie
    "##;

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(build)
        .expect("build script must evaluate");

    // Find the card node id (the child box of the scene root with bg_color #ff0066).
    let card_id = {
        let d = movie.director.lock().unwrap();
        find_box_with_bg(&d, "#ff0066")
    };
    let card_id = card_id.expect("card box must exist");

    // Save + load.
    let tmp = std::env::temp_dir().join("director_project_anim_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }
    let loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    // Evaluate both directors at t = 1.5s and compare the card's animated values.
    let t = 1.5;
    let (orig_x, orig_op) = {
        let mut d = movie.director.lock().unwrap();
        d.update(t);
        let sn = d.scene.get_node(card_id).expect("card node");
        (
            sn.transform.translate_x.current_value,
            sn_opacity(&d, card_id),
        )
    };
    let loaded_x_mid;
    let loaded_x;
    let loaded_op;
    {
        let mut d = loaded;
        let t_mid = 0.25;
        d.update(t_mid);
        let lid = find_box_with_bg(&d, "#ff0066").expect("loaded card box must exist");
        loaded_x_mid = d
            .scene
            .get_node(lid)
            .expect("loaded card node")
            .transform
            .translate_x
            .current_value;
        d.update(t);
        let lid = find_box_with_bg(&d, "#ff0066").expect("loaded card box must exist");
        let sn = d.scene.get_node(lid).expect("loaded card node");
        loaded_x = sn.transform.translate_x.current_value;
        loaded_op = sn_opacity(&d, lid);
    }

    // Easing-sensitive mid-point: eased (EaseInOut) and linear diverge here,
    // so this catches any loss of segment easing on reload.
    let t_mid = 0.25;
    let orig_x_mid = {
        let mut d = movie.director.lock().unwrap();
        d.update(t_mid);
        d.scene
            .get_node(card_id)
            .expect("card node")
            .transform
            .translate_x
            .current_value
    };
    assert!(
        (orig_x_mid - loaded_x_mid).abs() < 1e-3,
        "translate_x must round-trip at t={t_mid}: original {orig_x_mid} vs loaded {loaded_x_mid}"
    );
    assert!(
        (orig_x - loaded_x).abs() < 1e-3,
        "translate_x must round-trip: original {orig_x} vs loaded {loaded_x} at t={t}"
    );
    assert!(
        (orig_op - loaded_op).abs() < 1e-3,
        "opacity must round-trip: original {orig_op} vs loaded {loaded_op} at t={t}"
    );

    let _ = std::fs::remove_file(&tmp);
}

fn find_box_with_bg(d: &director_core::Director, hex: &str) -> Option<director_core::types::NodeId> {
    let target = hex.to_ascii_lowercase();
    for (i, sn_opt) in d.scene.nodes.iter().enumerate() {
        if let Some(sn) = sn_opt {
            if let Some(b) = sn.element.as_any().downcast_ref::<director_core::node::BoxNode>() {
                if let Some(c) = &b.bg_color {
                    let got = format!(
                        "#{:02x}{:02x}{:02x}",
                        (c.current_value.r * 255.0) as u8,
                        (c.current_value.g * 255.0) as u8,
                        (c.current_value.b * 255.0) as u8
                    );
                    if got == target {
                        return Some(i as director_core::types::NodeId);
                    }
                }
            }
        }
    }
    None
}

fn sn_opacity(d: &director_core::Director, id: director_core::types::NodeId) -> f32 {
    let sn = d.scene.get_node(id).expect("node");
    sn.element
        .as_any()
        .downcast_ref::<director_core::node::BoxNode>()
        .map(|b| b.opacity.current_value)
        .unwrap_or(1.0)
}

/// Verifies that Taffy layout (`Style`) survives a save/load round-trip: the
/// loaded node's full layout JSON equals the original node's layout JSON.
#[test]
fn project_roundtrip_preserves_layout() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let build = r##"
        let movie = new_director(320, 240, 30);
        let scene = movie.add_scene(2.0);
        let bg = scene.add_box(#{
            width: 320, height: 240,
            position: "absolute", left: 10, top: 20, right: 10, bottom: 20,
            padding: 8, margin: 4, gap: 6,
            flex_direction: "row", align_items: "flex_start",
            justify_content: "space_between",
            flex_grow: 1.0, aspect_ratio: 1.5
        });
        movie
    "##;

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(build)
        .expect("build script must evaluate");

    // Capture the original layout JSON for the absolutely-positioned box.
    let orig_layout = {
        let d = movie.director.lock().unwrap();
        let id = find_abs_box(&d).expect("absolute box must exist");
        let sn = d.scene.get_node(id).expect("node");
        let b = sn
            .element
            .as_any()
            .downcast_ref::<BoxNode>()
            .expect("box node");
        style_to_json(&b.style)
    };

    // Save + load.
    let tmp = std::env::temp_dir().join("director_project_layout_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }
    let loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    // The loaded graph must contain an absolutely-positioned box with the
    // exact same layout.
    let loaded_layout = {
        let id = find_abs_box(&loaded).expect("loaded absolute box must exist");
        let sn = loaded.scene.get_node(id).expect("node");
        let b = sn
            .element
            .as_any()
            .downcast_ref::<BoxNode>()
            .expect("loaded box node");
        style_to_json(&b.style)
    };

    assert_eq!(
        orig_layout, loaded_layout,
        "Taffy layout must round-trip exactly"
    );

    // Spot-check a couple of fields directly for clarity on failure.
    assert_eq!(loaded_layout.get("position"), Some(&serde_json::json!("absolute")));
    assert_eq!(loaded_layout.get("left"), Some(&serde_json::json!(10.0)));
    assert_eq!(loaded_layout.get("aspect_ratio"), Some(&serde_json::json!(1.5)));
    assert_eq!(loaded_layout.get("justify_content"), Some(&serde_json::json!("space_between")));

    let _ = std::fs::remove_file(&tmp);
}

fn find_abs_box(d: &director_core::Director) -> Option<director_core::types::NodeId> {
    for (i, sn_opt) in d.scene.nodes.iter().enumerate() {
        if let Some(sn) = sn_opt {
            if let Some(b) = sn.element.as_any().downcast_ref::<BoxNode>() {
                if matches!(b.style.position, taffy::style::Position::Absolute) {
                    return Some(i as director_core::types::NodeId);
                }
            }
        }
    }
    None
}
