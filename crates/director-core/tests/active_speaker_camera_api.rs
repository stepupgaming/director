//! Runtime test for the ActiveSpeakerCamera Rhai API.
//! Exercises `add_active_speaker_camera` from a Rhai script.

use director_core::scripting::register_rhai_api;
use director_core::DefaultAssetLoader;
use rhai::{Engine, Map};
use std::sync::Arc;

#[test]
fn active_speaker_camera_from_single_group_references_entity() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r##"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(5.0);

        let speaker = movie.create_entity("speaker", "mara");
        movie.add_entity_sample(speaker, 0.0, 400.0, 500.0, 200.0, 200.0, 1.0);
        movie.add_entity_sample(speaker, 5.0, 600.0, 700.0, 200.0, 200.0, 1.0);

        let camera = scene.add_active_speaker_camera(
            "assets/dummy.mp4",
            #{
                entity_id: speaker,
                start_time: 0.0,
                end_time: 5.0,
                framing: "medium",
                sample_rate: 10.0,
                smoothing_window: 5,
            }
        );

        movie.update(0.5);

        #{
            entity_count: camera.entity_ids().len(),
            first_entity: if camera.entity_ids().len() > 0 { camera.entity_ids()[0] } else { "" },
        }
    "##;

    let result = engine
        .eval::<Map>(script)
        .expect("active speaker camera script must evaluate");

    let entity_count: i64 = result
        .get("entity_count")
        .and_then(|v| v.as_int().ok())
        .expect("entity_count present");
    assert_eq!(
        entity_count, 1,
        "active speaker camera should reference one entity"
    );

    let first_entity = result
        .get("first_entity")
        .and_then(|v| v.clone().into_string().ok())
        .unwrap_or_default();
    assert!(!first_entity.is_empty(), "first entity UUID should be present");
}

#[test]
fn active_speaker_camera_from_multiple_groups_references_entities() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r##"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(5.0);

        let speaker_a = movie.create_entity("speaker", "mara");
        let speaker_b = movie.create_entity("speaker", "alex");
        movie.add_entity_sample(speaker_a, 0.0, 400.0, 500.0, 200.0, 200.0, 1.0);
        movie.add_entity_sample(speaker_b, 2.0, 600.0, 700.0, 200.0, 200.0, 1.0);

        let camera = scene.add_active_speaker_camera(
            "assets/dummy.mp4",
            #{
                groups: [
                    #{ entity_id: speaker_a, start_time: 0.0, end_time: 2.0, framing: "close_up" },
                    #{ entity_id: speaker_b, start_time: 2.0, end_time: 5.0, framing: "wide" },
                ],
                sample_rate: 10.0,
                smoothing_window: 5,
            }
        );

        movie.update(0.5);

        #{
            entity_count: camera.entity_ids().len(),
        }
    "##;

    let result = engine
        .eval::<Map>(script)
        .expect("active speaker camera groups script must evaluate");

    let entity_count: i64 = result
        .get("entity_count")
        .and_then(|v| v.as_int().ok())
        .expect("entity_count present");
    assert_eq!(
        entity_count, 2,
        "active speaker camera should reference two entities"
    );
}

#[test]
fn active_speaker_camera_requires_continuity_group() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r##"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(5.0);

        let camera = scene.add_active_speaker_camera(
            "assets/dummy.mp4",
            #{
                sample_rate: 10.0,
            }
        );

        #{
            entity_count: camera.entity_ids().len(),
        }
    "##;

    let result = engine.eval::<Map>(script);
    assert!(result.is_err(), "should fail without continuity groups");
}

#[test]
fn active_speaker_camera_creates_node_with_trajectory() {
    use director_core::element::Element;
    use director_core::node::ActiveSpeakerCamera;

    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r##"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(5.0);

        let speaker = movie.create_entity("speaker", "mara");
        movie.add_entity_sample(speaker, 0.0, 400.0, 500.0, 200.0, 200.0, 1.0);
        movie.add_entity_sample(speaker, 5.0, 600.0, 700.0, 200.0, 200.0, 1.0);

        let camera = scene.add_active_speaker_camera(
            "assets/dummy.mp4",
            #{
                entity_id: speaker,
                start_time: 0.0,
                end_time: 5.0,
                framing: "medium",
                sample_rate: 10.0,
                smoothing_window: 5,
            }
        );

        movie.update(0.5);

        camera
    "##;

    let camera = engine
        .eval::<director_core::scripting::types::NodeHandle>(script)
        .expect("active speaker camera script must evaluate");

    let d = camera.lock_director().expect("lock director");
    let node = d.scene.get_node(camera.id).expect("node exists");
    let camera_node = node
        .element
        .as_any()
        .downcast_ref::<ActiveSpeakerCamera>()
        .expect("node should be an ActiveSpeakerCamera");

    assert!(
        !camera_node.trajectory().is_empty(),
        "trajectory should have samples when valid entity tracks exist"
    );
}
