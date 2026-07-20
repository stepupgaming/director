//! # L1 Motion Stdlib
//!
//! Token-efficient motion verbs on `Node` for agents and humans.
//! Implemented as thin wrappers over keyframe `animate` / text animators
//! (no separate eval path).
//!
//! ## Verbs
//! - `fade_in` / `fade_out`
//! - `pop_in`
//! - `fly_in_up` / `fly_in_down` / `fly_in_left` / `fly_in_right`
//! - `fly_out_up` / `fly_out_down` / `fly_out_left` / `fly_out_right`
//! - `pulse` / `pulse_on_beat`
//! - `typewriter` (TextNode glyph reveal)
//! - `ken_burns` (slow zoom + drift)
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
const DEFAULT_KEN_BURNS_ZOOM: f32 = 1.12;
const DEFAULT_KEN_BURNS_DRIFT: f32 = 24.0;

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

/// Pulse once per beat for `duration` seconds at `bpm`.
fn pulse_on_beat_impl(
    n: &mut crate::scene::SceneNode,
    bpm: f64,
    duration: f64,
    peak: f32,
) -> Result<(), String> {
    if bpm <= 0.0 || !bpm.is_finite() {
        return Err(format!("pulse_on_beat: bpm must be positive (got {bpm})"));
    }
    if duration < 0.0 {
        return Err(format!("pulse_on_beat: negative duration {duration}"));
    }
    let beat = 60.0 / bpm;
    let pulse_dur = (beat * 0.45).clamp(0.06, beat.max(0.06));
    let mut t = 0.0;
    // Inclusive of beat at t=0; stop before exceeding duration.
    while t < duration - 1e-9 {
        pulse_impl(n, pulse_dur, t, peak)?;
        t += beat;
        // Safety: pathological tiny beat
        if beat < 1e-6 {
            break;
        }
    }
    Ok(())
}

fn text_char_count(n: &mut crate::scene::SceneNode) -> usize {
    use crate::node::TextNode;
    if let Some(text) = n.element.as_any().downcast_ref::<TextNode>() {
        return text.get_total_char_count();
    }
    0
}

/// Reveal text glyphs left-to-right over `dur` seconds (TextNode only).
fn typewriter_impl(n: &mut crate::scene::SceneNode, dur: f64, delay: f64) -> Result<(), String> {
    if dur < 0.0 {
        return Err(format!("typewriter: negative duration {dur}"));
    }
    let chars = text_char_count(n);
    if chars == 0 {
        return Err(
            "typewriter: node has no text content (call on a Text node, not a layout box)".into(),
        );
    }
    // Optional pre-delay: hold full string invisible, then stagger-reveal.
    // Text animators start at local t=0; approximate delay by pushing a zero-opacity
    // hold on the node, then running glyph animators (they still start at 0 — so
    // for delay we offset via stagger start by using add_text_animator_full only
    // and a leading opacity hold on the whole node).
    if delay > 0.0 {
        hold_then(n, "opacity", 0.0, delay)?;
        apply_animate_prop(n, "opacity", 0.0, 1.0, 0.001, &EasingType::Linear)?;
    }
    let glyph_dur = (dur / chars as f64).clamp(0.02, dur.max(0.02));
    let stagger = if chars > 1 {
        ((dur - glyph_dur) / (chars as f64 - 1.0)).max(0.0) as f32
    } else {
        0.0
    };
    n.element.add_text_animator_full(
        0,
        chars,
        "opacity".into(),
        0.0,
        1.0,
        glyph_dur,
        "linear",
        stagger,
    );
    Ok(())
}

