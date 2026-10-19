//! # Animation API
//!
//! Animation functions for Rhai scripts.
//!
//! ## Responsibilities
//! - **Keyframe Animation**: `animate` for property tweening
//! - **Spring Animation**: `spring` for physics-based motion
//! - **Path Animation**: `path_animate` for SVG path following
//! - **Text Animation**: `add_animator` for per-glyph animations
//! - **Instant Setters**: `set_blur` for immediate property changes

use crate::types::{PathAnimationState, PathMorphState};
use rhai::{Engine, EvalAltResult};
use skia_safe::Path;

use super::super::types::NodeHandle;
use super::super::utils::{parse_easing_result, parse_hex_color, parse_spring_config};


const TRANSFORM_PROPS: &str = "scale, scale_x, scale_y, rotation, skew_x, skew_y, translate_x/x, translate_y/y";

pub(crate) fn rhai_err(msg: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(msg.into().into(), rhai::Position::NONE))
}

/// Apply a keyframe segment to a transform or element property.
pub(crate) fn apply_animate_prop(
    n: &mut crate::scene::SceneNode,
    prop: &str,
    start: f32,
    end: f32,
    dur: f64,
    ease: &crate::animation::EasingType,
) -> Result<(), String> {
    if dur < 0.0 {
        return Err(format!("animate: negative duration {dur} is not allowed"));
    }
    match prop {
        "scale" => {
            n.transform
                .scale_x
                .add_segment(start, end, dur, ease.clone());
            n.transform
                .scale_y
                .add_segment(start, end, dur, ease.clone());
            Ok(())
        }
        "scale_x" => {
            n.transform.scale_x.add_segment(start, end, dur, ease.clone());
            Ok(())
        }
        "scale_y" => {
            n.transform.scale_y.add_segment(start, end, dur, ease.clone());
            Ok(())
        }
        "rotation" => {
            n.transform.rotation.add_segment(start, end, dur, ease.clone());
            Ok(())
        }
        "skew_x" => {
            n.transform.skew_x.add_segment(start, end, dur, ease.clone());
            Ok(())
        }
        "skew_y" => {
            n.transform.skew_y.add_segment(start, end, dur, ease.clone());
            Ok(())
        }
        "translate_x" | "x" => {
            n.transform
                .translate_x
                .add_segment(start, end, dur, ease.clone());
            Ok(())
        }
        "translate_y" | "y" => {
            n.transform
                .translate_y
                .add_segment(start, end, dur, ease.clone());
            Ok(())
        }
        other => n
            .element
            .animate_property(other, start, end, dur, &format_ease_for_element(ease))
            .map_err(|e| {
                if e.contains("unknown property") {
                    format!(
                        "{e}; transform props: {TRANSFORM_PROPS}"
                    )
                } else {
                    e
                }
            }),
    }
}

/// Element animate_property still takes easing as &str; re-serialize known types.
fn format_ease_for_element(ease: &crate::animation::EasingType) -> String {
    use crate::animation::EasingType::*;
    match ease {
        Linear => "linear".into(),
        EaseIn => "ease_in".into(),
        EaseOut => "ease_out".into(),
        EaseInOut => "ease_in_out".into(),
        BounceOut => "bounce_out".into(),
        BounceIn => "bounce_in".into(),
        BounceInOut => "bounce_in_out".into(),
        ElasticOut => "elastic_out".into(),
        ElasticIn => "elastic_in".into(),
        ElasticInOut => "elastic_in_out".into(),
        BackOut => "back_out".into(),
        BackIn => "back_in".into(),
        BackInOut => "back_in_out".into(),
        Hold => "hold".into(),
        CubicBezier { x1, y1, x2, y2 } => format!("cubic_bezier({x1},{y1},{x2},{y2})"),
        Expression { expr } => format!("expr({expr})"),
    }
}

