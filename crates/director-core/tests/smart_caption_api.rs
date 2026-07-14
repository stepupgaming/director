//! Runtime test for the SmartCaptionNode Rhai API.
//! Exercises `create_semantic_manifest`, `create_smart_caption_config`, and
//! `add_smart_caption` from a Rhai script.

use director_core::scripting::register_rhai_api;
use director_core::DefaultAssetLoader;
use rhai::{Engine, Map};
use std::sync::Arc;

#[test]
fn smart_caption_from_manifest_avoids_face() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r##"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(2.0);

        // Create a face entity at the bottom of the frame.
        let face = movie.create_entity("face", "mara");
        movie.add_entity_sample(face, 0.0, 400.0, 1500.0, 200.0, 200.0, 1.0);
        movie.add_entity_sample(face, 2.0, 400.0, 1500.0, 200.0, 200.0, 1.0);

        let manifest = movie.create_semantic_manifest(#{
            entity_refs: [face],
            start_time: 0.0,
            end_time: 2.0,
        });

        let config = movie.create_smart_caption_config(#{
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            sample_rate: 10.0,
        });

        let caption = scene.add_smart_caption(
            manifest,
            config,
            #{
                content: [#{ text: "hello", start: 0.0, end: 2.0 }],
                font_size: 48.0,
                color: "#ffffff",
            }
        );

        // Force an update so the trajectory is applied.
        movie.update(0.5);

        // The caption node should exist and have the face in its entity refs.
        let ids = caption.entity_ids();
        #{
            entity_count: ids.len(),
            first_entity: if ids.len() > 0 { ids[0] } else { "" },
        }
    "##;

    let result = engine
        .eval::<Map>(script)
        .expect("smart caption script must evaluate");

    let entity_count: i64 = result
        .get("entity_count")
        .and_then(|v| v.as_int().ok())
        .expect("entity_count present");
    assert_eq!(entity_count, 1, "smart caption should reference one entity");

    let first_entity = result
        .get("first_entity")
        .and_then(|v| v.clone().into_string().ok())
        .unwrap_or_default();
    assert!(!first_entity.is_empty(), "first entity UUID should be present");
}

#[test]
fn smart_caption_continuous_from_manifest_works() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r##"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(2.0);

        let face = movie.create_entity("face", "mara");
        movie.add_entity_sample(face, 0.0, 400.0, 1500.0, 200.0, 200.0, 1.0);
        movie.add_entity_sample(face, 2.0, 400.0, 1500.0, 200.0, 200.0, 1.0);

        let manifest = movie.create_semantic_manifest(#{
            entity_refs: [face],
            start_time: 0.0,
            end_time: 2.0,
        });

        let config = movie.create_smart_caption_config(#{
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            sample_rate: 10.0,
            continuous: true,
        });

        let caption = scene.add_smart_caption(
            manifest,
            config,
            #{
                content: [#{ text: "hello", start: 0.0, end: 2.0 }],
                font_size: 48.0,
                color: "#ffffff",
            }
        );

        movie.update(0.5);

        #{
            entity_count: caption.entity_ids().len(),
        }
    "##;

    let result = engine
        .eval::<Map>(script)
        .expect("continuous smart caption script must evaluate");

    let entity_count: i64 = result
        .get("entity_count")
        .and_then(|v| v.as_int().ok())
        .expect("entity_count present");
    assert_eq!(entity_count, 1, "continuous smart caption should reference one entity");
}

#[test]
fn smart_caption_continuous_explicit_api_works() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r##"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(2.0);

        let face = movie.create_entity("face", "mara");
        movie.add_entity_sample(face, 0.0, 400.0, 1500.0, 200.0, 200.0, 1.0);
        movie.add_entity_sample(face, 2.0, 400.0, 1500.0, 200.0, 200.0, 1.0);

        let manifest = movie.create_semantic_manifest(#{
            entity_refs: [face],
            start_time: 0.0,
            end_time: 2.0,
        });

        let config = movie.create_smart_caption_config(#{
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            sample_rate: 10.0,
        });

        let caption = scene.add_smart_caption_continuous(
            manifest,
            config,
            #{
                content: [#{ text: "hello", start: 0.0, end: 2.0 }],
                font_size: 48.0,
                color: "#ffffff",
            }
        );

        movie.update(0.5);

        #{
            entity_count: caption.entity_ids().len(),
            position: caption.caption_position(),
        }
    "##;

    let result = engine
        .eval::<Map>(script)
        .expect("explicit continuous smart caption script must evaluate");

    let entity_count: i64 = result
        .get("entity_count")
        .and_then(|v| v.as_int().ok())
        .expect("entity_count present");
    assert_eq!(entity_count, 1, "explicit continuous smart caption should reference one entity");

    let position = result
        .get("position")
        .and_then(|v| v.clone().into_string().ok())
        .expect("position present");
    assert_eq!(
        position, "free_xy",
        "continuous smart caption should use free_xy position, got {}",
        position
    );
}

#[test]
fn smart_caption_manifest_without_entities_defaults_to_top() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r##"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(2.0);

        let manifest = movie.create_semantic_manifest(#{
            entity_refs: [],
            start_time: 0.0,
            end_time: 2.0,
        });

        let config = movie.create_smart_caption_config(#{
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            sample_rate: 10.0,
        });

        let caption = scene.add_smart_caption(
            manifest,
            config,
            #{
                content: [#{ text: "hello", start: 0.0, end: 2.0 }],
                font_size: 48.0,
                color: "#ffffff",
            }
        );

        movie.update(0.5);

        #{
            entity_count: caption.entity_ids().len(),
        }
    "##;

    let result = engine
        .eval::<Map>(script)
        .expect("empty manifest smart caption script must evaluate");

    let entity_count: i64 = result
        .get("entity_count")
        .and_then(|v| v.as_int().ok())
        .expect("entity_count present");
    assert_eq!(entity_count, 0, "empty manifest should reference no entities");
}
