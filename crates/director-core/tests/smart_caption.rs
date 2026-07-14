use director_core::director::Director;
use director_core::element::Element;
use director_core::entity::{BoundingBox, EntityKind};
use director_core::node::caption::{CaptionLine, CaptionNode, CaptionPosition, CaptionWord};
use director_core::node::smart_caption::SmartCaptionNode;
use director_core::scripting::register_rhai_api;
use director_core::semantic::{solve_caption_trajectory, SemanticManifest, SolverConfig};
use director_core::video_wrapper::RenderMode;
use director_core::DefaultAssetLoader;
use rhai::Engine;
use std::sync::{Arc, Mutex};

fn empty_font_collection() -> Arc<Mutex<skia_safe::textlayout::FontCollection>> {
    Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()))
}

fn single_word_line(text: &str, start: f64, end: f64) -> CaptionLine {
    CaptionLine {
        text: text.to_string(),
        words: vec![CaptionWord {
            text: text.to_string(),
            start_sec: start,
            end_sec: end,
            display_line_idx: 0,
        }],
        start_sec: start,
        end_sec: end,
    }
}

#[test]
fn smart_caption_avoids_bottom_face() {
    let mut director = Director::new(
        1080,
        1920,
        30,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    );

    let face = director.entities.create(EntityKind::Face, "mara".to_string());
    director.entities.add_sample(
        face,
        0.0,
        BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
        1.0,
    );
    director.entities.add_sample(
        face,
        2.0,
        BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
        1.0,
    );

    let mut solver = SolverConfig::default();
    solver.motion_penalty = 0.1;
    solver.smoothing_window = 1;
    let manifest = SemanticManifest {
        entity_refs: vec![face],
        start_time: 0.0,
        end_time: 2.0,
        solver,
    };

    let trajectory = solve_caption_trajectory(
        &director.entities,
        &manifest,
        1080.0,
        1920.0,
        800.0,
        100.0,
        60.0,
        110.0,
        60.0,
        60.0,
        10.0,
    );

    let lines = vec![single_word_line("hello", 0.0, 2.0)];
    let inner = CaptionNode::new(lines, empty_font_collection());
    let mut smart = SmartCaptionNode::new(inner, trajectory);

    smart.update(0.5);
    assert_eq!(smart.inner.style.position, CaptionPosition::Top);
}

#[test]
fn smart_caption_switches_when_face_moves() {
    let mut director = Director::new(
        1080,
        1920,
        30,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    );

    // Static faces on the left and right edges keep the solver in the
    // vertical axis. A moving face starts low (overlapping the bottom
    // anchor) and ends high (overlapping the top anchor).
    let face = director.entities.create(EntityKind::Face, "mara".to_string());
    director.entities.add_sample(
        face,
        0.0,
        BoundingBox::new(400.0, 1760.0, 200.0, 200.0),
        1.0,
    );
    director.entities.add_sample(
        face,
        2.0,
        BoundingBox::new(400.0, 60.0, 200.0, 200.0),
        1.0,
    );
    let face_left = director.entities.create(EntityKind::Face, "left".to_string());
    director.entities.add_sample(
        face_left,
        0.0,
        BoundingBox::new(60.0, 800.0, 200.0, 200.0),
        1.0,
    );
    director.entities.add_sample(
        face_left,
        2.0,
        BoundingBox::new(60.0, 800.0, 200.0, 200.0),
        1.0,
    );
    let face_right = director.entities.create(EntityKind::Face, "right".to_string());
    director.entities.add_sample(
        face_right,
        0.0,
        BoundingBox::new(820.0, 800.0, 200.0, 200.0),
        1.0,
    );
    director.entities.add_sample(
        face_right,
        2.0,
        BoundingBox::new(820.0, 800.0, 200.0, 200.0),
        1.0,
    );

    let mut solver = SolverConfig::default();
    solver.motion_penalty = 0.1;
    solver.smoothing_window = 1;
    let manifest = SemanticManifest {
        entity_refs: vec![face, face_left, face_right],
        start_time: 0.0,
        end_time: 2.0,
        solver,
    };

    let trajectory = solve_caption_trajectory(
        &director.entities,
        &manifest,
        1080.0,
        1920.0,
        800.0,
        100.0,
        60.0,
        110.0,
        60.0,
        60.0,
        10.0,
    );

    let lines = vec![single_word_line("hello", 0.0, 2.0)];
    let inner = CaptionNode::new(lines, empty_font_collection());
    let mut smart = SmartCaptionNode::new(inner, trajectory);

    smart.update(0.2);
    assert_eq!(smart.inner.style.position, CaptionPosition::Top);
    // The solver minimizes motion, so it switches only when the face
    // actually threatens the top anchor (around t=1.9).
    smart.update(1.95);
    assert_eq!(smart.inner.style.position, CaptionPosition::Bottom);
}

