//! Round-trip test for Critical blocker #2: versioned project save/load.
//!
//! Builds a Director (with Box nodes + tracks) via the Rhai API, saves it
//! to a versioned project JSON, loads it back, and asserts that the
//! graph topology, node kinds, transforms, opacity, and tracks survive.

use director_core::element::Element;
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

/// Verifies that a SmartCaptionNode round-trips with its caption content,
/// semantic manifest, and trajectory instead of falling back to a BoxNode
/// placeholder.
#[test]
fn project_roundtrip_preserves_smart_caption() {
    use director_core::entity::{BoundingBox, EntityKind};
    use director_core::node::caption::{CaptionLine, CaptionNode, CaptionWord};
    use director_core::node::smart_caption::{SmartCaptionConfig, SmartCaptionNode};
    use director_core::semantic::{SemanticManifest, SolverConfig};
    use std::sync::{Arc, Mutex};

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(r##"new_director(1080, 1920, 30)"##)
        .expect("build script must evaluate");

    // Build a SmartCaptionNode directly.
    let line = CaptionLine {
        text: "hello world".into(),
        words: vec![
            CaptionWord {
                text: "hello".into(),
                start_sec: 0.0,
                end_sec: 1.0,
                display_line_idx: 0,
            },
            CaptionWord {
                text: "world".into(),
                start_sec: 1.0,
                end_sec: 2.0,
                display_line_idx: 0,
            },
        ],
        start_sec: 0.0,
        end_sec: 2.0,
    };
    let inner = CaptionNode::new(vec![line], Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new())));

    let mut registry = director_core::entity::EntityRegistry::new();
    let face = registry.create(EntityKind::Face, "mara".to_string());
    registry.add_sample(face, 0.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);
    registry.add_sample(face, 2.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);

    let manifest = SemanticManifest {
        entity_refs: vec![face],
        start_time: 0.0,
        end_time: 2.0,
        solver: SolverConfig::default(),
    };

    let config = SmartCaptionConfig::default();
    let mut smart = SmartCaptionNode::from_manifest(inner, &registry, &manifest, config.clone());
    smart.inner.style.font_size = 64.0;
    smart.inner.style.position = director_core::node::caption::CaptionPosition::Top;
    let original_trajectory_len = smart.trajectory.len();
    assert!(original_trajectory_len > 0);
    let original_sample = smart.trajectory.sample_at(0.5).expect("sample at 0.5");

    {
        let mut d = movie.director.lock().unwrap();
        d.entities = registry;
        let _node_id = d.scene.add_node(Box::new(smart));
    }

    // Save + load.
    let tmp = std::env::temp_dir().join("director_project_smart_caption_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }
    let loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    // Find the loaded SmartCaptionNode.
    let mut found = false;
    for sn in loaded.scene.nodes.iter().flatten() {
        if let Some(smart) = sn.element.as_any().downcast_ref::<SmartCaptionNode>() {
            found = true;
            assert_eq!(smart.inner.lines.len(), 1);
            assert_eq!(smart.inner.lines[0].text, "hello world");
            assert_eq!(smart.trajectory.len(), original_trajectory_len);
            assert!(smart.manifest.is_some());
            assert_eq!(smart.manifest.as_ref().unwrap().entity_refs.len(), 1);
            assert!(smart.config.is_some());
            assert_eq!(smart.config.as_ref().unwrap().canvas_w, config.canvas_w);
            assert_eq!(smart.config.as_ref().unwrap().canvas_h, config.canvas_h);
            assert!(!smart.is_continuous());
            assert!((smart.inner.style.font_size - 64.0).abs() < 1e-3);
            assert_eq!(smart.inner.style.position, director_core::node::caption::CaptionPosition::Top);
            let loaded_sample = smart.trajectory.sample_at(0.5).expect("loaded sample at 0.5");
            assert!((loaded_sample.0 - original_sample.0).abs() < 1e-3);
            assert!((loaded_sample.1 - original_sample.1).abs() < 1e-3);
        }
    }
    assert!(found, "loaded project must contain a SmartCaptionNode");

    let _ = std::fs::remove_file(&tmp);
}