/// Slow zoom + directional drift (Ken Burns). Good on image/box full-bleeds.
fn ken_burns_impl(
    n: &mut crate::scene::SceneNode,
    dur: f64,
    zoom: f32,
    drift_x: f32,
    drift_y: f32,
) -> Result<(), String> {
    if dur < 0.0 {
        return Err(format!("ken_burns: negative duration {dur}"));
    }
    if zoom < 1.0 {
        return Err(format!("ken_burns: zoom must be >= 1.0 (got {zoom})"));
    }
    apply_animate_prop(n, "scale", 1.0, zoom, dur, &EasingType::Linear)?;
    apply_animate_prop(n, "translate_x", 0.0, drift_x, dur, &EasingType::Linear)?;
    apply_animate_prop(n, "translate_y", 0.0, drift_y, dur, &EasingType::Linear)?;
    Ok(())
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

    // ----- pulse_on_beat(bpm, duration [, peak]) -----
    engine.register_fn(
        "pulse_on_beat",
        |node: &mut NodeHandle, bpm: f64, duration: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "pulse_on_beat", |n| {
                pulse_on_beat_impl(n, bpm, duration, DEFAULT_PULSE_SCALE)
            })
        },
    );
    engine.register_fn(
        "pulse_on_beat",
        |node: &mut NodeHandle, bpm: f64, duration: f64, peak: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "pulse_on_beat", |n| {
                pulse_on_beat_impl(n, bpm, duration, peak as f32)
            })
        },
    );

    // ----- typewriter(dur [, delay]) — TextNode glyph reveal -----
    engine.register_fn(
        "typewriter",
        |node: &mut NodeHandle, dur: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "typewriter", |n| typewriter_impl(n, dur, 0.0))
        },
    );
    engine.register_fn(
        "typewriter",
        |node: &mut NodeHandle, dur: f64, delay: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "typewriter", |n| typewriter_impl(n, dur, delay))
        },
    );

    // ----- ken_burns(dur [, zoom [, drift_x, drift_y]]) -----
    engine.register_fn(
        "ken_burns",
        |node: &mut NodeHandle, dur: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "ken_burns", |n| {
                ken_burns_impl(n, dur, DEFAULT_KEN_BURNS_ZOOM, DEFAULT_KEN_BURNS_DRIFT, -DEFAULT_KEN_BURNS_DRIFT * 0.5)
            })
        },
    );
    engine.register_fn(
        "ken_burns",
        |node: &mut NodeHandle, dur: f64, zoom: f64| -> Result<(), Box<EvalAltResult>> {
            with_node(node, "ken_burns", |n| {
                ken_burns_impl(
                    n,
                    dur,
                    zoom as f32,
                    DEFAULT_KEN_BURNS_DRIFT,
                    -DEFAULT_KEN_BURNS_DRIFT * 0.5,
                )
            })
        },
    );
    engine.register_fn(
        "ken_burns",
        |node: &mut NodeHandle,
         dur: f64,
         zoom: f64,
         drift_x: f64,
         drift_y: f64|
         -> Result<(), Box<EvalAltResult>> {
            with_node(node, "ken_burns", |n| {
                ken_burns_impl(n, dur, zoom as f32, drift_x as f32, drift_y as f32)
            })
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

    #[test]
    fn typewriter_on_text_ok() {
        let movie = eval_movie(
            "let movie = new_director(400, 200, 30);\n\
             let scene = movie.add_scene(2.0);\n\
             let root = scene.add_box(#{ width: \"100%\", height: \"100%\", bg_color: \"#000000\" });\n\
             let t = root.add_text(#{ content: \"Hello\", size: 32.0, color: \"#ffffff\" });\n\
             t.typewriter(0.8);\n\
             movie",
        );
        assert!(movie.director.lock().unwrap().scene.iter_alive().count() >= 2);
    }

    #[test]
    fn typewriter_on_box_errors() {
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
             root.typewriter(0.5);\n\
             movie",
        ) {
            Ok(_) => panic!("expected typewriter error on box"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("typewriter"),
            "err={err}"
        );
    }

    #[test]
    fn pulse_on_beat_and_ken_burns() {
        let movie = eval_movie(
            "let movie = new_director(320, 240, 30);\n\
             let scene = movie.add_scene(2.0);\n\
             let root = scene.add_box(#{ width: \"100%\", height: \"100%\", bg_color: \"#111111\" });\n\
             root.ken_burns(2.0, 1.15, 10.0, -8.0);\n\
             let t = root.add_text(#{ content: \"beat\", size: 24.0, color: \"#ffffff\" });\n\
             t.pulse_on_beat(120.0, 1.0);\n\
             movie",
        );
        assert!(movie.director.lock().unwrap().timeline_duration() >= 2.0 - 1e-6);
    }
}
