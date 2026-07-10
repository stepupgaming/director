// Animation State Visual Regression Tests
//
// Each test sets up a 2-second animation and renders at the midpoint (t=1.0)
// to capture a visually distinct intermediate state. This catches regressions
// in interpolation, easing, and transform application.

use director_core::animation::{Animated, EasingType};
use director_core::director::TimelineItem;
use director_core::node::BoxNode;
use director_core::types::Color;
use taffy::style::Dimension;

/// Helper: create a 300×300 director with a centered 100×100 red box.
/// Returns (director_arc, root_id, box_id).
fn setup_scene_with_box(
    bg: Color,
    box_color: Color,
) -> (
    std::sync::Arc<std::sync::Mutex<director_core::Director>>,
    usize,
    usize,
) {
    use crate::visual::setup_test_director;

    let director_arc = setup_test_director(300, 300);
    let root_id;
    let box_id;
    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(bg));
        root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
            freeze_time: None,
        });

        let mut box_node = BoxNode::new();
        box_node.style.size.width = Dimension::length(100.0);
        box_node.style.size.height = Dimension::length(100.0);
        box_node.bg_color = Some(Animated::new(box_color));
        box_id = d.scene.add_node(Box::new(box_node));
        d.scene.add_child(root_id, box_id);
    }
    (director_arc, root_id, box_id)
}

/// Opacity animating from 1.0 → 0.0 over 2s, captured at t=1.0 (≈50% transparent).
#[test]
fn anim_opacity_mid() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.8, 0.2, 0.2, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        node.element
            .animate_property("opacity", 1.0, 0.0, 2.0, "linear");
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "animations", "anim_opacity_mid").unwrap();
}

/// Scale animating from 1.0 → 3.0 over 2s, captured at t=1.0 (≈2.0× size).
#[test]
fn anim_scale_mid() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.2, 0.8, 0.2, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        node.transform
            .scale_x
            .add_segment(1.0, 3.0, 2.0, EasingType::Linear);
        node.transform
            .scale_y
            .add_segment(1.0, 3.0, 2.0, EasingType::Linear);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "animations", "anim_scale_mid").unwrap();
}

/// Rotation animating from 0° → 90° over 2s, captured at t=1.0 (≈45°).
#[test]
fn anim_rotation_45() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.2, 0.2, 0.8, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        node.transform
            .rotation
            .add_segment(0.0, 90.0, 2.0, EasingType::Linear);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "animations", "anim_rotation_45").unwrap();
}

/// Translation animating from (0,0) → (150,100) over 2s, captured at t=1.0 (≈75,50).
#[test]
fn anim_translate_mid() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.9, 0.6, 0.1, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        node.transform
            .translate_x
            .add_segment(0.0, 150.0, 2.0, EasingType::Linear);
        node.transform
            .translate_y
            .add_segment(0.0, 100.0, 2.0, EasingType::Linear);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "animations", "anim_translate_mid").unwrap();
}

/// Background color animating from red → blue over 2s (OKLab interpolation),
/// captured at t=1.0 (≈purple midpoint).
#[test]
fn anim_color_mid() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(1.0, 0.0, 0.0, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        let box_node = node
            .element
            .as_any_mut()
            .downcast_mut::<BoxNode>()
            .unwrap();
        if let Some(bg) = &mut box_node.bg_color {
            bg.add_segment(
                Color::new(1.0, 0.0, 0.0, 1.0),
                Color::new(0.0, 0.0, 1.0, 1.0),
                2.0,
                EasingType::Linear,
            );
        }
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "animations", "anim_color_mid").unwrap();
}

/// Border radius animating from 0 → 80 over 2s, captured at t=1.0 (≈40px radius).
#[test]
fn anim_border_radius_mid() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.6, 0.2, 0.8, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        node.element
            .animate_property("border_radius", 0.0, 80.0, 2.0, "linear");
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "animations", "anim_border_radius_mid").unwrap();
}

// ============================================================================
// Complex Easing Visual Regression Tests
// ============================================================================

/// Scale animating with BounceOut easing from 1.0 → 3.0 over 2s,
/// captured at t=1.2 (should show bounce oscillation).
#[test]
fn easing_bounce_out() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.8, 0.2, 0.2, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        node.transform
            .scale_x
            .add_segment(1.0, 3.0, 2.0, EasingType::BounceOut);
        node.transform
            .scale_y
            .add_segment(1.0, 3.0, 2.0, EasingType::BounceOut);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.2, "animations", "easing_bounce_out").unwrap();
}

/// Scale animating with ElasticOut easing from 1.0 → 3.0 over 2s,
/// captured at t=0.8 (should show elastic overshoot).
#[test]
fn easing_elastic_out() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.2, 0.8, 0.2, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        node.transform
            .scale_x
            .add_segment(1.0, 3.0, 2.0, EasingType::ElasticOut);
        node.transform
            .scale_y
            .add_segment(1.0, 3.0, 2.0, EasingType::ElasticOut);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.8, "animations", "easing_elastic_out").unwrap();
}

/// Scale animating with BackOut easing from 1.0 → 3.0 over 2s,
/// captured at t=1.0 (should show back overshoot).
#[test]
fn easing_back_out() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.2, 0.2, 0.8, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        node.transform
            .scale_x
            .add_segment(1.0, 3.0, 2.0, EasingType::BackOut);
        node.transform
            .scale_y
            .add_segment(1.0, 3.0, 2.0, EasingType::BackOut);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "animations", "easing_back_out").unwrap();
}

// ============================================================================
// Spring Physics Visual Regression Tests
// ============================================================================

/// Opacity spring from 0.0 → 1.0 with default wobbly config,
/// captured at t=0.5 (should show partial settle with possible oscillation).
#[test]
fn spring_physics_opacity() {
    use crate::visual::assert_visual_match;
    use director_core::animation::SpringConfig;

    let (director_arc, _, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.8, 0.2, 0.2, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        let config = SpringConfig {
            stiffness: 100.0,
            damping: 10.0,
            mass: 1.0,
            velocity: 0.0,
        };
        node.element
            .animate_property_spring("opacity", Some(0.0), 1.0, config);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.5, "animations", "spring_physics_opacity").unwrap();
}

/// Scale spring from 1.0 → 2.5 with high stiffness, captured at t=0.4.
#[test]
fn spring_physics_scale() {
    use crate::visual::assert_visual_match;
    use director_core::animation::SpringConfig;

    let (director_arc, _, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.2, 0.6, 0.9, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        let config = SpringConfig {
            stiffness: 180.0,
            damping: 12.0,
            mass: 1.0,
            velocity: 0.0,
        };
        node.transform
            .scale_x
            .add_spring_with_start(1.0, 2.5, config.clone());
        node.transform
            .scale_y
            .add_spring_with_start(1.0, 2.5, config);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.4, "animations", "spring_physics_scale").unwrap();
}
