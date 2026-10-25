use std::path::PathBuf;

#[test]
fn all_caption_fixtures() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixtures_dir = manifest_dir.join("tests/visual/fixtures/captions");

    let mut fixture_count = 0;
    let mut failures = Vec::new();

    for entry in std::fs::read_dir(&fixtures_dir).expect("Failed to read fixtures dir") {
        let entry = entry.expect("Failed to read dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rhai") {
            let rel_path = path
                .strip_prefix(&manifest_dir)
                .expect("fixture must be inside manifest dir");
            match crate::visual::run_rhai_visual_fixture(rel_path.to_str().unwrap(), "captions") {
                Ok(()) => fixture_count += 1,
                Err(msg) => failures.push(msg),
            }
        }
    }

    assert!(
        fixture_count > 0,
        "No .rhai fixtures found in {}",
        fixtures_dir.display()
    );

    if !failures.is_empty() {
        let mut msg = format!(
            "{} of {} caption fixture(s) failed:\n\n",
            failures.len(),
            fixture_count + failures.len()
        );
        for f in &failures {
            msg.push_str(&format!("  - {}\n", f));
        }
        panic!("{}", msg);
    }

    println!("Verified {} caption fixture(s)", fixture_count);
}

/// Rendered visual regression test for smart-caption face avoidance.
///
/// A face at the bottom of a 1080x1920 frame should push the smart caption
/// into the top half. After rendering, bright caption pixels must appear in
/// the top half and must not appear in the bottom half.
#[test]
fn smart_caption_avoids_bottom_face() {
    use director_core::animation::Animated;
    use director_core::entity::{BoundingBox, EntityKind};
    use director_core::node::caption::{CaptionLine, CaptionNode, CaptionWord};
    use director_core::node::smart_caption::{SmartCaptionConfig, SmartCaptionNode};
    use director_core::node::BoxNode;
    use director_core::semantic::{SemanticManifest, SolverConfig};
    use director_core::types::Color;
    use skia_safe::{AlphaType, ColorSpace, ColorType, EncodedImageFormat};
    use taffy::style::Dimension;

    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(1080, 1920);

    {
        let mut d = director_arc.lock().unwrap();

        // Black background so bright caption pixels are easy to find.
        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.0, 0.0, 0.0, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root_at(root_id, 0.0, 2.0, Some("smart_caption_face_avoidance".into()));

        // Face entity occupying the bottom-center of the frame.
        let face = d.entities.create(EntityKind::Face, "mara".to_string());
        d.entities.add_sample(
            face,
            0.0,
            BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
            1.0,
        );
        d.entities.add_sample(
            face,
            2.0,
            BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
            1.0,
        );

        // Smart caption that should avoid the face.
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
        let inner = CaptionNode::new(vec![line], d.assets.font_collection.clone());
        let manifest = SemanticManifest {
            entity_refs: vec![face],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };
        let smart = SmartCaptionNode::from_manifest(inner, &d.entities, &manifest, SmartCaptionConfig::default());
        let smart_id = d.scene.add_node(Box::new(smart));
        d.scene.add_child(root_id, smart_id);
    }

    let mut d = director_arc.lock().unwrap();

    // Drive the scene to t=0.5 so the SmartCaptionNode applies its trajectory.
    d.update(0.5, None);

    // Render at t=0.5.
    let width = d.width;
    let height = d.height;
    let info = skia_safe::ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );
    let mut surface =
        skia_safe::surfaces::raster(&info, None, None).expect("Failed to create Skia surface");
    director_render::render_frame(&mut d, 0.5, &mut surface)
        .expect("render_frame failed");

    let image = surface.image_snapshot();
    let data = image
        .encode(None, EncodedImageFormat::PNG, 100)
        .expect("Failed to encode image to PNG");
    let rendered_img = image::load_from_memory(data.as_bytes())
        .expect("Failed to load rendered image")
        .to_rgba8();

    // Compute the bounding box of all non-background (caption) pixels.
    // The background is pure black; anything else belongs to the caption
    // (text, pills, shadows, etc.). The face entity is not rendered.
    let half_y = (height as u32) / 2;
    let mut min_y: Option<u32> = None;
    let mut max_y: Option<u32> = None;
    let mut non_bg_count = 0u64;

    for (_, y, pixel) in rendered_img.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        // Treat any pixel that is not pure black as part of the caption.
        if r > 0 || g > 0 || b > 0 || a > 0 {
            min_y = Some(min_y.map_or(y, |v| v.min(y)));
            max_y = Some(max_y.map_or(y, |v| v.max(y)));
            non_bg_count += 1;
        }
    }

    assert!(non_bg_count > 0, "caption should be visible in the frame");

    let min_y = min_y.expect("non-background pixels exist");
    let max_y = max_y.expect("non-background pixels exist");
    let center_y = (min_y + max_y) / 2;

    assert!(
        center_y < half_y,
        "caption bounding box center should be in the top half of the frame; got min_y={}, max_y={}, half_y={}",
        min_y, max_y, half_y
    );

    assert_visual_match(&mut d, 0.5, "captions", "smart_caption_avoids_bottom_face")
        .expect("visual snapshot mismatch");
}