/// Verifies that a continuous SmartCaptionNode round-trips with its
/// trajectory and `continuous` flag.
#[test]
fn project_roundtrip_preserves_smart_caption_continuous() {
    use director_core::entity::{BoundingBox, EntityKind};
    use director_core::node::caption::{CaptionLine, CaptionNode, CaptionWord};
    use director_core::node::caption::CaptionPosition;
    use director_core::node::smart_caption::{SmartCaptionConfig, SmartCaptionNode};
    use director_core::semantic::{SemanticManifest, SolverConfig};
    use std::sync::{Arc, Mutex};

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(r##"new_director(1080, 1920, 30)"##)
        .expect("build script must evaluate");

    let line = CaptionLine {
        text: "continuous".into(),
        words: vec![CaptionWord {
            text: "continuous".into(),
            start_sec: 0.0,
            end_sec: 2.0,
            display_line_idx: 0,
        }],
        start_sec: 0.0,
        end_sec: 2.0,
    };
    let inner = CaptionNode::new(vec![line], Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new())));

    let mut registry = director_core::entity::EntityRegistry::new();
    let face = registry.create(EntityKind::Face, "mara".to_string());
    registry.add_sample(face, 0.0, BoundingBox::new(820.0, 1500.0, 200.0, 200.0), 1.0);
    registry.add_sample(face, 2.0, BoundingBox::new(820.0, 1500.0, 200.0, 200.0), 1.0);

    let manifest = SemanticManifest {
        entity_refs: vec![face],
        start_time: 0.0,
        end_time: 2.0,
        solver: SolverConfig::default(),
    };

    let config = SmartCaptionConfig::default();
    let smart = SmartCaptionNode::from_manifest_continuous(inner, &registry, &manifest, config);
    let original_sample = smart.trajectory.sample_at(0.5).expect("sample at 0.5");

    {
        let mut d = movie.director.lock().unwrap();
        d.entities = registry;
        let _node_id = d.scene.add_node(Box::new(smart));
    }

    let tmp = std::env::temp_dir().join("director_project_smart_caption_continuous_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }
    let loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    let mut found = false;
    for sn in loaded.scene.nodes.iter().flatten() {
        if let Some(smart) = sn.element.as_any().downcast_ref::<SmartCaptionNode>() {
            found = true;
            assert!(smart.is_continuous());
            let loaded_sample = smart.trajectory.sample_at(0.5).expect("loaded sample at 0.5");
            assert!((loaded_sample.0 - original_sample.0).abs() < 1e-3);
            assert!((loaded_sample.1 - original_sample.1).abs() < 1e-3);
            // Position is updated lazily by apply_trajectory; drive it once
            // to verify the continuous trajectory maps to FreeXY.
            let mut smart = smart.clone();
            smart.update(0.5);
            assert!(matches!(smart.inner.style.position, CaptionPosition::FreeXY { .. }));
        }
    }
    assert!(found, "loaded project must contain a SmartCaptionNode");

    let _ = std::fs::remove_file(&tmp);
}

/// Verifies that semantic entity tracks and node entity_refs survive a
/// save/load round-trip without remapping.
#[test]
fn project_roundtrip_preserves_entities() {
    use director_core::entity::EntityKind;

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let build = r##"
        let movie = new_director(320, 240, 30);
        let scene = movie.add_scene(2.0);
        let bg = scene.add_box(#{ width: 320, height: 240, bg_color: "#101018" });
        let face_id = movie.create_entity("face", "mara");
        movie.add_entity_sample(face_id, 0.0, 10.0, 10.0, 50.0, 50.0, 1.0);
        movie.add_entity_sample(face_id, 1.0, 20.0, 10.0, 50.0, 50.0, 1.0);
        bg.attach_entity(face_id);
        movie
    "##;

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(build)
        .expect("build script must evaluate");

    let tmp = std::env::temp_dir().join("director_project_entities_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }

    let loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    // Entity tracks preserved.
    assert_eq!(loaded.entities.len(), 1);
    let track = loaded.entities.iter().next().expect("entity track");
    assert_eq!(track.kind, EntityKind::Face);
    assert_eq!(track.label, "mara");
    assert_eq!(track.samples.len(), 2);

    // Node entity_refs preserved.
    let bg_id = find_box_with_bg(&loaded, "#101018").expect("bg box");
    let sn = loaded.scene.get_node(bg_id).expect("bg node");
    assert_eq!(sn.entity_refs.len(), 1);
    assert_eq!(sn.entity_refs[0], track.id);

    // Sampling works after round-trip.
    let sample = loaded.entities.sample_at(track.id, 0.5).expect("sample");
    assert!((sample.bbox.x - 15.0).abs() < 0.01);

    let _ = std::fs::remove_file(&tmp);
}