#[test]
fn rhai_add_smart_caption_follows_entity() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(1080, 1920, 30);
        let face = create_entity(movie, "face", "mara");
        add_entity_sample(movie, face, 0.0, 400.0, 1500.0, 200.0, 200.0, 1.0);
        add_entity_sample(movie, face, 2.0, 400.0, 1500.0, 200.0, 200.0, 1.0);

        let scene = movie.add_scene(2.0);
        let caption = scene.add_smart_caption(#{
            content: [#{ text: "hello", start: 0.0, end: 2.0 }],
            entity_refs: [face.to_string()],
            start_time: 0.0,
            end_time: 2.0,
            motion_penalty: 0.1,
            smoothing_window: 1,
            sample_rate: 10.0,
        });

        #{
            entity_count: caption.entity_ids().len(),
            first_entity: caption.entity_ids()[0].to_string(),
        }
    "#;

    let result = engine
        .eval::<rhai::Map>(script)
        .expect("smart caption script must evaluate");
    let entity_count = result
        .get("entity_count")
        .and_then(|v| v.as_int().ok())
        .expect("entity_count must be an integer");
    assert_eq!(entity_count, 1, "smart caption node must reference the face entity");
    let first_entity = result
        .get("first_entity")
        .and_then(|v| v.clone().into_string().ok())
        .expect("first_entity must be a string");
    assert!(!first_entity.is_empty(), "smart caption entity ref must be non-empty");
}

#[test]
fn smart_caption_uses_left_anchor_when_right_top_and_bottom_are_blocked() {
    let mut director = Director::new(
        1080,
        1920,
        30,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    );

    // Faces block the top, bottom, and right, leaving the left side open.
    let face_top = director.entities.create(EntityKind::Face, "top".to_string());
    director.entities.add_sample(
        face_top,
        0.0,
        BoundingBox::new(400.0, 60.0, 200.0, 200.0),
        1.0,
    );
    director.entities.add_sample(
        face_top,
        2.0,
        BoundingBox::new(400.0, 60.0, 200.0, 200.0),
        1.0,
    );
    let face_bottom = director.entities.create(EntityKind::Face, "bottom".to_string());
    director.entities.add_sample(
        face_bottom,
        0.0,
        BoundingBox::new(400.0, 1660.0, 200.0, 200.0),
        1.0,
    );
    director.entities.add_sample(
        face_bottom,
        2.0,
        BoundingBox::new(400.0, 1660.0, 200.0, 200.0),
        1.0,
    );
    let face_right = director.entities.create(EntityKind::Face, "right".to_string());
    director.entities.add_sample(
        face_right,
        0.0,
        BoundingBox::new(820.0, 800.0, 200.0, 200.0),
        1.0,
    );
    director.entities.add_sample(
        face_right,
        2.0,
        BoundingBox::new(820.0, 800.0, 200.0, 200.0),
        1.0,
    );

    let mut solver = SolverConfig::default();
    solver.motion_penalty = 0.1;
    solver.smoothing_window = 1;
    let manifest = SemanticManifest {
        entity_refs: vec![face_top, face_bottom, face_right],
        start_time: 0.0,
        end_time: 2.0,
        solver,
    };

    let trajectory = solve_caption_trajectory(
        &director.entities,
        &manifest,
        1080.0,
        1920.0,
        800.0,
        100.0,
        60.0,
        110.0,
        60.0,
        60.0,
        10.0,
    );

    let lines = vec![single_word_line("hello", 0.0, 2.0)];
    let inner = CaptionNode::new(lines, empty_font_collection());
    let mut smart = SmartCaptionNode::new(inner, trajectory);

    smart.update(0.5);
    assert_eq!(smart.inner.style.position, CaptionPosition::Left);
}

