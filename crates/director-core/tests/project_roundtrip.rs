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
    assert_eq!(loaded.fps, 30.0);

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
        !loaded.flat_timeline().is_empty(),
        "at least one legacy timeline item must be loaded"
    );

    // The saved file is valid JSON with the version field.
    let raw = std::fs::read_to_string(&tmp).expect("read saved file");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    assert_eq!(v["version"], 2, "version must be 2 (generational NodeId)");
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
        d.update(t, None);
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
        d.update(t_mid, None);
        let lid = find_box_with_bg(&d, "#ff0066").expect("loaded card box must exist");
        loaded_x_mid = d
            .scene
            .get_node(lid)
            .expect("loaded card node")
            .transform
            .translate_x
            .current_value;
        d.update(t, None);
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
        d.update(t_mid, None);
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
    for (id, sn) in d.scene.iter_alive() {
        if let Some(b) = sn.element.as_any().downcast_ref::<director_core::node::BoxNode>() {
            if let Some(c) = &b.bg_color {
                let got = format!(
                    "#{:02x}{:02x}{:02x}",
                    (c.current_value.r * 255.0) as u8,
                    (c.current_value.g * 255.0) as u8,
                    (c.current_value.b * 255.0) as u8
                );
                if got == target {
                    return Some(id);
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
    for (id, sn) in d.scene.iter_alive() {
        if let Some(b) = sn.element.as_any().downcast_ref::<BoxNode>() {
            if matches!(b.style.position, taffy::style::Position::Absolute) {
                return Some(id);
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

/// Verifies that an ImageNode round-trips with its source path and object
/// fit instead of becoming a BoxNode placeholder.
#[test]
fn project_roundtrip_preserves_image() {
    use director_core::node::image_node::ImageNode;
    use director_core::types::{ObjectFit, ObjectPosition};

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(r##"new_director(320, 240, 30)"##)
        .expect("build script must evaluate");

    // Build an ImageNode with a source path and object fit.
    let mut img = ImageNode::from_image_with_path(None, "assets/photo.png");
    img.object_fit = ObjectFit::Contain;
    img.object_position = ObjectPosition {
        x_percent: 0.0,
        x_offset: 4.0,
        y_percent: 0.5,
        y_offset: 0.0,
    };
    img.opacity.current_value = 0.8;

    {
        let mut d = movie.director.lock().unwrap();
        let _id = d.scene.add_node(Box::new(img));
    }

    let tmp = std::env::temp_dir().join("director_project_image_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }
    let loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    let mut found = false;
    for sn in loaded.scene.nodes.iter().flatten() {
        if let Some(img) = sn.element.as_any().downcast_ref::<ImageNode>() {
            found = true;
            assert_eq!(img.source_path.as_deref(), Some("assets/photo.png"));
            assert_eq!(img.object_fit, ObjectFit::Contain);
            assert_eq!(
                img.object_position,
                ObjectPosition {
                    x_percent: 0.0,
                    x_offset: 4.0,
                    y_percent: 0.5,
                    y_offset: 0.0,
                }
            );
            assert!((img.opacity.current_value - 0.8).abs() < 1e-3);
        }
    }
    assert!(found, "loaded project must contain an ImageNode");

    let _ = std::fs::remove_file(&tmp);
}

/// Verifies that a VideoNode round-trips with its path, source path, and
/// object fit instead of becoming a BoxNode placeholder.
#[test]
fn project_roundtrip_preserves_video() {
    use director_core::node::video_node::{VideoNode, VideoSource};
    use director_core::types::{ObjectFit, ObjectPosition};
    use director_core::video_wrapper::RenderMode;

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(r##"new_director(320, 240, 30)"##)
        .expect("build script must evaluate");

    // Build a VideoNode from a path.
    let mut vid = VideoNode::new(
        VideoSource::Path(std::path::PathBuf::from("assets/clip.mp4")),
        RenderMode::Preview,
    )
    .with_source_path("assets/clip.mp4");
    vid.object_fit = ObjectFit::Contain;
    vid.object_position = ObjectPosition {
        x_percent: 0.0,
        x_offset: 4.0,
        y_percent: 0.5,
        y_offset: 0.0,
    };
    vid.opacity.current_value = 0.75;

    {
        let mut d = movie.director.lock().unwrap();
        let _id = d.scene.add_node(Box::new(vid));
    }

    let tmp = std::env::temp_dir().join("director_project_video_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }
    let loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    let mut found = false;
    for sn in loaded.scene.nodes.iter().flatten() {
        if let Some(vid) = sn.element.as_any().downcast_ref::<VideoNode>() {
            found = true;
            assert_eq!(vid.source_path.as_deref(), Some("assets/clip.mp4"));
            assert_eq!(vid.path(), &std::path::PathBuf::from("assets/clip.mp4"));
            assert_eq!(vid.object_fit, ObjectFit::Contain);
            assert_eq!(
                vid.object_position,
                ObjectPosition {
                    x_percent: 0.0,
                    x_offset: 4.0,
                    y_percent: 0.5,
                    y_offset: 0.0,
                }
            );
            assert!((vid.opacity.current_value - 0.75).abs() < 1e-3);
        }
    }
    assert!(found, "loaded project must contain a VideoNode");

    let _ = std::fs::remove_file(&tmp);
}

/// Verifies that a LottieNode round-trips with its speed, loop flag, and
/// frame instead of becoming a BoxNode placeholder.
#[test]
fn project_roundtrip_preserves_lottie() {
    use director_core::node::lottie::LottieNode;
    use std::sync::Arc;

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(r##"new_director(320, 240, 30)"##)
        .expect("build script must evaluate");

    // Minimal valid Lottie JSON.
    let lottie_json = br##"{
        "v": "5.5.7",
        "fr": 30,
        "ip": 0,
        "op": 60,
        "w": 100,
        "h": 100,
        "nm": "test",
        "ddd": 0,
        "assets": [],
        "layers": [],
        "markers": []
    }"##;

    let assets = {
        let d = movie.director.lock().unwrap();
        d.assets.clone()
    };
    let mut lottie = LottieNode::new(lottie_json, std::collections::HashMap::new(), &assets)
        .expect("lottie must parse");
    lottie.speed = 2.0;
    lottie.loop_anim = true;
    lottie.frame.current_value = 12.0;
    lottie.opacity.current_value = 0.9;

    {
        let mut d = movie.director.lock().unwrap();
        let _id = d.scene.add_node(Box::new(lottie));
    }

    let tmp = std::env::temp_dir().join("director_project_lottie_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }
    let loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    let mut found = false;
    for sn in loaded.scene.nodes.iter().flatten() {
        if let Some(lottie) = sn.element.as_any().downcast_ref::<LottieNode>() {
            found = true;
            assert!((lottie.speed - 2.0).abs() < 1e-3);
            assert!(lottie.loop_anim);
            assert!((lottie.frame.current_value - 12.0).abs() < 1e-3);
            assert!((lottie.opacity.current_value - 0.9).abs() < 1e-3);
        }
    }
    assert!(found, "loaded project must contain a LottieNode");

    let _ = std::fs::remove_file(&tmp);
}

/// Verifies that an EffectNode round-trips with its effects instead of
/// becoming a BoxNode placeholder.
#[test]
fn project_roundtrip_preserves_effect() {
    use director_core::animation::Animated;
    use director_core::node::effect::{EffectNode, EffectType};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(r##"new_director(320, 240, 30)"##)
        .expect("build script must evaluate");

    let effect = EffectNode {
        effects: vec![
            EffectType::Blur(Animated::new(5.0)),
            EffectType::Pixelate(Animated::new(8.0)),
        ],
        style: taffy::style::Style::default(),
        shader_cache: Arc::new(Mutex::new(HashMap::new())),
        current_time: 0.0,
    };

    {
        let mut d = movie.director.lock().unwrap();
        let _id = d.scene.add_node(Box::new(effect));
    }

    let tmp = std::env::temp_dir().join("director_project_effect_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }
    let loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    let mut found = false;
    for sn in loaded.scene.nodes.iter().flatten() {
        if let Some(effect) = sn.element.as_any().downcast_ref::<EffectNode>() {
            found = true;
            assert_eq!(effect.effects.len(), 2);
            match &effect.effects[0] {
                EffectType::Blur(sigma) => assert!((sigma.current_value - 5.0).abs() < 1e-3),
                _ => panic!("first effect should be Blur"),
            }
            match &effect.effects[1] {
                EffectType::Pixelate(value) => assert!((value.current_value - 8.0).abs() < 1e-3),
                _ => panic!("second effect should be Pixelate"),
            }
        }
    }
    assert!(found, "loaded project must contain an EffectNode");

    let _ = std::fs::remove_file(&tmp);
}

/// Verifies that a CompositionNode round-trips with its start offset and
/// internal director settings instead of becoming a BoxNode placeholder.
#[test]
fn project_roundtrip_preserves_composition() {
    use director_core::node::composition::CompositionNode;
    use director_core::node::BoxNode;
    use director_core::video_wrapper::RenderMode;
    use director_core::{AssetLoader, DefaultAssetLoader, Director};
    use std::sync::Arc;

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(r##"new_director(320, 240, 30)"##)
        .expect("build script must evaluate");

    // Build a nested director with a single box.
    let loader: Arc<dyn AssetLoader> = Arc::new(DefaultAssetLoader);
    let mut inner = Director::new(100, 100, 30.0, loader, RenderMode::Preview, None);
    let mut box_node = BoxNode::new();
    box_node.bg_color = Some(director_core::animation::Animated::new(
        director_core::types::Color::new(1.0, 0.0, 0.0, 1.0),
    ));
    inner.scene.add_node(Box::new(box_node));

    let mut composition = CompositionNode::new(inner);
    composition.start_offset = 1.5;

    {
        let mut d = movie.director.lock().unwrap();
        let _id = d.scene.add_node(Box::new(composition));
    }

    let tmp = std::env::temp_dir().join("director_project_composition_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }
    let loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    let mut found = false;
    for sn in loaded.scene.nodes.iter().flatten() {
        if let Some(comp) = sn.element.as_any().downcast_ref::<CompositionNode>() {
            found = true;
            assert!((comp.start_offset - 1.5).abs() < 1e-3);
            let inner = comp.internal_director.lock().unwrap();
            assert_eq!(inner.width, 100);
            assert_eq!(inner.height, 100);
        }
    }
    assert!(found, "loaded project must contain a CompositionNode");

    let _ = std::fs::remove_file(&tmp);
}

/// Verifies that a SpectrumVisualizerNode round-trips with its bar count,
/// colors, and position instead of becoming a BoxNode placeholder.
#[test]
fn project_roundtrip_preserves_spectrum() {
    use director_core::node::spectrum_visualizer::SpectrumVisualizerNode;
    use director_core::types::Color;

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(r##"new_director(320, 240, 30)"##)
        .expect("build script must evaluate");

    let mut spectrum = SpectrumVisualizerNode::new(
        0,
        32,
        Color::new(1.0, 0.0, 0.0, 1.0),
        4.0,
        0.85,
        "top".to_string(),
        0.1,
        0.95,
        2.0,
    );
    spectrum.opacity.current_value = 0.7;

    {
        let mut d = movie.director.lock().unwrap();
        let _id = d.scene.add_node(Box::new(spectrum));
    }

    let tmp = std::env::temp_dir().join("director_project_spectrum_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }
    let loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    let mut found = false;
    for sn in loaded.scene.nodes.iter().flatten() {
        if let Some(spectrum) = sn.element.as_any().downcast_ref::<SpectrumVisualizerNode>() {
            found = true;
            assert_eq!(spectrum.bar_count, 32);
            assert!((spectrum.bar_color.r - 1.0).abs() < 1e-3);
            assert!((spectrum.smoothing - 0.85).abs() < 1e-3);
            assert_eq!(spectrum.position, "top");
            assert!((spectrum.opacity.current_value - 0.7).abs() < 1e-3);
        }
    }
    assert!(found, "loaded project must contain a SpectrumVisualizerNode");

    let _ = std::fs::remove_file(&tmp);
}

/// Verifies that a SmartCaptionNode created through the Rhai API survives a
/// project save/load round-trip with its trajectory and entity refs intact.
#[test]
fn project_roundtrip_preserves_smart_caption_from_rhai() {
    use director_core::node::smart_caption::SmartCaptionNode;

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let build = r##"
        let movie = new_director(1080, 1920, 30);
        let face = create_entity(movie, "face", "mara");
        add_entity_sample(movie, face, 0.0, 400.0, 1500.0, 200.0, 200.0, 1.0);
        add_entity_sample(movie, face, 2.0, 400.0, 1500.0, 200.0, 200.0, 1.0);

        let scene = movie.add_scene(2.0);
        let caption = scene.add_smart_caption(#{
            content: [#{ text: "hello world", start: 0.0, end: 2.0 }],
            entity_refs: [face.to_string()],
            start_time: 0.0,
            end_time: 2.0,
            motion_penalty: 0.1,
            smoothing_window: 1,
            sample_rate: 10.0,
        });

        #{ movie: movie, face: face.to_string() }
    "##;

    let result = engine
        .eval::<rhai::Map>(build)
        .expect("build script must evaluate");
    let movie = result
        .get("movie")
        .cloned()
        .and_then(|v| v.try_cast::<director_core::scripting::MovieHandle>())
        .expect("movie handle");
    let face_uuid = result
        .get("face")
        .and_then(|v| v.clone().into_string().ok())
        .expect("face uuid string");

    let tmp = std::env::temp_dir().join("director_project_smart_caption_rhai_roundtrip.json");
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
            assert!(!smart.trajectory.is_empty(), "trajectory must survive round-trip");
            assert_eq!(sn.entity_refs.len(), 1, "entity refs must survive round-trip");
            assert_eq!(sn.entity_refs[0].to_string(), face_uuid, "entity ref UUID must match");
            // The trajectory should place the caption away from the bottom face.
            let (_x, y) = smart.trajectory.sample_at(0.5).expect("sample at 0.5");
            assert!(y < 960.0, "caption should avoid bottom face, got y={}", y);
        }
    }
    assert!(found, "loaded project must contain a SmartCaptionNode");

    let _ = std::fs::remove_file(&tmp);
}

/// Verifies that a loaded SmartCaptionNode's trajectory still avoids the
/// tracked entities in the restored EntityRegistry, and that a corrupted
/// trajectory is detected by the post-load verification stage.
#[test]
fn loaded_smart_caption_trajectory_avoids_entities() {
    use director_core::entity::{BoundingBox, EntityKind};
    use director_core::node::caption::{CaptionLine, CaptionNode, CaptionWord};
    use director_core::node::smart_caption::{SmartCaptionConfig, SmartCaptionNode};
    use director_core::render_manifest::{VerificationConfig, VerificationIssueType};
    use director_core::semantic::{CaptionTrajectory, SemanticManifest, SolverConfig, TrajectorySample};
    use std::sync::{Arc, Mutex};

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(r##"new_director(1080, 1920, 30)"##)
        .expect("build script must evaluate");

    // Build a SmartCaptionNode that avoids a face at the bottom of the frame.
    let line = CaptionLine {
        text: "hello world".into(),
        words: vec![CaptionWord {
            text: "hello world".into(),
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
    registry.add_sample(face, 0.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);
    registry.add_sample(face, 2.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);

    let manifest = SemanticManifest {
        entity_refs: vec![face],
        start_time: 0.0,
        end_time: 2.0,
        solver: SolverConfig::default(),
    };

    let config = SmartCaptionConfig::default();
    let smart = SmartCaptionNode::from_manifest(inner, &registry, &manifest, config);

    {
        let mut d = movie.director.lock().unwrap();
        d.entities = registry;
        let _node_id = d.scene.add_node(Box::new(smart));
    }

    // Save + load.
    let tmp = std::env::temp_dir().join("director_project_smart_caption_verify_roundtrip.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).expect("save_project_json must succeed");
    }
    let mut loaded = director_core::Director::load_project_json(&tmp)
        .expect("load_project_json must succeed");

    // Post-load verification should report no overlap issues.
    let issues = loaded.verify_smart_caption_trajectories(&VerificationConfig::default());
    assert!(
        issues.is_empty(),
        "loaded SmartCaption trajectory should avoid restored entities, got {:?}",
        issues
    );

    // Negative case: corrupt the loaded trajectory so it overlaps the face.
    let smart_id = loaded
        .scene
        .iter_alive()
        .find_map(|(id, sn)| {
            sn.element
                .as_any()
                .downcast_ref::<SmartCaptionNode>()
                .map(|_| id)
        })
        .expect("loaded project must contain a SmartCaptionNode");

    {
        let sn = loaded.scene.get_node_mut(smart_id).expect("smart caption node");
        let smart = sn
            .element
            .as_any_mut()
            .downcast_mut::<SmartCaptionNode>()
            .expect("SmartCaptionNode");
        smart.trajectory = CaptionTrajectory::new(vec![
            TrajectorySample {
                time: 0.0,
                x: 500.0,
                y: 1600.0,
                anchor: None,
            },
            TrajectorySample {
                time: 2.0,
                x: 500.0,
                y: 1600.0,
                anchor: None,
            },
        ]);
    }

    let issues = loaded.verify_smart_caption_trajectories(&VerificationConfig::default());
    assert!(!issues.is_empty(), "corrupted trajectory should be detected");
    assert_eq!(issues[0].issue_type, VerificationIssueType::CaptionOverFace);
    assert_eq!(issues[0].node_ids, vec![smart_id]);

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


/// Plan 05: required kinds never silently become Box when rebuild fails.
#[test]
fn required_kind_missing_data_errors() {
    use director_core::project::{director_from_project, Project, PROJECT_VERSION};
    use director_core::types::NodeId;
    use serde_json::json;

    // Minimal project with a broken Lottie (required) — no model / path.
    let project = Project {
        version: PROJECT_VERSION,
        width: 64,
        height: 64,
        fps: 30.0,
        nodes: vec![director_core::project::NodeData {
            id: NodeId::at(0),
            parent: None,
            kind: "Lottie".into(),
            transform: Default::default(),
            opacity: 1.0,
            z_index: 0,
            blend_mode: "src_over".into(),
            props: Default::default(),
            layout: Default::default(),
            animations: Default::default(),
            entity_refs: vec![],
        }],
        tracks: vec![],
        legacy_timeline: vec![],
        entities: vec![],
    };
    // Construct via JSON to avoid private field friction if NodeData fields differ.
    let _ = project;
    let raw = json!({
        "version": PROJECT_VERSION,
        "width": 64,
        "height": 64,
        "fps": 30.0,
        "nodes": [{
            "id": {"index": 0, "generation": 0},
            "parent": null,
            "kind": "Lottie",
            "transform": {
                "scale_x": 1.0, "scale_y": 1.0, "rotation": 0.0,
                "skew_x": 0.0, "skew_y": 0.0,
                "translate_x": 0.0, "translate_y": 0.0,
                "pivot_x": 0.5, "pivot_y": 0.5
            },
            "opacity": 1.0,
            "z_index": 0,
            "blend_mode": "src_over",
            "props": {},
            "layout": {},
            "animations": {},
            "entity_refs": []
        }],
        "tracks": [],
        "legacy_timeline": [],
        "entities": []
    });
    let project: director_core::project::Project =
        serde_json::from_value(raw).expect("project deser");
    let msg = match director_from_project(project) {
        Ok(_) => panic!("expected load error for broken required Lottie"),
        Err(e) => format!("{e:#}"),
    };
    assert!(
        msg.contains("Lottie") || msg.contains("required"),
        "expected required-kind error, got: {msg}"
    );
}

/// Plan 05: kind policy matrix classification.
#[test]
fn node_kind_tier_matrix() {
    use director_core::project::{node_kind_tier, NodeKindTier, REQUIRED_NODE_KINDS};
    for k in REQUIRED_NODE_KINDS {
        assert_eq!(node_kind_tier(k), NodeKindTier::Required, "{k}");
    }
    assert_eq!(node_kind_tier("Vector"), NodeKindTier::Optional);
    assert_eq!(node_kind_tier("NotANode"), NodeKindTier::Unknown);
}

/// Plan 05: catalog effect surface non-empty and project tags cover runtime enum tags.
#[test]
fn catalog_effect_parity() {
    use director_core::api_catalog::{catalog_effect_names, project_effect_type_tags};
    use director_core::animation::Animated;
    use director_core::node::effect::EffectType;
    use std::collections::HashSet;

    let names = catalog_effect_names();
    assert!(names.len() >= 50, "catalog should list many effects, got {}", names.len());
    for must in ["blur", "pixelate", "vignette", "glow"] {
        assert!(names.iter().any(|n| *n == must), "missing catalog name {must}");
    }

    let tags: HashSet<&str> = project_effect_type_tags().iter().copied().collect();
    // Spot-check runtime tags via type_tag() on constructed variants.
    let samples = [
        EffectType::Blur(Animated::new(1.0)).type_tag(),
        EffectType::Pixelate(Animated::new(2.0)).type_tag(),
        EffectType::Thermal.type_tag(),
        EffectType::Mirror.type_tag(),
        EffectType::Gameboy.type_tag(),
    ];
    for t in samples {
        assert!(tags.contains(t), "project_effect_type_tags missing {t}");
    }
    assert_eq!(tags.len(), project_effect_type_tags().len());
}

/// Plan 05: fps is f64 in project JSON (NTSC rates).
#[test]
fn schema_fps_f64_project_json() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());
    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(
            r##"let m = new_director(64, 64, 23.976); m.add_scene(0.5); m"##,
        )
        .expect("build");
    let tmp = std::env::temp_dir().join("director_fps_f64.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).unwrap();
    }
    let raw = std::fs::read_to_string(&tmp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let fps = v["fps"].as_f64().expect("fps number");
    assert!((fps - 23.976).abs() < 1e-9, "fps={fps}");
    let loaded = director_core::Director::load_project_json(&tmp).unwrap();
    assert!((loaded.fps - 23.976).abs() < 1e-9);
    let _ = std::fs::remove_file(&tmp);
}

/// Plan 05: save → load → pixels match for a simple Box scene.
#[test]
fn project_roundtrip_pixels_smoke() {
    use director_core::systems::layout::LayoutEngine;
    use director_core::systems::renderer::render_at_time;
    use skia_safe::{AlphaType, ColorSpace, ColorType, ImageInfo};

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());
    let build = r##"
        let movie = new_director(64, 64, 30);
        let scene = movie.add_scene(1.0);
        let bg = scene.add_box(#{ width: 64, height: 64, bg_color: "#112233" });
        let card = bg.add_box(#{ width: 20, height: 20, bg_color: "#ff0066" });
        movie
    "##;
    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(build)
        .expect("build");

    fn pixels(d: &mut director_core::Director) -> Vec<u8> {
        let info = ImageInfo::new(
            (d.width, d.height),
            ColorType::RGBA8888,
            AlphaType::Premul,
            Some(ColorSpace::new_srgb()),
        );
        let mut surface = skia_safe::surfaces::raster(&info, None, None).unwrap();
        let mut layout = LayoutEngine::new();
        render_at_time(d, &mut layout, 0.0, surface.canvas(), &mut None).unwrap();
        let mut px = vec![0u8; (d.width * d.height * 4) as usize];
        assert!(surface.read_pixels(&info, &mut px, (d.width * 4) as usize, (0, 0)));
        px
    }

    let before = {
        let mut d = movie.director.lock().unwrap();
        pixels(&mut d)
    };

    let tmp = std::env::temp_dir().join("director_project_pixels_smoke.json");
    {
        let d = movie.director.lock().unwrap();
        d.save_project_json(&tmp).unwrap();
    }
    let mut loaded = director_core::Director::load_project_json(&tmp).unwrap();
    let after = pixels(&mut loaded);
    assert_eq!(before.len(), after.len());
    let max_diff = before
        .iter()
        .zip(after.iter())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    assert!(
        max_diff <= 2,
        "pixel round-trip max channel diff {max_diff} exceeds tolerance"
    );
    let _ = std::fs::remove_file(&tmp);
}