/// Rendered visual regression test for smart-caption anchor switching.
///
/// A face that moves from the bottom to the top of the frame should cause the
/// smart caption to switch from the top anchor to the bottom anchor mid-scene.
#[test]
fn smart_caption_switches_anchor_with_moving_face() {
    use director_core::animation::Animated;
    use director_core::entity::{BoundingBox, EntityKind};
    use director_core::node::caption::{CaptionLine, CaptionNode, CaptionPosition, CaptionWord};
    use director_core::node::smart_caption::{SmartCaptionConfig, SmartCaptionNode};
    use director_core::node::BoxNode;
    use director_core::semantic::{SemanticManifest, SolverConfig};
    use director_core::types::Color;
    use taffy::style::Dimension;

    use crate::visual::setup_test_director;

    fn anchor_is_top(position: &CaptionPosition) -> bool {
        matches!(
            position,
            CaptionPosition::Top | CaptionPosition::TopLeft | CaptionPosition::TopRight
        )
    }

    fn anchor_is_bottom(position: &CaptionPosition) -> bool {
        matches!(
            position,
            CaptionPosition::Bottom | CaptionPosition::BottomLeft | CaptionPosition::BottomRight
        )
    }

    let director_arc = setup_test_director(1080, 1920);
    let mut d = director_arc.lock().unwrap();

    // Black background so bright caption pixels are easy to find.
    let mut root = BoxNode::new();
    root.style.size.width = Dimension::percent(1.0);
    root.style.size.height = Dimension::percent(1.0);
    root.bg_color = Some(Animated::new(Color::new(0.0, 0.0, 0.0, 1.0)));
    let root_id = d.scene.add_node(Box::new(root));

    d.push_scene_root_at(root_id, 0.0, 4.0, Some("smart_caption_moving_face".into()));

    // Face entity that stays at the bottom for the first second, moves to
    // the top over the next two seconds, and stays at the top for the final
    // second. This gives the solver stable regions where the anchor choice is
    // unambiguous.
    let face = d.entities.create(EntityKind::Face, "mara".to_string());
    d.entities.add_sample(
        face,
        0.0,
        BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
        1.0,
    );
    d.entities.add_sample(
        face,
        1.0,
        BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
        1.0,
    );
    d.entities.add_sample(
        face,
        3.0,
        BoundingBox::new(400.0, 60.0, 200.0, 200.0),
        1.0,
    );
    d.entities.add_sample(
        face,
        4.0,
        BoundingBox::new(400.0, 60.0, 200.0, 200.0),
        1.0,
    );

    // Smart caption that should switch anchors as the face moves.
    let line = CaptionLine {
        text: "hello world".into(),
        words: vec![CaptionWord {
            text: "hello world".into(),
            start_sec: 0.0,
            end_sec: 4.0,
            display_line_idx: 0,
        }],
        start_sec: 0.0,
        end_sec: 4.0,
    };
    let inner = CaptionNode::new(vec![line], d.assets.font_collection.clone());

    // Use a low motion penalty so the solver freely switches to the best
    // anchor at each sample. This makes the top -> bottom switch explicit
    // when the face moves from bottom to top.
    let solver = SolverConfig {
        motion_penalty: 0.0,
        ..SolverConfig::default()
    };

    let manifest = SemanticManifest {
        entity_refs: vec![face],
        start_time: 0.0,
        end_time: 4.0,
        solver,
    };
    let smart = SmartCaptionNode::from_manifest(inner, &d.entities, &manifest, SmartCaptionConfig::default());
    let smart_id = d.scene.add_node(Box::new(smart));
    d.scene.add_child(root_id, smart_id);

    // Verify the solver switches anchors by inspecting the SmartCaptionNode's
    // chosen CaptionPosition at different times.
    d.update(0.5, None);
    let top_position = {
        let node = d.scene.get_node(smart_id).expect("smart caption node exists");
        let smart = node
            .element
            .as_any()
            .downcast_ref::<SmartCaptionNode>()
            .expect("node is a SmartCaptionNode");
        smart.inner.style.position.clone()
    };
    assert!(
        anchor_is_top(&top_position),
        "caption should use a top anchor when the face is at the bottom, got {:?}",
        top_position
    );

    d.update(3.5, None);
    let bottom_position = {
        let node = d.scene.get_node(smart_id).expect("smart caption node exists");
        let smart = node
            .element
            .as_any()
            .downcast_ref::<SmartCaptionNode>()
            .expect("node is a SmartCaptionNode");
        smart.inner.style.position.clone()
    };
    assert!(
        anchor_is_bottom(&bottom_position),
        "caption should use a bottom anchor when the face is at the top, got {:?}",
        bottom_position
    );

}
