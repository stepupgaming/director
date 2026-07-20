//! # L1 Motion Stdlib
//!
//! Token-efficient motion verbs on `Node` for agents and humans.
//! Implemented as thin wrappers over keyframe `animate` (no new eval path).
//!
//! ## Verbs
//! - `fade_in` / `fade_out`
//! - `pop_in`
//! - `fly_in_up` / `fly_in_down` / `fly_in_left` / `fly_in_right`
//! - `fly_out_up` / `fly_out_down` / `fly_out_left` / `fly_out_right`
//! - `pulse`
//!
//! Each verb has overloads: `(dur)`, `(dur, delay)`, and where useful
//! `(dur, delay, distance)` for fly verbs.
//!
//! ## Layering
//! - **L0** — raw `animate` / engine
//! - **L1** — this module
//! - **L2** — templates + `register_composition`
//! - **L3** — recipe scripts under `examples/recipes/`

use crate::animation::EasingType;
use crate::scripting::types::NodeHandle;
use rhai::{Engine, EvalAltResult};

use super::animation::{apply_animate_prop, rhai_err};

const DEFAULT_FLY_PX: f32 = 72.0;
const DEFAULT_POP_SCALE: f32 = 0.82;
const DEFAULT_PULSE_SCALE: f32 = 1.08;

fn with_node<F>(node: &mut NodeHandle, op: &str, f: F) -> Result<(), Box<EvalAltResult>>
where
    F: FnOnce(&mut crate::scene::SceneNode) -> Result<(), String>,
{
    let mut d = node.lock_director()?;
    let Some(n) = d.scene.get_node_mut(node.id) else {
        return Err(rhai_err(format!("{op}: node {} not found", node.id)));
    };
    f(n).map_err(rhai_err)
}

fn hold_then(
    n: &mut crate::scene::SceneNode,
    prop: &str,
    value: f32,
    delay: f64,
) -> Result<(), String> {
    if delay > 0.0 {
        apply_animate_prop(n, prop, value, value, delay, &EasingType::Linear)?;
    }
    Ok(())
}

fn fade_in_impl(n: &mut crate::scene::SceneNode, dur: f64, delay: f64) -> Result<(), String> {
    if dur < 0.0 {
        return Err(format!("fade_in: negative duration {dur}"));
    }
    hold_then(n, "opacity", 0.0, delay)?;
    apply_animate_prop(n, "opacity", 0.0, 1.0, dur, &EasingType::EaseOut)
}

fn fade_out_impl(n: &mut crate::scene::SceneNode, dur: f64, delay: f64) -> Result<(), String> {
    if dur < 0.0 {
        return Err(format!("fade_out: negative duration {dur}"));
    }
    hold_then(n, "opacity", 1.0, delay)?;
    apply_animate_prop(n, "opacity", 1.0, 0.0, dur, &EasingType::EaseIn)
}

fn pop_in_impl(n: &mut crate::scene::SceneNode, dur: f64, delay: f64) -> Result<(), String> {
    if dur < 0.0 {
        return Err(format!("pop_in: negative duration {dur}"));
    }
    hold_then(n, "opacity", 0.0, delay)?;
    hold_then(n, "scale", DEFAULT_POP_SCALE, delay)?;
    apply_animate_prop(n, "opacity", 0.0, 1.0, dur, &EasingType::EaseOut)?;
    apply_animate_prop(
        n,
        "scale",
        DEFAULT_POP_SCALE,
        1.0,
        dur,
        &EasingType::BounceOut,
    )
}

fn fly_in_impl(
    n: &mut crate::scene::SceneNode,
    axis: &str,
    from: f32,
    dur: f64,
    delay: f64,
) -> Result<(), String> {
    if dur < 0.0 {
        return Err(format!("fly_in: negative duration {dur}"));
    }
    hold_then(n, "opacity", 0.0, delay)?;
    hold_then(n, axis, from, delay)?;
    apply_animate_prop(n, "opacity", 0.0, 1.0, dur, &EasingType::EaseOut)?;
    apply_animate_prop(n, axis, from, 0.0, dur, &EasingType::EaseOut)
}

