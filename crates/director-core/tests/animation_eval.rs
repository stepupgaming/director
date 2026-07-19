//! Plan 04 — animation evaluation acceptance tests.
//!
//! Locks pure `value_at`, loud authoring errors, TextFit vs font_size separation,
//! and path loop wrapping.

use director_core::animation::{
    evaluate_expression_result, Animated, EasingType,
};
use director_core::element::{Element, TextFit, TextSpan};
use director_core::node::{BoxNode, TextNode};
use director_core::scripting::register_rhai_api;
use director_core::scripting::utils::parse_easing_result;
use director_core::types::wrap_looped_time;
use director_core::DefaultAssetLoader;
use keyframe::EasingFunction;
use rhai::Engine;
use skia_safe::textlayout::FontCollection;
use std::sync::{Arc, Mutex};
use taffy::style::{Dimension, Style};

fn font_collection() -> Arc<Mutex<FontCollection>> {
    Arc::new(Mutex::new(FontCollection::new()))
}

#[test]
fn eval_linear_midpoint() {
    let mut a = Animated::new(0.0_f32);
    a.add_segment(0.0, 10.0, 1.0, EasingType::Linear);
    assert!((a.value_at(0.5) - 5.0).abs() < 1e-3);
    a.update(0.5);
    assert!((a.current_value - 5.0).abs() < 1e-3);
}

#[test]
fn eval_hold_stays_start_until_end() {
    let mut a = Animated::new(0.0_f32);
    a.add_segment(0.0, 10.0, 1.0, EasingType::Hold);
    assert!((a.value_at(0.0) - 0.0).abs() < 1e-5);
    assert!((a.value_at(0.5) - 0.0).abs() < 1e-5);
    assert!((a.value_at(0.999) - 0.0).abs() < 1e-5);
    assert!((a.value_at(1.0) - 10.0).abs() < 1e-5);
}

#[test]
fn eval_bezier_monotonic_x() {
    let ease = EasingType::CubicBezier {
        x1: 0.42,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    };
    let mut prev = ease.y(0.0);
    for i in 1..=20 {
        let x = i as f64 / 20.0;
        let y = ease.y(x);
        assert!(y + 1e-6 >= prev, "non-monotonic at x={x}");
        prev = y;
    }
    assert!((ease.y(1.0) - 1.0).abs() < 1e-5);
}

#[test]
fn eval_before_after_extents() {
    let mut a = Animated::new(1.0_f32);
    a.add_segment(1.0, 5.0, 2.0, EasingType::Linear);
    assert!((a.value_at(-1.0) - 1.0).abs() < 1e-5);
    assert!((a.value_at(100.0) - 5.0).abs() < 1e-5);
}

#[test]
fn reverse_yoyo_loop_tables() {
    let mut a = Animated::new(0.0_f32);
    a.add_segment(0.0, 10.0, 1.0, EasingType::Linear);
    a.reverse();
    assert!((a.value_at(0.0) - 10.0).abs() < 1e-2);
    assert!((a.value_at(1.0) - 0.0).abs() < 1e-2);

    let mut b = Animated::new(0.0_f32);
    b.add_segment(0.0, 10.0, 1.0, EasingType::Linear);
    b.yoyo();
    assert!((b.value_at(0.5) - 5.0).abs() < 1e-2);
    assert!((b.value_at(1.5) - 5.0).abs() < 1e-2);

    let mut c = Animated::new(0.0_f32);
    c.add_segment(0.0, 4.0, 1.0, EasingType::Linear);
    c.loop_count(2);
    assert!((c.value_at(0.5) - 2.0).abs() < 1e-2);
    assert!((c.value_at(1.5) - 2.0).abs() < 1e-2);
}

#[test]
fn unknown_prop_errors() {
    let mut box_node = BoxNode::new();
    let err = box_node
        .animate_property("not_a_real_prop", 0.0, 1.0, 1.0, "linear")
        .unwrap_err();
    assert!(err.contains("unknown property"), "{err}");

    let mut text = TextNode::new(
        vec![TextSpan {
            text: "hi".into(),
            ..Default::default()
        }],
        font_collection(),
    );
    let err = text
        .animate_property("bogus", 0.0, 1.0, 1.0, "linear")
        .unwrap_err();
    assert!(err.contains("unknown property"), "{err}");
}

#[test]
fn unknown_easing_errors() {
    assert!(parse_easing_result("not_an_ease").is_err());
    let mut box_node = BoxNode::new();
    let err = box_node
        .animate_property("opacity", 0.0, 1.0, 1.0, "not_an_ease")
        .unwrap_err();
    assert!(
        err.contains("unknown easing") || err.contains("not_an_ease"),
        "{err}"
    );
}

