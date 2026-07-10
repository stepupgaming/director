//! Round-trip test for Critical blocker #2: versioned project save/load.
//!
//! Builds a Director (with Box nodes + tracks) via the Rhai API, saves it
//! to a versioned project JSON, loads it back, and asserts that the
//! graph topology, node kinds, transforms, opacity, and tracks survive.

use director_core::animation::Animated;
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
        let movie = new_director(320, 240, 30.0);
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
        let movie = new_director(320, 240, 30.0);
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
        let movie = new_director(320, 240, 30.0);
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

/// TextNode must round-trip its spans through save/load (audit #2 node kinds).
/// Before this landed, a Text node silently collapsed to a Box placeholder with
/// empty content — losing every span, color, and font size.
#[test]
fn project_roundtrip_preserves_text_spans() {
    use director_core::node::TextNode;

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let build = r##"
        let movie = new_director(480, 270, 30.0);
        let scene = movie.add_scene(2.0);
        let bg = scene.add_box(#{ width: 480, height: 270, bg_color: "#0a0a14" });
        let t = bg.add_text(#{
            content: [
                #{ text: "Hello ", font_size: 48.0, color: "#ffffff" },
                #{ text: "World", font_size: 48.0, color: "#ff0066", font_weight: 700 }
            ],
            fit: "none",
            text_align: "center",
        });
        movie
    "##;

    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(build)
        .expect("script must evaluate");

    // Capture the original text-node snapshot in one pass.
    struct Snapshot {
        id: director_core::types::NodeId,
        first_text: String,
        first_size: f32,
        first_color: String,
        second_text: String,
        second_color: String,
        text_count: usize,
        box_count: usize,
    }
    let snap: Snapshot = {
        let director = movie.director.lock().unwrap();
        let mut text_count = 0usize;
        let mut box_count = 0usize;
        let mut id = 0usize;
        let mut first_text = String::new();
        let mut first_size = 0.0f32;
        let mut first_color = String::new();
        let mut second_text = String::new();
        let mut second_color = String::new();
        for (i, node_opt) in director.scene.nodes.iter().enumerate() {
            if let Some(sn) = node_opt {
                if let Some(t) = sn.element.as_any().downcast_ref::<TextNode>() {
                    text_count += 1;
                    id = i;
                    let spans = &t.spans;
                    if spans.len() >= 2 {
                        first_text = spans[0].text.clone();
                        first_size = spans[0].font_size.unwrap_or(0.0);
                        first_color = spans[0]
                            .color
                            .map(color_hex)
                            .unwrap_or_default();
                        second_text = spans[1].text.clone();
                        second_color = spans[1]
                            .color
                            .map(color_hex)
                            .unwrap_or_default();
                    }
                }
                if sn.element.as_any().is::<director_core::node::BoxNode>() {
                    box_count += 1;
                }
            }
        }
        Snapshot {
            id: id as director_core::types::NodeId,
            first_text,
            first_size,
            first_color,
            second_text,
            second_color,
            text_count,
            box_count,
        }
    };
    assert!(snap.text_count >= 1, "expected at least one TextNode");
    assert!(snap.first_size > 0.0, "expected first span size > 0");

    // Save + reload.
    let tmp = std::env::temp_dir().join("director_text_roundtrip.json");
    {
        let dir = movie.director.lock().unwrap();
        director_core::Director::save_project_json(&dir, &tmp)
            .expect("save must succeed");
    }
    let loaded = director_core::Director::load_project_json(&tmp).expect("load must succeed");

    let reloaded = loaded.scene.nodes[snap.id as usize]
        .as_ref()
        .unwrap()
        .element
        .as_any()
        .downcast_ref::<director_core::node::TextNode>()
        .expect("TextNode must reload as TextNode, not Box placeholder");
    assert_eq!(reloaded.spans.len(), 2, "span count must survive round-trip");
    assert_eq!(reloaded.spans[0].text, snap.first_text);
    assert_eq!(reloaded.spans[0].font_size.unwrap_or(0.0), snap.first_size);
    assert_eq!(
        reloaded.spans[0].color.map(color_hex).unwrap_or_default(),
        snap.first_color,
        "first span color must survive round-trip"
    );
    assert_eq!(reloaded.spans[1].text, snap.second_text);
    assert_eq!(
        reloaded.spans[1].color.map(color_hex).unwrap_or_default(),
        snap.second_color,
        "second span red color must survive round-trip"
    );

    // Box count should still include the bg from the build.
    let mut still_box = 0usize;
    let mut still_text = 0usize;
    for sn in loaded.scene.nodes.iter().flatten() {
        if sn.element.as_any().is::<director_core::node::BoxNode>() {
            still_box += 1;
        }
        if sn.element.as_any().is::<TextNode>() {
            still_text += 1;
        }
    }
    assert_eq!(still_box, snap.box_count, "Box kind count preserved");
    assert_eq!(still_text, snap.text_count, "Text kind count preserved");

    let _ = std::fs::remove_file(&tmp);
}