fn fly_out_impl(
    n: &mut crate::scene::SceneNode,
    axis: &str,
    to: f32,
    dur: f64,
    delay: f64,
) -> Result<(), String> {
    if dur < 0.0 {
        return Err(format!("fly_out: negative duration {dur}"));
    }
    hold_then(n, "opacity", 1.0, delay)?;
    hold_then(n, axis, 0.0, delay)?;
    apply_animate_prop(n, "opacity", 1.0, 0.0, dur, &EasingType::EaseIn)?;
    apply_animate_prop(n, axis, 0.0, to, dur, &EasingType::EaseIn)
}

fn pulse_impl(
    n: &mut crate::scene::SceneNode,
    dur: f64,
    delay: f64,
    peak: f32,
) -> Result<(), String> {
    if dur < 0.0 {
        return Err(format!("pulse: negative duration {dur}"));
    }
    let half = dur * 0.5;
    hold_then(n, "scale", 1.0, delay)?;
    apply_animate_prop(n, "scale", 1.0, peak, half, &EasingType::EaseOut)?;
    apply_animate_prop(n, "scale", peak, 1.0, half, &EasingType::EaseIn)
}

/// Register L1 motion verbs on `Node`.
pub fn register(engine: &mut Engine) {
    // ----- fade_in -----
    engine.register_fn(
        "fade_in",
        |node: &mut NodeHandle, dur: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "fade_in", |n| fade_in_impl(n, dur, 0.0))
        },
    );
    engine.register_fn(
        "fade_in",
        |node: &mut NodeHandle, dur: f64, delay: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "fade_in", |n| fade_in_impl(n, dur, delay))
        },
    );

    // ----- fade_out -----
    engine.register_fn(
        "fade_out",
        |node: &mut NodeHandle, dur: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "fade_out", |n| fade_out_impl(n, dur, 0.0))
        },
    );
    engine.register_fn(
        "fade_out",
        |node: &mut NodeHandle, dur: f64, delay: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "fade_out", |n| fade_out_impl(n, dur, delay))
        },
    );

    // ----- pop_in -----
    engine.register_fn(
        "pop_in",
        |node: &mut NodeHandle, dur: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "pop_in", |n| pop_in_impl(n, dur, 0.0))
        },
    );
    engine.register_fn(
        "pop_in",
        |node: &mut NodeHandle, dur: f64, delay: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "pop_in", |n| pop_in_impl(n, dur, delay))
        },
    );

    // ----- fly_in_* -----
    macro_rules! fly_in {
        ($name:literal, $axis:literal, $sign:expr) => {
            engine.register_fn(
                $name,
                |node: &mut NodeHandle, dur: f64| -> Result<(), Box<EvalAltResult>> {
                    with_node(node, $name, |n| {
                        fly_in_impl(n, $axis, $sign * DEFAULT_FLY_PX, dur, 0.0)
                    })
                },
            );
            engine.register_fn(
                $name,
                |node: &mut NodeHandle, dur: f64, delay: f64| -> Result<(), Box<EvalAltResult>> {
                    with_node(node, $name, |n| {
                        fly_in_impl(n, $axis, $sign * DEFAULT_FLY_PX, dur, delay)
                    })
                },
            );
            engine.register_fn(
                $name,
                |node: &mut NodeHandle,
                 dur: f64,
                 delay: f64,
                 distance: f64|
                 -> Result<(), Box<EvalAltResult>> {
                    with_node(node, $name, |n| {
                        fly_in_impl(n, $axis, $sign * distance as f32, dur, delay)
                    })
                },
            );
        };
    }
    fly_in!("fly_in_up", "translate_y", 1.0);
    fly_in!("fly_in_down", "translate_y", -1.0);
    fly_in!("fly_in_left", "translate_x", 1.0);
    fly_in!("fly_in_right", "translate_x", -1.0);

    // ----- fly_out_* -----
    macro_rules! fly_out {
        ($name:literal, $axis:literal, $sign:expr) => {
            engine.register_fn(
                $name,
                |node: &mut NodeHandle, dur: f64| -> Result<(), Box<EvalAltResult>> {
                    with_node(node, $name, |n| {
                        fly_out_impl(n, $axis, $sign * DEFAULT_FLY_PX, dur, 0.0)
                    })
                },
            );
            engine.register_fn(
                $name,
                |node: &mut NodeHandle, dur: f64, delay: f64| -> Result<(), Box<EvalAltResult>> {
                    with_node(node, $name, |n| {
                        fly_out_impl(n, $axis, $sign * DEFAULT_FLY_PX, dur, delay)
                    })
                },
            );
            engine.register_fn(
                $name,
                |node: &mut NodeHandle,
                 dur: f64,
                 delay: f64,
                 distance: f64|
                 -> Result<(), Box<EvalAltResult>> {
                    with_node(node, $name, |n| {
                        fly_out_impl(n, $axis, $sign * distance as f32, dur, delay)
                    })
                },
            );
        };
    }
    fly_out!("fly_out_up", "translate_y", -1.0);
    fly_out!("fly_out_down", "translate_y", 1.0);
    fly_out!("fly_out_left", "translate_x", -1.0);
    fly_out!("fly_out_right", "translate_x", 1.0);

    // ----- pulse -----
    engine.register_fn(
        "pulse",
        |node: &mut NodeHandle, dur: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "pulse", |n| {
                pulse_impl(n, dur, 0.0, DEFAULT_PULSE_SCALE)
            })
        },
    );
    engine.register_fn(
        "pulse",
        |node: &mut NodeHandle, dur: f64, delay: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "pulse", |n| {
                pulse_impl(n, dur, delay, DEFAULT_PULSE_SCALE)
            })
        },
    );
    engine.register_fn(
        "pulse",
        |node: &mut NodeHandle, dur: f64, delay: f64, peak: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "pulse", |n| pulse_impl(n, dur, delay, peak as f32))
        },
    );
}