fn apply_spring_prop(
    n: &mut crate::scene::SceneNode,
    prop: &str,
    start: Option<f32>,
    end: f32,
    conf: crate::animation::SpringConfig,
) -> Result<(), String> {
    match prop {
        "scale" => {
            if let Some(s) = start {
                n.transform.scale_x.add_spring_with_start(s, end, conf);
                n.transform.scale_y.add_spring_with_start(s, end, conf);
            } else {
                n.transform.scale_x.add_spring(end, conf);
                n.transform.scale_y.add_spring(end, conf);
            }
            Ok(())
        }
        "scale_x" => {
            if let Some(s) = start {
                n.transform.scale_x.add_spring_with_start(s, end, conf);
            } else {
                n.transform.scale_x.add_spring(end, conf);
            }
            Ok(())
        }
        "scale_y" => {
            if let Some(s) = start {
                n.transform.scale_y.add_spring_with_start(s, end, conf);
            } else {
                n.transform.scale_y.add_spring(end, conf);
            }
            Ok(())
        }
        "rotation" => {
            if let Some(s) = start {
                n.transform.rotation.add_spring_with_start(s, end, conf);
            } else {
                n.transform.rotation.add_spring(end, conf);
            }
            Ok(())
        }
        "skew_x" => {
            if let Some(s) = start {
                n.transform.skew_x.add_spring_with_start(s, end, conf);
            } else {
                n.transform.skew_x.add_spring(end, conf);
            }
            Ok(())
        }
        "skew_y" => {
            if let Some(s) = start {
                n.transform.skew_y.add_spring_with_start(s, end, conf);
            } else {
                n.transform.skew_y.add_spring(end, conf);
            }
            Ok(())
        }
        "translate_x" | "x" => {
            if let Some(s) = start {
                n.transform.translate_x.add_spring_with_start(s, end, conf);
            } else {
                n.transform.translate_x.add_spring(end, conf);
            }
            Ok(())
        }
        "translate_y" | "y" => {
            if let Some(s) = start {
                n.transform.translate_y.add_spring_with_start(s, end, conf);
            } else {
                n.transform.translate_y.add_spring(end, conf);
            }
            Ok(())
        }
        other => n.element.animate_property_spring(other, start, end, conf),
    }
}