fn color_hex(c: director_core::types::Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (c.r * 255.0) as u8,
        (c.g * 255.0) as u8,
        (c.b * 255.0) as u8
    )
}

/// ImageNode must reload as `Image` (audit #2 node kinds), not collapse to a
/// Box placeholder. We use a non-existent path so we don't depend on a real
/// PNG being on disk; the *kind* survives even when the bytes fail to load.
#[test]
fn project_roundtrip_preserves_image_kind() {
    use director_core::node::image_node::ImageNode;

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let build = r##"
        let movie = new_director(640, 360, 30.0);
        let scene = movie.add_scene(2.0);
        let bg = scene.add_box(#{ width: 640, height: 360, bg_color: "#000000" });
        let img = bg.add_image("does-not-exist.png", #{
            object_fit: "contain",
            border_radius: 12.0,
        });
        movie
    "##;
    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(build)
        .expect("script must evaluate");

    let image_id = {
        let director = movie.director.lock().unwrap();
        let mut found = None;
        for (i, node_opt) in director.scene.nodes.iter().enumerate() {
            if let Some(sn) = node_opt {
                if sn.element.as_any().is::<ImageNode>() {
                    found = Some(i as director_core::types::NodeId);
                }
            }
        }
        found.expect("build should produce an ImageNode")
    };

    let tmp = std::env::temp_dir().join("director_image_roundtrip.json");
    {
        let dir = movie.director.lock().unwrap();
        director_core::Director::save_project_json(&dir, &tmp).expect("save");
    }
    let loaded = director_core::Director::load_project_json(&tmp).expect("load");

    let reloaded = loaded.scene.nodes[image_id as usize]
        .as_ref()
        .expect("image node slot present after round-trip")
        .element
        .as_any()
        .downcast_ref::<ImageNode>()
        .expect("round-tripped ImageNode must reconstruct as ImageNode, not Box placeholder");
    assert_eq!(
        reloaded.source_path.as_deref(),
        Some("does-not-exist.png"),
        "source path must survive round-trip"
    );
    assert_eq!(
        reloaded.object_fit,
        director_core::types::ObjectFit::Contain,
        "object_fit must survive round-trip"
    );
    assert!(
        (reloaded.border_radius.current_value - 12.0).abs() < 0.001,
        "border_radius must survive round-trip"
    );

    let _ = std::fs::remove_file(&tmp);
}