// Re-export apply helpers visibility: animation module must expose apply_animate_prop + rhai_err
// as pub(crate) — see animation.rs.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::register_rhai_api_with_default_mode;
    use crate::scripting::MovieHandle;
    use crate::video_wrapper::RenderMode;
    use crate::DefaultAssetLoader;
    use rhai::Engine;
    use std::sync::Arc;

    fn eval_movie(script: &str) -> MovieHandle {
        let mut engine = Engine::new();
        register_rhai_api_with_default_mode(
            &mut engine,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
        );
        engine.eval::<MovieHandle>(script).expect("script")
    }

    #[test]
    fn fade_in_sets_opacity_keyframes() {
        // Avoid #hex inside r#"..."# (terminates the raw string).
        let movie = eval_movie(
            "let movie = new_director(100, 100, 30);\n\
             let scene = movie.add_scene(2.0);\n\
             let root = scene.add_box(#{ width: \"100%\", height: \"100%\", bg_color: \"#000000\" });\n\
             let t = root.add_text(#{ content: \"Hi\", size: 20.0, color: \"#ffffff\" });\n\
             t.fade_in(0.5);\n\
             movie",
        );
        let d = movie.director.lock().unwrap();
        assert!(d.scene.iter_alive().count() >= 2);
    }

    #[test]
    fn pop_in_and_fly_in_compose() {
        let movie = eval_movie(
            "let movie = new_director(320, 240, 30);\n\
             let scene = movie.add_scene(3.0);\n\
             let root = scene.add_box(#{ width: \"100%\", height: \"100%\",\n\
                 justify_content: \"center\", align_items: \"center\",\n\
                 bg_color: \"#111111\" });\n\
             let a = root.add_text(#{ content: \"A\", size: 32.0, color: \"#ffffff\" });\n\
             a.pop_in(0.4);\n\
             let b = root.add_text(#{ content: \"B\", size: 28.0, color: \"#00ffff\" });\n\
             b.fly_in_up(0.5, 0.2);\n\
             b.pulse(0.3, 0.8);\n\
             movie",
        );
        let d = movie.director.lock().unwrap();
        assert!(d.timeline_duration() >= 3.0 - 1e-6);
    }

    #[test]
    fn negative_duration_errors() {
        let mut engine = Engine::new();
        register_rhai_api_with_default_mode(
            &mut engine,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
        );
        let err = match engine.eval::<MovieHandle>(
            "let movie = new_director(100, 100, 30);\n\
             let scene = movie.add_scene(1.0);\n\
             let root = scene.add_box(#{ width: 50.0, height: 50.0 });\n\
             root.fade_in(-1.0);\n\
             movie",
        ) {
            Ok(_) => panic!("expected negative duration error"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("negative duration"),
            "err={err}"
        );
    }
}
