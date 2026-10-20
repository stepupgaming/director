//! Runtime test for the StylePolicy Rhai API.
//! Exercises `style_policy`, `set_style_policy`, `score_style_policy`, and
//! `style_policy_score` from a Rhai script.

use director_core::scripting::register_rhai_api;
use director_core::DefaultAssetLoader;
use rhai::{Array, Engine, Map};
use std::sync::Arc;

#[test]
fn style_policy_create_and_score_work_from_rhai() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(2.0);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let text = root.add_text(#{ content: "Huge text", size: 250.0 });

        let policy = style_policy(#{
            typography: #{
                max_font_size: 120.0,
                weight: 1.0,
            },
            motion: #{
                avg_motion_displacement_threshold: 0.0,
            },
        });
        movie.set_style_policy(policy);

        let score = movie.score_style_policy(0.0, 2.0, 10);
        let compile_score = movie.style_policy_score();

        #{
            score: score,
            compile_score: compile_score,
        }
    "#;

    let result = engine
        .eval::<Map>(script)
        .expect("style policy script must evaluate");

    let score: Map = result
        .get("score")
        .and_then(|v| v.clone().try_cast::<Map>())
        .expect("score must be a map");

    let violations: Array = score
        .get("violations")
        .and_then(|v| v.clone().try_cast::<Array>())
        .expect("violations must be an array");
    let has_max_font_size = violations.iter().any(|v| {
        v.clone()
            .try_cast::<Map>()
            .map(|m| {
                m.get("rule")
                    .and_then(|t| t.clone().into_string().ok())
                    == Some("typography.max_font_size".to_string())
            })
            .unwrap_or(false)
    });
    assert!(
        has_max_font_size,
        "should detect typography.max_font_size violation"
    );

    let compile_score: Map = result
        .get("compile_score")
        .and_then(|v| v.clone().try_cast::<Map>())
        .expect("compile_score must be a map");
    let compile_violations: Array = compile_score
        .get("violations")
        .and_then(|v| v.clone().try_cast::<Array>())
        .expect("compile violations must be an array");
    let compile_has_max_font_size = compile_violations.iter().any(|v| {
        v.clone()
            .try_cast::<Map>()
            .map(|m| {
                m.get("rule")
                    .and_then(|t| t.clone().into_string().ok())
                    == Some("typography.max_font_size".to_string())
            })
            .unwrap_or(false)
    });
    assert!(
        compile_has_max_font_size,
        "compile score should also contain typography.max_font_size violation"
    );
}

#[test]
fn style_policy_score_and_violations_available_after_compile() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(2.0);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let text = root.add_text(#{ content: "Huge text", size: 250.0 });

        let policy = style_policy(#{
            typography: #{
                max_font_size: 120.0,
                weight: 1.0,
            },
            motion: #{
                avg_motion_displacement_threshold: 0.0,
            },
        });
        movie.set_style_policy(policy);

        movie.compile();

        let score = movie.style_policy_score();
        let violations = movie.style_policy_violations();

        #{
            score: score,
            violations: violations,
        }
    "#;

    let result = engine
        .eval::<Map>(script)
        .expect("compile-time style policy script must evaluate");

    let _score: Map = result
        .get("score")
        .and_then(|v| v.clone().try_cast::<Map>())
        .expect("score must be a map");

    let violations: Array = result
        .get("violations")
        .and_then(|v| v.clone().try_cast::<Array>())
        .expect("violations must be an array");
    assert!(!violations.is_empty(), "should have violations after compile");

    let has_max_font_size = violations.iter().any(|v| {
        v.clone()
            .try_cast::<Map>()
            .map(|m| {
                m.get("rule")
                    .and_then(|t| t.clone().into_string().ok())
                    == Some("typography.max_font_size".to_string())
            })
            .unwrap_or(false)
    });
    assert!(
        has_max_font_size,
        "compile-time violations should include typography.max_font_size"
    );
}

#[test]
fn style_policy_empty_scene_scores_perfectly_from_rhai() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(2.0);

        let policy = style_policy(#{
            typography: #{
                max_font_size: 120.0,
            },
            motion: #{
                avg_motion_displacement_threshold: 0.0,
            },
        });
        movie.set_style_policy(policy);

        let score = movie.score_style_policy(0.0, 2.0, 10);
        score
    "#;

    let score = engine
        .eval::<Map>(script)
        .expect("empty scene style policy script must evaluate");
    let total_score: f64 = score
        .get("total_score")
        .and_then(|v| v.as_float().ok())
        .expect("total_score present");
    assert_eq!(total_score, 1.0, "empty scene should score perfectly");

    let violations: Array = score
        .get("violations")
        .and_then(|v| v.clone().try_cast::<Array>())
        .expect("violations must be an array");
    assert!(violations.is_empty(), "empty scene should have no violations");
}

#[test]
fn style_policy_samples_configurable_from_rhai() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(2.0);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let box = root.add_box(#{ width: 100.0, height: 100.0 });

        // Animate the box across the screen very quickly.
        box.animate("x", 0.0, 5000.0, 1.0, "linear");

        let policy = style_policy(#{
            samples: 20,
            motion: #{
                max_pixels_per_second: 1000.0,
                weight: 1.0,
            },
        });
        movie.set_style_policy(policy);

        movie.compile();
        let score = movie.style_policy_score();
        score
    "#;

    let score = engine
        .eval::<Map>(script)
        .expect("samples config script must evaluate");
    let total_score: f64 = score
        .get("total_score")
        .and_then(|v| v.as_float().ok())
        .expect("total_score present");
    assert!(total_score < 1.0, "fast motion should produce a sub-perfect score");

    let violations: Array = score
        .get("violations")
        .and_then(|v| v.clone().try_cast::<Array>())
        .expect("violations must be an array");
    let has_motion = violations.iter().any(|v| {
        v.clone()
            .try_cast::<Map>()
            .map(|m| {
                m.get("rule")
                    .and_then(|t| t.clone().into_string().ok())
                    .map(|s| s.starts_with("motion"))
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    });
    assert!(has_motion, "should detect a motion violation with custom samples");
}

#[test]
fn style_policy_motion_violation_detected_from_rhai() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(2.0);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let box = root.add_box(#{ width: 100.0, height: 100.0 });

        // Animate the box across the screen very quickly.
        box.animate("x", 0.0, 5000.0, 1.0, "linear");

        let policy = style_policy(#{
            motion: #{
                max_pixels_per_second: 1000.0,
                weight: 1.0,
            },
        });
        movie.set_style_policy(policy);

        let score = movie.score_style_policy(0.0, 2.0, 10);
        score
    "#;

    let score = engine
        .eval::<Map>(script)
        .expect("motion style policy script must evaluate");
    let total_score: f64 = score
        .get("total_score")
        .and_then(|v| v.as_float().ok())
        .expect("total_score present");
    assert!(total_score < 1.0, "fast motion should produce a sub-perfect score");

    let violations: Array = score
        .get("violations")
        .and_then(|v| v.clone().try_cast::<Array>())
        .expect("violations must be an array");
    let has_motion = violations.iter().any(|v| {
        v.clone()
            .try_cast::<Map>()
            .map(|m| {
                m.get("rule")
                    .and_then(|t| t.clone().into_string().ok())
                    .map(|s| s.starts_with("motion"))
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    });
    assert!(has_motion, "should detect a motion violation");
}