#[test]
fn smart_caption_uses_right_anchor_when_left_top_and_bottom_are_blocked() {
    let mut director = Director::new(
        1080,
        1920,
        30,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    );

    // Faces block the top, bottom, and left, leaving the right side open.
    let face_top = director.entities.create(EntityKind::Face, "top".to_string());
    director.entities.add_sample(
        face_top,
        0.0,
        BoundingBox::new(400.0, 60.0, 200.0, 200.0),
        1.0,
    );
    director.entities.add_sample(
        face_top,
        2.0,
        BoundingBox::new(400.0, 60.0, 200.0, 200.0),
        1.0,
    );
    let face_bottom = director.entities.create(EntityKind::Face, "bottom".to_string());
    director.entities.add_sample(
        face_bottom,
        0.0,
        BoundingBox::new(400.0, 1660.0, 200.0, 200.0),
        1.0,
    );
    director.entities.add_sample(
        face_bottom,
        2.0,
        BoundingBox::new(400.0, 1660.0, 200.0, 200.0),
        1.0,
    );
    let face_left = director.entities.create(EntityKind::Face, "left".to_string());
    director.entities.add_sample(
        face_left,
        0.0,
        BoundingBox::new(60.0, 800.0, 200.0, 200.0),
        1.0,
    );
    director.entities.add_sample(
        face_left,
        2.0,
        BoundingBox::new(60.0, 800.0, 200.0, 200.0),
        1.0,
    );

    let mut solver = SolverConfig::default();
    solver.motion_penalty = 0.1;
    solver.smoothing_window = 1;
    let manifest = SemanticManifest {
        entity_refs: vec![face_top, face_bottom, face_left],
        start_time: 0.0,
        end_time: 2.0,
        solver,
    };

    let trajectory = solve_caption_trajectory(
        &director.entities,
        &manifest,
        1080.0,
        1920.0,
        800.0,
        100.0,
        60.0,
        110.0,
        60.0,
        60.0,
        10.0,
    );

    let lines = vec![single_word_line("hello", 0.0, 2.0)];
    let inner = CaptionNode::new(lines, empty_font_collection());
    let mut smart = SmartCaptionNode::new(inner, trajectory);

    smart.update(0.5);
    assert_eq!(smart.inner.style.position, CaptionPosition::Right);
}

#[test]
fn smart_caption_continuous_uses_free_xy_position() {
    use director_core::node::smart_caption::{SmartCaptionConfig, SmartCaptionNode};

    let mut director = Director::new(
        1080,
        1920,
        30,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    );

    let face = director.entities.create(EntityKind::Face, "mara".to_string());
    director.entities.add_sample(
        face,
        0.0,
        BoundingBox::new(820.0, 1500.0, 200.0, 200.0),
        1.0,
    );
    director.entities.add_sample(
        face,
        2.0,
        BoundingBox::new(820.0, 1500.0, 200.0, 200.0),
        1.0,
    );

    let mut solver = SolverConfig::default();
    solver.motion_penalty = 0.1;
    solver.smoothing_window = 1;
    let manifest = SemanticManifest {
        entity_refs: vec![face],
        start_time: 0.0,
        end_time: 2.0,
        solver,
    };

    let lines = vec![single_word_line("hello", 0.0, 2.0)];
    let inner = CaptionNode::new(lines, empty_font_collection());
    let mut smart = SmartCaptionNode::from_manifest_continuous(
        inner,
        &director.entities,
        &manifest,
        SmartCaptionConfig::default(),
    );

    smart.update(0.5);
    assert!(matches!(smart.inner.style.position, CaptionPosition::FreeXY { .. }));
}

#[test]
fn project_roundtrip_smart_caption_preserves_entity_refs() {
    use director_core::project::{director_to_project, director_from_project};

    let mut director = Director::new(
        1080,
        1920,
        30,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    );

    let face = director.entities.create(EntityKind::Face, "mara".to_string());
    director.entities.add_sample(
        face,
        0.0,
        BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
        1.0,
    );
    director.entities.add_sample(
        face,
        2.0,
        BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
        1.0,
    );

    let mut solver = SolverConfig::default();
    solver.motion_penalty = 0.1;
    solver.smoothing_window = 1;
    let manifest = SemanticManifest {
        entity_refs: vec![face],
        start_time: 0.0,
        end_time: 2.0,
        solver,
    };

    let trajectory = solve_caption_trajectory(
        &director.entities,
        &manifest,
        1080.0,
        1920.0,
        800.0,
        100.0,
        60.0,
        110.0,
        60.0,
        60.0,
        10.0,
    );

    let lines = vec![single_word_line("hello", 0.0, 2.0)];
    let inner = CaptionNode::new(lines, empty_font_collection());
    let smart = SmartCaptionNode::new(inner, trajectory);
    let id = director.scene.add_node(Box::new(smart));
    if let Some(n) = director.scene.get_node_mut(id) {
        n.entity_refs = vec![face];
    }

    let project = director_to_project(&director);
    let loaded = director_from_project(project);
    assert!(loaded.is_ok(), "loading a project with a SmartCaptionNode must not panic");
    let loaded = loaded.unwrap();
    if let Some(sn) = loaded.scene.get_node(id) {
        assert_eq!(sn.entity_refs.len(), 1, "loaded node must preserve entity ref count");
        // SmartCaptionNode is now fully round-tripped; verify it is reconstructed
        // as a SmartCaptionNode rather than falling back to a BoxNode placeholder.
        let smart = sn
            .element
            .as_any()
            .downcast_ref::<director_core::node::smart_caption::SmartCaptionNode>()
            .expect("loaded node should be a SmartCaptionNode");
        assert!(
            !smart.trajectory.is_empty(),
            "SmartCaptionNode trajectory should survive the round-trip"
        );
    }
}