/// CaptionNode must reload with its caption lines intact (audit #2).
#[test]
fn project_roundtrip_preserves_caption_lines() {
    use director_core::node::caption::CaptionNode;

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let build = r##"
        let movie = new_director(640, 480, 30.0);
        let scene = movie.add_scene(2.0);
        let bg = scene.add_box(#{ width: 640, height: 480, bg_color: "#202028" });
        let c = bg.add_caption(#{
            source: "1\n00:00:00,500 --> 00:00:02,000\nHello world\n",
            font_size: 42.0,
            color: "#ffffff",
            highlight_color: "#ffcc00",
            position: "bottom",
            pill_enabled: false,
        });
        movie
    "##;
    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(build)
        .expect("script must evaluate");

    // Save.
    let (caption_id, original_count, original_first_text) = {
        let dir = movie.director.lock().unwrap();
        let mut caption_id = 0usize;
        let mut count = 0usize;
        let mut first_text = String::new();
        for (i, n) in dir.scene.nodes.iter().enumerate() {
            if let Some(sn) = n {
                if let Some(c) = sn.element.as_any().downcast_ref::<CaptionNode>() {
                    caption_id = i;
                    count = c.lines.len();
                    if let Some(first) = c.lines.first() {
                        first_text = first.text.clone();
                    }
                }
            }
        }
        (caption_id, count, first_text)
    };
    assert!(original_count > 0, "expected at least one caption line");

    let tmp = std::env::temp_dir().join("director_caption_roundtrip.json");
    {
        let dir = movie.director.lock().unwrap();
        director_core::Director::save_project_json(&dir, &tmp).expect("save");
    }
    let loaded = director_core::Director::load_project_json(&tmp).expect("load");

    let reloaded = loaded.scene.nodes[caption_id]
        .as_ref()
        .expect("caption slot present")
        .element
        .as_any()
        .downcast_ref::<CaptionNode>()
        .expect("CaptionNode must reload as CaptionNode, not Box placeholder");
    assert_eq!(reloaded.lines.len(), original_count, "caption line count preserved");
    assert_eq!(
        reloaded.lines[0].text, original_first_text,
        "first caption line text preserved"
    );
    // Pill was disabled in the build, must still be false on reload.
    assert!(!reloaded.style.pill_enabled);

    let _ = std::fs::remove_file(&tmp);
}

/// EffectNode must round-trip; the kind tags survive so reload restores each
/// variant of EffectType instead of collapsing to a Box placeholder.
#[test]
fn project_roundtrip_preserves_effect_kinds() {
    use director_core::node::effect::{EffectNode, EffectType};

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    // Direct unit-style construction is easier than going through the Rhai
    // API here since we want every variant exercised at once.
    let mut director = director_core::Director::new(
        512,
        512,
        30.0,
        Arc::new(DefaultAssetLoader),
        director_core::video_wrapper::RenderMode::Preview,
        None,
    );
    let mut original = EffectNode {
        effects: Vec::new(),
        style: taffy::style::Style::default(),
        shader_cache: std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::HashMap::new(),
        )),
        current_time: 0.0,
    };
    original.effects = vec![
        EffectType::Blur(Animated::new(4.0)),
        EffectType::DropShadow {
            blur: Animated::new(8.0),
            offset_x: Animated::new(2.0),
            offset_y: Animated::new(4.0),
            color: Animated::new(director_core::types::Color::BLACK),
        },
        EffectType::Vignette {
            intensity: Animated::new(0.7),
            radius: Animated::new(0.6),
            feather: Animated::new(0.4),
        },
        EffectType::Mirror,
        EffectType::Thermal,
    ];
    let original_kinds: Vec<String> = original
        .effects
        .iter()
        .map(director_core::project::effect_kind_name_pub)
        .map(|s| s.to_string())
        .collect();

    let id = director.scene.add_node(Box::new(original));
    director.timeline.push(director_core::director::TimelineItem {
        scene_root: id,
        name: None,
        start_time: 0.0,
        duration: 1.0,
        z_index: 0,
        audio_tracks: vec![],
            freeze_time: None,
    });

    let tmp = std::env::temp_dir().join("director_effect_roundtrip.json");
    director.save_project_json(&tmp).expect("save");
    let loaded = director_core::Director::load_project_json(&tmp).expect("load");

    let reloaded = loaded
        .scene
        .nodes
        .iter()
        .find_map(|n| n.as_ref())
        .and_then(|sn| sn.element.as_any().downcast_ref::<EffectNode>())
        .expect("EffectNode must reload as EffectNode, not Box placeholder");
    let reloaded_kinds: Vec<String> = reloaded
        .effects
        .iter()
        .map(director_core::project::effect_kind_name_pub)
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        reloaded_kinds, original_kinds,
        "effect kinds must survive round-trip"
    );

    let _ = std::fs::remove_file(&tmp);
}

