//! Authoring mistakes must fail the script instead of rendering nothing.

use director_core::{
    scripting::{register_rhai_api, MovieHandle},
    DefaultAssetLoader,
};
use rhai::Engine;
use std::sync::Arc;

fn eval_movie(script: &str) -> Result<MovieHandle, Box<rhai::EvalAltResult>> {
    let mut engine = Engine::new();
    register_rhai_api(&mut engine, Arc::new(DefaultAssetLoader));
    engine.eval(script)
}

fn assert_err_contains(script: &str, needles: &[&str]) {
    let err = eval_movie(script).err().expect("script should fail");
    let msg = err.to_string();
    for needle in needles {
        assert!(
            msg.contains(needle),
            "error `{msg}` should contain `{needle}`"
        );
    }
}

#[test]
fn unknown_effect_errors() {
    assert_err_contains(
        r#"
let movie = new_director(64, 64, 30);
let scene = movie.add_scene(1.0);
let node = scene.add_box(#{ width: 10.0, height: 10.0 });
node.apply_effect("vignete", #{ intensity: 1.0 });
movie
"#,
        &["unknown effect", "vignete", "close matches", "vignette"],
    );
}

#[test]
fn unknown_transition_errors() {
    assert_err_contains(
        r#"
let movie = new_director(64, 64, 30);
let s1 = movie.add_scene(2.0);
let s2 = movie.add_scene(2.0);
movie.add_transition(s1, s2, "fad", 0.5, "linear");
movie
"#,
        &["unknown transition", "fad", "fade"],
    );
}

#[test]
fn unknown_effect_param_errors() {
    assert_err_contains(
        r#"
let movie = new_director(64, 64, 30);
let scene = movie.add_scene(1.0);
let node = scene.add_box(#{ width: 10.0, height: 10.0 });
node.apply_effect("vignette", #{ nope: 1.0 });
movie
"#,
        &["unknown parameter", "nope", "intensity"],
    );
}

#[test]
fn known_effect_map_applies() {
    let result = eval_movie(
        "\
let movie = new_director(64, 64, 30);
let scene = movie.add_scene(1.0);
let node = scene.add_box(#{ width: 10.0, height: 10.0, bg_color: \"#ffffff\" });
node.apply_effect(\"vignette\", #{ intensity: 0.4, radius: 0.3, feather: 0.2 });
movie
",
    );
    assert!(result.is_ok(), "known effect should apply: {:?}", result.err());
}

#[test]
fn missing_audio_errors() {
    assert_err_contains(
        r#"
let movie = new_director(64, 64, 30);
movie.add_audio("missing-no-such-file.wav");
movie
"#,
        &["audio not found", "missing-no-such-file.wav"],
    );
}

#[test]
fn missing_image_errors() {
    assert_err_contains(
        r#"
let movie = new_director(64, 64, 30);
let scene = movie.add_scene(1.0);
scene.add_image("missing-no-such-file.png");
movie
"#,
        &["image not found or not decodable", "missing-no-such-file.png"],
    );
}

#[test]
fn missing_svg_errors() {
    assert_err_contains(
        r#"
let movie = new_director(64, 64, 30);
let scene = movie.add_scene(1.0);
scene.add_svg("missing-no-such-file.svg");
movie
"#,
        &["svg not found", "missing-no-such-file.svg"],
    );
}
