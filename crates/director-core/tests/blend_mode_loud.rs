//! Unknown blend modes must fail the script.

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

#[test]
fn unknown_blend_mode_errors() {
    let err = eval_movie(
        r#"
let movie = new_director(64, 64, 30);
let scene = movie.add_scene(1.0);
let node = scene.add_box(#{ width: 10.0, height: 10.0 });
node.set_blend_mode("nope");
movie
"#,
    )
    .err()
    .expect("script should fail");
    let msg = err.to_string();
    assert!(
        msg.contains("unknown blend mode"),
        "error `{msg}` should contain `unknown blend mode`"
    );
    assert!(
        msg.contains("src_over"),
        "error `{msg}` should contain `src_over`"
    );
}

#[test]
fn multiply_blend_mode_succeeds() {
    let result = eval_movie(
        r#"
let movie = new_director(64, 64, 30);
let scene = movie.add_scene(1.0);
let node = scene.add_box(#{ width: 10.0, height: 10.0 });
node.set_blend_mode("multiply");
movie
"#,
    );
    if let Err(err) = result {
        panic!("{err}");
    }
}