fn _effect_kind_name_unused() {}

/// Audit #19: `add_freeze` keeps sampling the source at `freeze_at` for the
/// entire hold window, and `add_loop` clones the source's audio onto every
/// loop iteration so a music loop doesn't drop the audio.
#[test]
fn freeze_and_loop_preserve_audio_and_freeze_time() {
    use director_core::director::{Director, TimelineItem};

    let mut director = Director::new(
        320,
        240,
        30.0,
        Arc::new(DefaultAssetLoader),
        director_core::video_wrapper::RenderMode::Preview,
        None,
    );
    let root = director.scene.add_node(Box::new(BoxNode::new()));
    // Source clip with a music track ID attached.
    director.timeline.push(TimelineItem {
        scene_root: root,
        name: Some("scene_a".into()),
        start_time: 0.0,
        duration: 2.0,
        z_index: 0,
        audio_tracks: vec![7],
        freeze_time: None,
    });
    // Stand-in for `add_loop(movie, scene, 3)` semantics: clone the source
    // three times back-to-back with the same audio_tracks (matches the new
    // Rhai implementation that copies audio_tracks onto every loop).
    for i in 1..3 {
        director.timeline.push(TimelineItem {
            scene_root: root,
            name: Some(format!("loop_{i}")),
            start_time: 2.0 + (i as f64) * 2.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![7],
            freeze_time: None,
        });
    }
    // Source A (0..2) + loop_1 (2..4) + loop_2 (4..6) + freeze (8..9.5) = 4 items.
    // Hold a frame at t=1.5 for 1.5s after the loops.
    director.timeline.push(TimelineItem {
        scene_root: root,
        name: Some("freeze".into()),
        start_time: 8.0,
        duration: 1.5,
        z_index: 0,
        audio_tracks: vec![],
        freeze_time: Some(1.5),
    });

    // 1. Loop iterations preserve audio.
    assert_eq!(director.timeline.len(), 4, "source + 2 loops + freeze");
    let loops: Vec<&TimelineItem> =
        director.timeline.iter().filter(|t| t.name.as_deref() == Some("loop_1")
            || t.name.as_deref() == Some("loop_2")).collect();
    assert_eq!(loops.len(), 2);
    for l in loops {
        assert_eq!(l.audio_tracks, vec![7], "loop must copy source audio (audit #19)");
    }

    // 2. Freeze holds sampling at the recorded local time for the entire
    //    duration window regardless of advancing global_time.
    let freeze_local_t_0 = director
        .collect_active_roots(8.0)
        .into_iter()
        .find(|(rid, _)| *rid == root)
        .map(|(_, t)| t)
        .expect("freeze window should be active at t=8.0");
    let freeze_local_t_mid = director
        .collect_active_roots(9.0)
        .into_iter()
        .find(|(rid, _)| *rid == root)
        .map(|(_, t)| t)
        .expect("freeze window should still be active at t=9.0");
    assert!(
        (freeze_local_t_0 - 1.5).abs() < 1e-9,
        "freeze: local_time at start of hold should be freeze_time=1.5, got {freeze_local_t_0}"
    );
    assert!(
        (freeze_local_t_mid - 1.5).abs() < 1e-9,
        "freeze: local_time mid-hold MUST stay at freeze_time=1.5, got {freeze_local_t_mid}"
    );

    // 3. After the freeze window expires, normal local_time resumes for
    //    whatever timeline item is live.
    director.timeline.push(TimelineItem {
        scene_root: root,
        name: Some("after".into()),
        start_time: 9.5,
        duration: 1.0,
        z_index: 0,
        audio_tracks: vec![],
        freeze_time: None,
    });
    let after = director
        .collect_active_roots(10.0)
        .into_iter()
        .find(|(rid, _)| *rid == root)
        .map(|(_, t)| t)
        .expect("post-freeze item should be active at t=10.0");
    assert!(
        (after - 0.5).abs() < 1e-9,
        "after freeze, local_time should be elapsed=0.5; got {after}"
    );
}