#[test]
fn expression_rejects_bad_ident() {
    assert!(evaluate_expression_result("nope(t)", 0.5).is_err());
    assert!(evaluate_expression_result("", 0.5).is_err());
    assert!(evaluate_expression_result("sin(t)", 0.0).is_ok());
}

#[test]
fn shrink_fit_does_not_clobber_font_size_keyframes() {
    let mut text = TextNode::new(
        vec![TextSpan {
            text: "HELLO WORLD THIS IS A LONG LINE".into(),
            ..Default::default()
        }],
        font_collection(),
    );
    text.fit_mode = TextFit::Shrink;
    text.min_size = 8.0;
    text.max_size = 200.0;
    text.default_font_size = Animated::new(80.0);
    text.default_font_size
        .add_segment(80.0, 40.0, 1.0, EasingType::Linear);
    text.style = Style {
        size: taffy::geometry::Size {
            width: Dimension::length(120.0),
            height: Dimension::length(40.0),
        },
        ..Default::default()
    };

    let kfs_before = text.default_font_size.raw_keyframes.clone();

    text.update(0.0);
    let channel_before_fit = text.default_font_size.current_value;
    text.post_layout(skia_safe::Rect::from_xywh(0.0, 0.0, 120.0, 40.0));

    assert_eq!(
        text.default_font_size.raw_keyframes, kfs_before,
        "fit must not rewrite keyframes"
    );
    assert!(
        (text.default_font_size.current_value - channel_before_fit).abs() < 1e-5,
        "fit must not clobber animated current_value (was {channel_before_fit}, now {})",
        text.default_font_size.current_value
    );
    assert!(
        text.fitted_font_size.is_some(),
        "fit should populate fitted_font_size"
    );
    let fitted = text.fitted_font_size.unwrap();
    assert!(
        fitted <= channel_before_fit + 1e-3,
        "fitted {fitted} should be <= authored {channel_before_fit}"
    );

    text.update(0.5);
    let mid = text.default_font_size.current_value;
    assert!((mid - 60.0).abs() < 1.0, "expected ~60 at t=0.5, got {mid}");
    text.post_layout(skia_safe::Rect::from_xywh(0.0, 0.0, 120.0, 40.0));
    assert!(
        (text.default_font_size.current_value - mid).abs() < 1e-5,
        "post_layout must not overwrite mid-animation value"
    );
}

#[test]
fn path_loop_wrap_boundaries() {
    let dur = 2.0;
    // play once
    assert!((wrap_looped_time(0.5, dur, 0) - 0.5).abs() < 1e-9);
    assert!((wrap_looped_time(2.0, dur, 0) - 2.0).abs() < 1e-9);
    assert!((wrap_looped_time(3.0, dur, 0) - 2.0).abs() < 1e-9);

    // infinite
    assert!((wrap_looped_time(2.5, dur, -1) - 0.5).abs() < 1e-9);
    assert!((wrap_looped_time(4.0, dur, -1) - 0.0).abs() < 1e-9);

    // finite loop_count = 2 (allows elapsed 0 and 1)
    assert!((wrap_looped_time(0.5, dur, 2) - 0.5).abs() < 1e-9);
    assert!((wrap_looped_time(2.5, dur, 2) - 0.5).abs() < 1e-9);
    // elapsed = 2 at t=4 → clamp
    assert!((wrap_looped_time(4.0, dur, 2) - 2.0).abs() < 1e-9);
    assert!((wrap_looped_time(5.0, dur, 2) - 2.0).abs() < 1e-9);
}

#[test]
fn rhai_unknown_prop_and_easing_error() {
    let mut eng = Engine::new();
    let loader = Arc::new(DefaultAssetLoader);
    register_rhai_api(&mut eng, loader);

    let result = eng.eval::<()>(
        r#"
        let m = new_director(32, 32, 30);
        let s = m.add_scene(1.0);
        let b = s.add_box(#{ width: 10, height: 10 });
        b.animate("not_real", 0.0, 1.0, 0.5, "linear");
        "#,
    );
    assert!(result.is_err(), "unknown prop must error in Rhai");
    let msg = format!("{:?}", result.err().unwrap());
    assert!(
        msg.contains("unknown property") || msg.contains("not_real"),
        "{msg}"
    );

    let result = eng.eval::<()>(
        r#"
        let m = new_director(32, 32, 30);
        let s = m.add_scene(1.0);
        let b = s.add_box(#{ width: 10, height: 10 });
        b.animate("opacity", 0.0, 1.0, 0.5, "nope_ease");
        "#,
    );
    assert!(result.is_err(), "unknown easing must error in Rhai");
    let msg = format!("{:?}", result.err().unwrap());
    assert!(
        msg.contains("unknown easing") || msg.contains("nope_ease"),
        "{msg}"
    );
}
