//! Runtime test for the render manifest verification/repair API.
//! Exercises `build_render_manifest`, `manifest.verify()`, and `manifest.repair()`
//! from a Rhai script.

use director_core::scripting::register_rhai_api;
use director_core::DefaultAssetLoader;
use rhai::{Array, Engine, Map};
use std::sync::Arc;

#[test]
fn manifest_verify_and_repair_work_from_rhai() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(2.0);
        // A full-width box at the top edge extends into the TikTok top safe zone.
        let root = scene.add_box(#{ width: 1080.0, height: 200.0 });

        let manifest = build_render_manifest(movie, 0.0);
        let issues = manifest.verify();
        let result = manifest.repair();

        #{
            issue_count: issues.len(),
            first_issue_type: if issues.len() > 0 { issues[0].issue_type } else { "" },
            repair: result,
        }
    "#;

    let result = engine
        .eval::<Map>(script)
        .expect("manifest verify/repair script must evaluate");

    let issue_count: i64 = result
        .get("issue_count")
        .and_then(|v| v.as_int().ok())
        .expect("issue_count present");
    assert!(issue_count > 0, "should detect at least one verification issue");

    let first_issue_type = result
        .get("first_issue_type")
        .and_then(|v| v.clone().into_string().ok())
        .unwrap_or_default();
    assert_eq!(
        first_issue_type, "safe_zone_violation",
        "first issue should be a safe-zone violation"
    );

    let repair: Map = result
        .get("repair")
        .and_then(|v| v.clone().try_cast::<Map>())
        .expect("repair must be a map");

    let affected_nodes: Array = repair
        .get("affected_nodes")
        .and_then(|v| v.clone().try_cast::<Array>())
        .expect("affected_nodes must be an array");
    assert!(!affected_nodes.is_empty(), "repair should affect at least one node");

    let queued_issue_count: i64 = repair
        .get("queued_issue_count")
        .and_then(|v| v.as_int().ok())
        .expect("queued_issue_count present");
    assert_eq!(
        queued_issue_count, issue_count,
        "repair should queue the same number of issues that verify returned"
    );
}

#[test]
fn manifest_verify_with_config_works_from_rhai() {
    let mut engine = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut engine, loader.clone());

    let script = r#"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(2.0);
        let root = scene.add_box(#{ width: 1080.0, height: 200.0 });

        let manifest = build_render_manifest(movie, 0.0);
        // Use a non-existent safe-zone preset so verification skips safe-zone checks.
        let issues = manifest.verify(#{ safe_zone_preset: "none" });
        let result = manifest.repair(#{ safe_zone_preset: "none" });

        #{
            issues: issues,
            repair: result,
        }
    "#;

    let result = engine
        .eval::<Map>(script)
        .expect("manifest verify with config script must evaluate");

    let issues: Array = result
        .get("issues")
        .and_then(|v| v.clone().try_cast::<Array>())
        .expect("issues must be an array");

    let has_static_stretch = issues.iter().any(|v| {
        v.clone()
            .try_cast::<Map>()
            .map(|m| {
                m.get("issue_type")
                    .and_then(|t| t.clone().into_string().ok())
                    == Some("static_stretch".to_string())
            })
            .unwrap_or(false)
    });
    assert!(has_static_stretch, "should detect a static-stretch issue");

    let has_safe_zone = issues.iter().any(|v| {
        v.clone()
            .try_cast::<Map>()
            .map(|m| {
                m.get("issue_type")
                    .and_then(|t| t.clone().into_string().ok())
                    == Some("safe_zone_violation".to_string())
            })
            .unwrap_or(false)
    });
    assert!(!has_safe_zone, "should skip safe-zone violations with unknown preset");

    let repair: Map = result
        .get("repair")
        .and_then(|v| v.clone().try_cast::<Map>())
        .expect("repair must be a map");
    let queued_issue_count: i64 = repair
        .get("queued_issue_count")
        .and_then(|v| v.as_int().ok())
        .expect("queued_issue_count present");
    assert_eq!(queued_issue_count, issues.len() as i64);
}
