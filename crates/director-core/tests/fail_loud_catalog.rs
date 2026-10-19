//! Every catalog effect name must apply through the Rhai `apply_effect` entry point.
//!
//! `Engine::gen_fn_signatures` (Rhai `metadata`) lists the live registrations.
//! `RHAI_FUNCTION_CATALOG` is a short helper index and is not rewritten here.

use director_core::{
    api_catalog::{catalog_effect_names, EFFECT_CATALOG, RHAI_FUNCTION_CATALOG},
    scripting::{register_rhai_api, MovieHandle},
    DefaultAssetLoader,
};
use rhai::Engine;
use std::sync::Arc;

const NUMBER_EFFECTS: &[&str] = &[
    "blur",
    "contrast",
    "brightness",
    "saturation",
    "exposure",
    "hue_rotate",
    "temperature",
];

fn effect_call(name: &str) -> String {
    if name == "shader" {
        return r#"n.apply_effect("shader", #{ code: "half4 main(float2 p) { return half4(0.0); }" })"#
            .to_string();
    }
    if NUMBER_EFFECTS.contains(&name) {
        return format!(r#"n.apply_effect("{name}", 1.0)"#);
    }
    let entry = EFFECT_CATALOG
        .iter()
        .find(|entry| entry.name == name)
        .expect("catalog name");
    if entry.params.is_empty() {
        format!(r#"n.apply_effect("{name}")"#)
    } else {
        format!(r#"n.apply_effect("{name}", #{{}})"#)
    }
}

#[test]
fn catalog_effect_names_apply() {
    let mut engine = Engine::new();
    register_rhai_api(&mut engine, Arc::new(DefaultAssetLoader));

    let signatures = engine.gen_fn_signatures(false);
    assert!(
        signatures.iter().any(|sig| sig.contains("apply_effect")),
        "registered signatures should include apply_effect"
    );
    assert!(
        signatures.len() > RHAI_FUNCTION_CATALOG.len(),
        "live Rhai registrations ({}) exceed the short function catalog ({})",
        signatures.len(),
        RHAI_FUNCTION_CATALOG.len()
    );

    for name in catalog_effect_names() {
        let call = effect_call(&name);
        let script = [
            "let movie = new_director(64, 64, 30);",
            "let scene = movie.add_scene(0.2);",
            "let n = scene.add_box(#{ width: 10.0, height: 10.0, bg_color: \"#fff\" });",
            &(call + ";"),
            "movie",
        ]
        .join("\n");
        let result = engine.eval::<MovieHandle>(&script);
        assert!(
            result.is_ok(),
            "apply_effect('{name}') should succeed: {:?}",
            result.err()
        );
    }
}
