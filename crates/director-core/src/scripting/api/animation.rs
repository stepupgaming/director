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
use rhai::Engine;
use skia_safe::Path;
use tracing::error;

use super::super::types::NodeHandle;
use super::super::utils::{parse_easing_loud, parse_hex_color, parse_spring_config};

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
         delay: f64| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                let ease_fn = parse_easing_loud(ease);

                // Add delay segment (hold at start value)
                if delay > 0.0 {
                    match prop {
                        "scale" => {
                            n.transform.scale_x.add_segment(
                                start as f32,
                                start as f32,
                                delay,
                                crate::animation::EasingType::Linear,
                            );
                            n.transform.scale_y.add_segment(
                                start as f32,
                                start as f32,
                                delay,
                                crate::animation::EasingType::Linear,
                            );
                        }
                        "x" | "translate_x" => n.transform.translate_x.add_segment(
                            start as f32,
                            start as f32,
                            delay,
                            crate::animation::EasingType::Linear,
                        ),
                        "y" | "translate_y" => n.transform.translate_y.add_segment(
                            start as f32,
                            start as f32,
                            delay,
                            crate::animation::EasingType::Linear,
                        ),
                        "rotation" => n.transform.rotation.add_segment(
                            start as f32,
                            start as f32,
                            delay,
                            crate::animation::EasingType::Linear,
                        ),
                        _ => {
                            error!("animate: unknown property '{prop}'; ignoring delay segment");
                            n.element.animate_property(
                                prop,
                                start as f32,
                                start as f32,
                                delay,
                                "linear",
                            );
                        }
                    }
                }

                // Add actual animation segment
                match prop {
                    "scale" => {
                        n.transform
                            .scale_x
                            .add_segment(start as f32, end as f32, dur, ease_fn.clone());
                        n.transform
                            .scale_y
                            .add_segment(start as f32, end as f32, dur, ease_fn);
                    }
                    "scale_x" => {
                        n.transform
                            .scale_x
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "scale_y" => {
                        n.transform
                            .scale_y
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "rotation" => {
                        n.transform
                            .rotation
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "translate_x" | "x" => {
                        n.transform
                            .translate_x
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "translate_y" | "y" => {
                        n.transform
                            .translate_y
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    _ => {
                        error!(
                            "animate: unknown property '{prop}'; no animation applied. \
                             Supported: scale, scale_x, scale_y, rotation, skew_x, skew_y, \
                             translate_x/x, translate_y/y, or a node-specific property"
                        );
                        n.element
                            .animate_property(prop, start as f32, end as f32, dur, ease);
                    }
                }
            }
        },
    );

    // ========== ANIMATE ==========
    engine.register_fn(
        "animate",
        |node: &mut NodeHandle, prop: &str, start: f64, end: f64, dur: f64, ease: &str| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                let ease_fn = parse_easing_loud(ease);

                match prop {
                    "scale" => {
                        n.transform
                            .scale_x
                            .add_segment(start as f32, end as f32, dur, ease_fn.clone());
                        n.transform
                            .scale_y
                            .add_segment(start as f32, end as f32, dur, ease_fn);
                    }
                    "scale_x" => {
                        n.transform
                            .scale_x
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "scale_y" => {
                        n.transform
                            .scale_y
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "rotation" => {
                        n.transform
                            .rotation
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "skew_x" => {
                        n.transform
                            .skew_x
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "skew_y" => {
                        n.transform
                            .skew_y
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "translate_x" | "x" => {
                        n.transform
                            .translate_x
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "translate_y" | "y" => {
                        n.transform
                            .translate_y
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    _ => {
                        error!(
                            "animate: unknown property '{prop}'; no animation applied. \
                             Supported: scale, scale_x, scale_y, rotation, skew_x, skew_y, \
                             translate_x/x, translate_y/y, or a node-specific property"
                        );
                        n.element
                            .animate_property(prop, start as f32, end as f32, dur, ease);
                    }
                }
            }
        },
    );

    // Integer Overload for animate
    engine.register_fn(
        "animate",
        |node: &mut NodeHandle, prop: &str, start: i64, end: i64, dur: f64, ease: &str| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                let ease_fn = parse_easing_loud(ease);

                match prop {
                    "scale" => {
                        n.transform
                            .scale_x
                            .add_segment(start as f32, end as f32, dur, ease_fn.clone());
                        n.transform
                            .scale_y
                            .add_segment(start as f32, end as f32, dur, ease_fn);
                    }
                    "scale_x" => {
                        n.transform
                            .scale_x
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "scale_y" => {
                        n.transform
                            .scale_y
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "rotation" => {
                        n.transform
                            .rotation
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "skew_x" => {
                        n.transform
                            .skew_x
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "skew_y" => {
                        n.transform
                            .skew_y
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "translate_x" | "x" => {
                        n.transform
                            .translate_x
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    "translate_y" | "y" => {
                        n.transform
                            .translate_y
                            .add_segment(start as f32, end as f32, dur, ease_fn)
                    }
                    _ => {
                        error!(
                            "animate: unknown property '{prop}'; no animation applied. \
                             Supported: scale, scale_x, scale_y, rotation, skew_x, skew_y, \
                             translate_x/x, translate_y/y, or a node-specific property"
                        );
                        n.element
                            .animate_property(prop, start as f32, end as f32, dur, ease);
                    }
                }
            }
        },
    );

    // Spring animation via animate (shorthand: animate(prop, end, config))
    engine.register_fn(
        "animate",
        |node: &mut NodeHandle, prop: &str, end: f64, config: rhai::Map| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            let spring_conf = parse_spring_config(&config);

            if let Some(n) = d.scene.get_node_mut(node.id) {
                match prop {
                    "scale" => {
                        n.transform
                            .scale_x
                            .add_spring(end as f32, spring_conf.clone());
                        n.transform.scale_y.add_spring(end as f32, spring_conf);
                    }
                    "scale_x" => n.transform.scale_x.add_spring(end as f32, spring_conf),
                    "scale_y" => n.transform.scale_y.add_spring(end as f32, spring_conf),
                    "rotation" => n.transform.rotation.add_spring(end as f32, spring_conf),
                    "skew_x" => n.transform.skew_x.add_spring(end as f32, spring_conf),
                    "skew_y" => n.transform.skew_y.add_spring(end as f32, spring_conf),
                    "translate_x" | "x" => {
                        n.transform.translate_x.add_spring(end as f32, spring_conf)
                    }
                    "translate_y" | "y" => {
                        n.transform.translate_y.add_spring(end as f32, spring_conf)
                    }
                    _ => {
                        error!("animate (spring): unknown property '{prop}'; no spring applied");
                        n.element
                            .animate_property_spring(prop, None, end as f32, spring_conf);
                    }
                }
            }
        },
    );

    // ========== SPRING ==========
    engine.register_fn(
        "spring",
        |node: &mut NodeHandle, prop: &str, start: f64, end: f64, config: rhai::Map| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            let spring_conf = parse_spring_config(&config);

            if let Some(n) = d.scene.get_node_mut(node.id) {
                match prop {
                    "scale" => {
                        n.transform.scale_x.add_spring_with_start(
                            start as f32,
                            end as f32,
                            spring_conf.clone(),
                        );
                        n.transform.scale_y.add_spring_with_start(
                            start as f32,
                            end as f32,
                            spring_conf,
                        );
                    }
                    "scale_x" => n.transform.scale_x.add_spring_with_start(
                        start as f32,
                        end as f32,
                        spring_conf,
                    ),
                    "scale_y" => n.transform.scale_y.add_spring_with_start(
                        start as f32,
                        end as f32,
                        spring_conf,
                    ),
                    "rotation" => n.transform.rotation.add_spring_with_start(
                        start as f32,
                        end as f32,
                        spring_conf,
                    ),
                    "skew_x" => n.transform.skew_x.add_spring_with_start(
                        start as f32,
                        end as f32,
                        spring_conf,
                    ),
                    "skew_y" => n.transform.skew_y.add_spring_with_start(
                        start as f32,
                        end as f32,
                        spring_conf,
                    ),
                    "translate_x" | "x" => n.transform.translate_x.add_spring_with_start(
                        start as f32,
                        end as f32,
                        spring_conf,
                    ),
                    "translate_y" | "y" => n.transform.translate_y.add_spring_with_start(
                        start as f32,
                        end as f32,
                        spring_conf,
                    ),
                    _ => {
                        error!("animate (spring): unknown property '{prop}'; no spring applied");
                        n.element.animate_property_spring(
                            prop,
                            Some(start as f32),
                            end as f32,
                            spring_conf,
                        );
                    }
                }
            }
        },
    );

    // ========== PATH_ANIMATE ==========
    engine.register_fn(
        "path_animate",
        |node: &mut NodeHandle, svg: &str, dur: f64, ease: &str| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                // Try to parse SVG path
                if let Some(path) = Path::from_svg(svg) {
                    let ease_fn = parse_easing_loud(ease);
                    let mut state = PathAnimationState::new(path, false, dur);
                    state.progress.add_keyframe(1.0, dur, ease_fn);
                    n.path_animation = Some(state);
                } else {
                    error!("Failed to parse SVG path: {}", svg);
                }
            }
        },
    );

    // ========== PATH_ANIMATE_WITH_TANGENT ==========
    engine.register_fn(
        "path_animate_with_tangent",
        |node: &mut NodeHandle, svg: &str, dur: f64, ease: &str| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                // Try to parse SVG path
                if let Some(path) = Path::from_svg(svg) {
                    let ease_fn = parse_easing_loud(ease);
                    let mut state = PathAnimationState::new(path, true, dur);
                    state.progress.add_keyframe(1.0, dur, ease_fn);
                    n.path_animation = Some(state);
                } else {
                    error!("Failed to parse SVG path: {}", svg);
                }
            }
        },
    );

    // ========== PATH_MORPH ==========
    engine.register_fn(
        "path_morph",
        |node: &mut NodeHandle, from_svg: &str, to_svg: &str, dur: f64, ease: &str| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                if let Some(from_path) = Path::from_svg(from_svg) {
                    if let Some(to_path) = Path::from_svg(to_svg) {
                        let ease_fn = parse_easing_loud(ease);
                        let mut state = PathMorphState::new(from_path, to_path, false, dur);
                        state.progress.add_keyframe(1.0, dur, ease_fn);
                        n.path_morph = Some(state);
                    } else {
                        error!("Failed to parse target SVG path: {}", to_svg);
                    }
                } else {
                    error!("Failed to parse source SVG path: {}", from_svg);
                }
            }
        },
    );

    engine.register_fn(
        "path_morph_with_tangent",
        |node: &mut NodeHandle, from_svg: &str, to_svg: &str, dur: f64, ease: &str| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                if let Some(from_path) = Path::from_svg(from_svg) {
                    if let Some(to_path) = Path::from_svg(to_svg) {
                        let ease_fn = parse_easing_loud(ease);
                        let mut state = PathMorphState::new(from_path, to_path, true, dur);
                        state.progress.add_keyframe(1.0, dur, ease_fn);
                        n.path_morph = Some(state);
                    } else {
                        error!("Failed to parse target SVG path: {}", to_svg);
                    }
                } else {
                    error!("Failed to parse source SVG path: {}", from_svg);
                }
            }
        },
    );

    // ========== PATH_ANIMATE_OPTS (with loop count and speed) ==========
    engine.register_fn(
        "path_animate_opts",
        |node: &mut NodeHandle, svg: &str, dur: f64, ease: &str, loop_count: i64, speed: f64| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                if let Some(path) = Path::from_svg(svg) {
                    let ease_fn = parse_easing_loud(ease);
                    let mut state = PathAnimationState::new(path, false, dur);
                    state.progress.add_keyframe(1.0, dur, ease_fn);
                    state.loop_count = loop_count as i32;
                    state.speed = speed as f32;
                    n.path_animation = Some(state);
                } else {
                    error!("Failed to parse SVG path: {}", svg);
                }
            }
        },
    );

    engine.register_fn(
        "path_animate_opts",
        |node: &mut NodeHandle, svg: &str, dur: f64, ease: &str, loop_count: i64, speed: f64, follow_tangent: bool| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                if let Some(path) = Path::from_svg(svg) {
                    let ease_fn = parse_easing_loud(ease);
                    let mut state = PathAnimationState::new(path, follow_tangent, dur);
                    state.progress.add_keyframe(1.0, dur, ease_fn);
                    state.loop_count = loop_count as i32;
                    state.speed = speed as f32;
                    n.path_animation = Some(state);
                } else {
                    error!("Failed to parse SVG path: {}", svg);
                }
            }
        },
    );

    // ========== PATH_MORPH_OPTS (with loop count and speed) ==========
    engine.register_fn(
        "path_morph_opts",
        |node: &mut NodeHandle, from_svg: &str, to_svg: &str, dur: f64, ease: &str, loop_count: i64, speed: f64| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                if let Some(from_path) = Path::from_svg(from_svg) {
                    if let Some(to_path) = Path::from_svg(to_svg) {
                        let ease_fn = parse_easing_loud(ease);
                        let mut state = PathMorphState::new(from_path, to_path, false, dur);
                        state.progress.add_keyframe(1.0, dur, ease_fn);
                        state.loop_count = loop_count as i32;
                        state.speed = speed as f32;
                        n.path_morph = Some(state);
                    } else {
                        error!("Failed to parse target SVG path: {}", to_svg);
                    }
                } else {
                    error!("Failed to parse source SVG path: {}", from_svg);
                }
            }
        },
    );

    // ========== SET_BLUR ==========
    engine.register_fn("set_blur", |node: &mut NodeHandle, val: f64| {
        let mut d = node.director.lock().unwrap();
        if let Some(n) = d.scene.get_node_mut(node.id) {
            n.element
                .animate_property("blur", val as f32, val as f32, 0.0, "linear");
        }
    });

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

    /// `animate_bezier(prop, start, end, dur, x1, y1, x2, y2)` — cubic bezier easing.
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
         y2: f64| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                let ease = crate::animation::EasingType::CubicBezier { x1, y1, x2, y2 };
                apply_transform_or_element_anim(n, prop, start as f32, end as f32, dur, ease);
            }
        },
    );

    /// `animate_hold(prop, start, end, dur)` — hold at start then jump to end.
    engine.register_fn(
        "animate_hold",
        |node: &mut NodeHandle, prop: &str, start: f64, end: f64, dur: f64| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                let ease = crate::animation::EasingType::Hold;
                apply_transform_or_element_anim(n, prop, start as f32, end as f32, dur, ease);
            }
        },
    );

    /// `animate_expression(prop, expr, duration)` — bake a math expression into keyframes.
    engine.register_fn(
        "animate_expression",
        |node: &mut NodeHandle, prop: &str, expr: &str, duration: f64| {
            let mut d = match node.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(n) = d.scene.get_node_mut(node.id) {
                let anim = crate::animation::Animated::<f32>::from_expression(expr, duration);
                apply_f32_anim(n, prop, anim);
            }
        },
    );

    /// `reverse_animation(prop)` — reverse existing keyframes on a channel.
    engine.register_fn("reverse_animation", |node: &mut NodeHandle, prop: &str| {
        let mut d = match node.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(n) = d.scene.get_node_mut(node.id) {
            mutate_f32_channel(n, prop, |anim| anim.reverse());
        }
    });

    /// `yoyo_animation(prop)` — append a reversed copy of the channel (ping-pong).
    engine.register_fn("yoyo_animation", |node: &mut NodeHandle, prop: &str| {
        let mut d = match node.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(n) = d.scene.get_node_mut(node.id) {
            mutate_f32_channel(n, prop, |anim| anim.yoyo());
        }
    });

    /// `loop_animation(prop, count)` — repeat the channel `count` times.
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