/// Register animation-related Rhai functions.
pub fn register(engine: &mut Engine) {
    // ========== ANIMATE (with Delay) ==========
    engine.register_fn(
        "animate",
        |node: &mut NodeHandle,
         prop: &str,
         start: f64,
         end: f64,
         dur: f64,
         ease: &str,
         delay: f64|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!("animate: node {} not found", node.id)));
            };
            let ease_fn = parse_easing_result(ease).map_err(rhai_err)?;
            if delay > 0.0 {
                apply_animate_prop(
                    n,
                    prop,
                    start as f32,
                    start as f32,
                    delay,
                    &crate::animation::EasingType::Linear,
                )
                .map_err(rhai_err)?;
            }
            apply_animate_prop(n, prop, start as f32, end as f32, dur, &ease_fn).map_err(rhai_err)
        },
    );

    // ========== ANIMATE ==========
    engine.register_fn(
        "animate",
        |node: &mut NodeHandle, prop: &str, start: f64, end: f64, dur: f64, ease: &str|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!("animate: node {} not found", node.id)));
            };
            let ease_fn = parse_easing_result(ease).map_err(rhai_err)?;
            apply_animate_prop(n, prop, start as f32, end as f32, dur, &ease_fn).map_err(rhai_err)
        },
    );

    // Integer Overload for animate
    engine.register_fn(
        "animate",
        |node: &mut NodeHandle, prop: &str, start: i64, end: i64, dur: f64, ease: &str|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!("animate: node {} not found", node.id)));
            };
            let ease_fn = parse_easing_result(ease).map_err(rhai_err)?;
            apply_animate_prop(n, prop, start as f32, end as f32, dur, &ease_fn).map_err(rhai_err)
        },
    );

    // Spring animation via animate (shorthand: animate(prop, end, config))
    engine.register_fn(
        "animate",
        |node: &mut NodeHandle, prop: &str, end: f64, config: rhai::Map|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let spring_conf = parse_spring_config(&config);
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!("animate: node {} not found", node.id)));
            };
            apply_spring_prop(n, prop, None, end as f32, spring_conf).map_err(rhai_err)
        },
    );

    // ========== SPRING ==========
    engine.register_fn(
        "spring",
        |node: &mut NodeHandle, prop: &str, start: f64, end: f64, config: rhai::Map|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let spring_conf = parse_spring_config(&config);
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!("spring: node {} not found", node.id)));
            };
            apply_spring_prop(n, prop, Some(start as f32), end as f32, spring_conf).map_err(rhai_err)
        },
    );

    // ========== PATH_ANIMATE ==========
    engine.register_fn(
        "path_animate",
        |node: &mut NodeHandle, svg: &str, dur: f64, ease: &str|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!("path_animate: node {} not found", node.id)));
            };
            let path = Path::from_svg(svg)
                .ok_or_else(|| rhai_err(format!("Failed to parse SVG path: {svg}")))?;
            let ease_fn = parse_easing_result(ease).map_err(rhai_err)?;
            let mut state = PathAnimationState::new(path, false, dur);
            state.progress.add_keyframe(1.0, dur, ease_fn);
            n.path_animation = Some(state);
            Ok(())
        },
    );

    // ========== PATH_ANIMATE_WITH_TANGENT ==========
    engine.register_fn(
        "path_animate_with_tangent",
        |node: &mut NodeHandle, svg: &str, dur: f64, ease: &str|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!(
                    "path_animate_with_tangent: node {} not found",
                    node.id
                )));
            };
            let path = Path::from_svg(svg)
                .ok_or_else(|| rhai_err(format!("Failed to parse SVG path: {svg}")))?;
            let ease_fn = parse_easing_result(ease).map_err(rhai_err)?;
            let mut state = PathAnimationState::new(path, true, dur);
            state.progress.add_keyframe(1.0, dur, ease_fn);
            n.path_animation = Some(state);
            Ok(())
        },
    );

    // ========== PATH_MORPH ==========
    engine.register_fn(
        "path_morph",
        |node: &mut NodeHandle, from_svg: &str, to_svg: &str, dur: f64, ease: &str|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!("path_morph: node {} not found", node.id)));
            };
            let from_path = Path::from_svg(from_svg)
                .ok_or_else(|| rhai_err(format!("Failed to parse source SVG path: {from_svg}")))?;
            let to_path = Path::from_svg(to_svg)
                .ok_or_else(|| rhai_err(format!("Failed to parse target SVG path: {to_svg}")))?;
            let ease_fn = parse_easing_result(ease).map_err(rhai_err)?;
            let mut state = PathMorphState::new(from_path, to_path, false, dur);
            state.progress.add_keyframe(1.0, dur, ease_fn);
            n.path_morph = Some(state);
            Ok(())
        },
    );

    engine.register_fn(
        "path_morph_with_tangent",
        |node: &mut NodeHandle, from_svg: &str, to_svg: &str, dur: f64, ease: &str|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!(
                    "path_morph_with_tangent: node {} not found",
                    node.id
                )));
            };
            let from_path = Path::from_svg(from_svg)
                .ok_or_else(|| rhai_err(format!("Failed to parse source SVG path: {from_svg}")))?;
            let to_path = Path::from_svg(to_svg)
                .ok_or_else(|| rhai_err(format!("Failed to parse target SVG path: {to_svg}")))?;
            let ease_fn = parse_easing_result(ease).map_err(rhai_err)?;
            let mut state = PathMorphState::new(from_path, to_path, true, dur);
            state.progress.add_keyframe(1.0, dur, ease_fn);
            n.path_morph = Some(state);
            Ok(())
        },
    );

    // ========== PATH_ANIMATE_OPTS (with loop count and speed) ==========
    engine.register_fn(
        "path_animate_opts",
        |node: &mut NodeHandle, svg: &str, dur: f64, ease: &str, loop_count: i64, speed: f64|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!(
                    "path_animate_opts: node {} not found",
                    node.id
                )));
            };
            let path = Path::from_svg(svg)
                .ok_or_else(|| rhai_err(format!("Failed to parse SVG path: {svg}")))?;
            let ease_fn = parse_easing_result(ease).map_err(rhai_err)?;
            let mut state = PathAnimationState::new(path, false, dur);
            state.progress.add_keyframe(1.0, dur, ease_fn);
            state.loop_count = loop_count as i32;
            state.speed = speed as f32;
            n.path_animation = Some(state);
            Ok(())
        },
    );

    engine.register_fn(
        "path_animate_opts",
        |node: &mut NodeHandle,
         svg: &str,
         dur: f64,
         ease: &str,
         loop_count: i64,
         speed: f64,
         follow_tangent: bool|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!(
                    "path_animate_opts: node {} not found",
                    node.id
                )));
            };
            let path = Path::from_svg(svg)
                .ok_or_else(|| rhai_err(format!("Failed to parse SVG path: {svg}")))?;
            let ease_fn = parse_easing_result(ease).map_err(rhai_err)?;
            let mut state = PathAnimationState::new(path, follow_tangent, dur);
            state.progress.add_keyframe(1.0, dur, ease_fn);
            state.loop_count = loop_count as i32;
            state.speed = speed as f32;
            n.path_animation = Some(state);
            Ok(())
        },
    );

    // ========== PATH_MORPH_OPTS (with loop count and speed) ==========
    engine.register_fn(
        "path_morph_opts",
        |node: &mut NodeHandle,
         from_svg: &str,
         to_svg: &str,
         dur: f64,
         ease: &str,
         loop_count: i64,
         speed: f64|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!(
                    "path_morph_opts: node {} not found",
                    node.id
                )));
            };
            let from_path = Path::from_svg(from_svg)
                .ok_or_else(|| rhai_err(format!("Failed to parse source SVG path: {from_svg}")))?;
            let to_path = Path::from_svg(to_svg)
                .ok_or_else(|| rhai_err(format!("Failed to parse target SVG path: {to_svg}")))?;
            let ease_fn = parse_easing_result(ease).map_err(rhai_err)?;
            let mut state = PathMorphState::new(from_path, to_path, false, dur);
            state.progress.add_keyframe(1.0, dur, ease_fn);
            state.loop_count = loop_count as i32;
            state.speed = speed as f32;
            n.path_morph = Some(state);
            Ok(())
        },
    );

    // ========== SET_BLUR ==========
    engine.register_fn(
        "set_blur",
        |node: &mut NodeHandle, val: f64| -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!("set_blur: node {} not found", node.id)));
            };
            n.element
                .animate_property("blur", val as f32, val as f32, 0.0, "linear")
                .map_err(rhai_err)
        },
    );

    // ========== ANIMATE_COLOR ==========
    engine.register_fn(
        "animate_color",
        |node: &mut NodeHandle,
         start_hex: &str,
         end_hex: &str,
         dur: f64,
         ease: &str| {
            let start = parse_hex_color(start_hex).unwrap_or(crate::types::Color::WHITE);
            let end = parse_hex_color(end_hex).unwrap_or(crate::types::Color::WHITE);
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                n.element.animate_color(start, end, dur, ease);
            }
        },
    );

    // ========== ADD_ANIMATOR (Text Animation) ==========
    engine.register_fn(
        "add_animator",
        |node: &mut NodeHandle,
         start_idx: i64,
         end_idx: i64,
         prop: &str,
         start: f64,
         end: f64,
         dur: f64,
         ease: &str| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                n.element.add_text_animator(
                    start_idx as usize,
                    end_idx as usize,
                    prop.to_string(),
                    start as f32,
                    end as f32,
                    dur,
                    ease,
                );
            }
        },
    );

    // Integer Overload for add_animator
    engine.register_fn(
        "add_animator",
        |node: &mut NodeHandle,
         start_idx: i64,
         end_idx: i64,
         prop: &str,
         start: i64,
         end: i64,
         dur: f64,
         ease: &str| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                n.element.add_text_animator(
                    start_idx as usize,
                    end_idx as usize,
                    prop.to_string(),
                    start as f32,
                    end as f32,
                    dur,
                    ease,
                );
            }
        },
    );

    // With stagger
    engine.register_fn(
        "add_animator",
        |node: &mut NodeHandle,
         start_idx: i64,
         end_idx: i64,
         prop: &str,
         start: f64,
         end: f64,
         dur: f64,
         ease: &str,
         stagger: f64| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                n.element.add_text_animator_full(
                    start_idx as usize,
                    end_idx as usize,
                    prop.to_string(),
                    start as f32,
                    end as f32,
                    dur,
                    ease,
                    stagger as f32,
                );
            }
        },
    );

    // Integer overload with stagger
    engine.register_fn(
        "add_animator",
        |node: &mut NodeHandle,
         start_idx: i64,
         end_idx: i64,
         prop: &str,
         start: i64,
         end: i64,
         dur: f64,
         ease: &str,
         stagger: f64| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                n.element.add_text_animator_full(
                    start_idx as usize,
                    end_idx as usize,
                    prop.to_string(),
                    start as f32,
                    end as f32,
                    dur,
                    ease,
                    stagger as f32,
                );
            }
        },
    );

    // ========== ADVANCED EASING / LOOPING PRIMITIVES ==========

    // `animate_bezier(prop, start, end, dur, x1, y1, x2, y2)` — cubic bezier easing.
    engine.register_fn(
        "animate_bezier",
        |node: &mut NodeHandle,
         prop: &str,
         start: f64,
         end: f64,
         dur: f64,
         x1: f64,
         y1: f64,
         x2: f64,
         y2: f64|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!(
                    "animate_bezier: node {} not found",
                    node.id
                )));
            };
            let ease = crate::animation::EasingType::CubicBezier { x1, y1, x2, y2 };
            apply_animate_prop(n, prop, start as f32, end as f32, dur, &ease).map_err(rhai_err)
        },
    );

    // `animate_hold(prop, start, end, dur)` — hold at start then jump to end.
    engine.register_fn(
        "animate_hold",
        |node: &mut NodeHandle, prop: &str, start: f64, end: f64, dur: f64|
         -> Result<(), Box<EvalAltResult>> {
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!(
                    "animate_hold: node {} not found",
                    node.id
                )));
            };
            apply_animate_prop(
                n,
                prop,
                start as f32,
                end as f32,
                dur,
                &crate::animation::EasingType::Hold,
            )
            .map_err(rhai_err)
        },
    );

    // `animate_expression(prop, expr, duration)` — bake a math expression into keyframes.
    engine.register_fn(
        "animate_expression",
        |node: &mut NodeHandle, prop: &str, expr: &str, duration: f64|
         -> Result<(), Box<EvalAltResult>> {
            // Validate expression before baking.
            crate::animation::evaluate_expression_result(expr, 0.0).map_err(rhai_err)?;
            crate::animation::evaluate_expression_result(expr, 1.0).map_err(rhai_err)?;
            let mut d = node.lock_director()?;
            let Some(n) = d.scene.get_node_mut(node.id) else {
                return Err(rhai_err(format!(
                    "animate_expression: node {} not found",
                    node.id
                )));
            };
            let anim = crate::animation::Animated::<f32>::from_expression(expr, duration);
            apply_f32_anim(n, prop, anim).map_err(rhai_err)
        },
    );

    // `reverse_animation(prop)` — reverse existing keyframes on a channel.
    engine.register_fn("reverse_animation", |node: &mut NodeHandle, prop: &str| {
        let mut d = match node.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(n) = d.scene.get_node_mut(node.id) {
            mutate_f32_channel(n, prop, |anim| anim.reverse());
        }
    });

    // `yoyo_animation(prop)` — append a reversed copy of the channel (ping-pong).
    engine.register_fn("yoyo_animation", |node: &mut NodeHandle, prop: &str| {
        let mut d = match node.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(n) = d.scene.get_node_mut(node.id) {
            mutate_f32_channel(n, prop, |anim| anim.yoyo());
        }
    });

    // `loop_animation(prop, count)` — repeat the channel `count` times.
    engine.register_fn("loop_animation", |node: &mut NodeHandle, prop: &str, count: i64| {
        let mut d = match node.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(n) = d.scene.get_node_mut(node.id) {
            mutate_f32_channel(n, prop, |anim| anim.loop_count(count as i32));
        }
    });
}