fn apply_transform_or_element_anim(
    n: &mut crate::scene::SceneNode,
    prop: &str,
    start: f32,
    end: f32,
    dur: f64,
    ease: crate::animation::EasingType,
) {
    match prop {
        "scale" => {
            n.transform.scale_x.add_segment(start, end, dur, ease.clone());
            n.transform.scale_y.add_segment(start, end, dur, ease);
        }
        "scale_x" => n.transform.scale_x.add_segment(start, end, dur, ease),
        "scale_y" => n.transform.scale_y.add_segment(start, end, dur, ease),
        "rotation" => n.transform.rotation.add_segment(start, end, dur, ease),
        "skew_x" => n.transform.skew_x.add_segment(start, end, dur, ease),
        "skew_y" => n.transform.skew_y.add_segment(start, end, dur, ease),
        "translate_x" | "x" => n.transform.translate_x.add_segment(start, end, dur, ease),
        "translate_y" | "y" => n.transform.translate_y.add_segment(start, end, dur, ease),
        _ => {
            n.element.animate_property(prop, start, end, dur, "linear");
        }
    }
}

fn apply_f32_anim(n: &mut crate::scene::SceneNode, prop: &str, anim: crate::animation::Animated<f32>) {
    match prop {
        "scale" => {
            n.transform.scale_x = anim.clone();
            n.transform.scale_y = anim;
        }
        "scale_x" => n.transform.scale_x = anim,
        "scale_y" => n.transform.scale_y = anim,
        "rotation" => n.transform.rotation = anim,
        "skew_x" => n.transform.skew_x = anim,
        "skew_y" => n.transform.skew_y = anim,
        "translate_x" | "x" => n.transform.translate_x = anim,
        "translate_y" | "y" => n.transform.translate_y = anim,
        _ => {
            if let Some(v) = anim.raw_keyframes.first() {
                let start = v.0;
                let end = anim.raw_keyframes.last().map(|k| k.0).unwrap_or(start);
                n.element.animate_property(prop, start, end, anim.sequence.duration(), "linear");
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