fn apply_f32_anim(
    n: &mut crate::scene::SceneNode,
    prop: &str,
    anim: crate::animation::Animated<f32>,
) -> Result<(), String> {
    match prop {
        "scale" => {
            n.transform.scale_x = anim.clone();
            n.transform.scale_y = anim;
            Ok(())
        }
        "scale_x" => {
            n.transform.scale_x = anim;
            Ok(())
        }
        "scale_y" => {
            n.transform.scale_y = anim;
            Ok(())
        }
        "rotation" => {
            n.transform.rotation = anim;
            Ok(())
        }
        "skew_x" => {
            n.transform.skew_x = anim;
            Ok(())
        }
        "skew_y" => {
            n.transform.skew_y = anim;
            Ok(())
        }
        "translate_x" | "x" => {
            n.transform.translate_x = anim;
            Ok(())
        }
        "translate_y" | "y" => {
            n.transform.translate_y = anim;
            Ok(())
        }
        other => {
            if let Some(v) = anim.raw_keyframes.first() {
                let start = v.0;
                let end = anim.raw_keyframes.last().map(|k| k.0).unwrap_or(start);
                n.element.animate_property(
                    other,
                    start,
                    end,
                    anim.sequence.duration(),
                    "linear",
                )
            } else {
                Err(format!("animate_expression: empty animation for '{other}'"))
            }
        }
    }
}

fn mutate_f32_channel<F>(n: &mut crate::scene::SceneNode, prop: &str, mut f: F)
where
    F: FnMut(&mut crate::animation::Animated<f32>),
{
    match prop {
        "scale" => {
            f(&mut n.transform.scale_x);
            f(&mut n.transform.scale_y);
        }
        "scale_x" => f(&mut n.transform.scale_x),
        "scale_y" => f(&mut n.transform.scale_y),
        "rotation" => f(&mut n.transform.rotation),
        "skew_x" => f(&mut n.transform.skew_x),
        "skew_y" => f(&mut n.transform.skew_y),
        "translate_x" | "x" => f(&mut n.transform.translate_x),
        "translate_y" | "y" => f(&mut n.transform.translate_y),
        _ => {}
    }
}
