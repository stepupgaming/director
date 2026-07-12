// Visual Effects Visual Regression Tests
//
// Tests for BoxNode visual properties: blur, drop shadow, border,
// rounded corners, and overflow:hidden clipping.

use director_core::animation::Animated;
use director_core::director::TimelineItem;
use director_core::element::TextSpan;
use director_core::node::TextNode;
use director_core::node::BoxNode;
use director_core::types::Color;
use taffy::style::{Dimension, LengthPercentageAuto};

/// Helper: create a 300x300 director with a white background root and a
/// centered child box of the given size and color. Returns (director_arc, root_id, box_id).
fn setup_scene_with_box(
    box_w: f32,
    box_h: f32,
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
        root.bg_color = Some(Animated::new(Color::WHITE));
        root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let mut box_node = BoxNode::new();
        box_node.style.size.width = Dimension::length(box_w);
        box_node.style.size.height = Dimension::length(box_h);
        box_node.bg_color = Some(Animated::new(box_color));
        box_id = d.scene.add_node(Box::new(box_node));
        d.scene.add_child(root_id, box_id);
    }
    (director_arc, root_id, box_id)
}

/// Box with a Gaussian blur applied.
#[test]
fn effect_blur() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(120.0, 120.0, Color::new(0.8, 0.2, 0.2, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        let box_node = node
            .element
            .as_any_mut()
            .downcast_mut::<BoxNode>()
            .unwrap();
        box_node.blur = Animated::new(15.0);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "effects", "effect_blur").unwrap();
}

/// Box with a drop shadow.
#[test]
fn effect_shadow() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(120.0, 120.0, Color::new(0.2, 0.6, 0.9, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        let box_node = node
            .element
            .as_any_mut()
            .downcast_mut::<BoxNode>()
            .unwrap();
        box_node.shadow_color = Some(Animated::new(Color::new(0.0, 0.0, 0.0, 0.6)));
        box_node.shadow_blur = Animated::new(12.0);
        box_node.shadow_offset_x = Animated::new(15.0);
        box_node.shadow_offset_y = Animated::new(15.0);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "effects", "effect_shadow").unwrap();
}

/// Box with a visible border (stroke).
#[test]
fn effect_border() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(120.0, 120.0, Color::new(0.9, 0.9, 0.3, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        let box_node = node
            .element
            .as_any_mut()
            .downcast_mut::<BoxNode>()
            .unwrap();
        box_node.border_width = Animated::new(6.0);
        box_node.border_color = Some(Animated::new(Color::new(0.2, 0.2, 0.2, 1.0)));
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "effects", "effect_border").unwrap();
}

/// Box with rounded corners (border radius).
#[test]
fn effect_border_radius() {
    use crate::visual::assert_visual_match;

    let (director_arc, _, box_id) =
        setup_scene_with_box(120.0, 120.0, Color::new(0.3, 0.8, 0.4, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        let box_node = node
            .element
            .as_any_mut()
            .downcast_mut::<BoxNode>()
            .unwrap();
        box_node.border_radius = Animated::new(30.0);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "effects", "effect_border_radius").unwrap();
}

/// Overflow:hidden clips a child box that extends beyond the parent bounds.
/// Parent: 100x100 red box with overflow hidden.
/// Child: 150x150 blue box that overflows parent.
/// Expected: blue is clipped to the 100x100 parent region.
#[test]
fn effect_overflow_hidden() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 300);
    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::WHITE));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        // Parent box (red) with overflow hidden, 100x100 centered
        let mut parent = BoxNode::new();
        parent.style.size.width = Dimension::length(100.0);
        parent.style.size.height = Dimension::length(100.0);
        parent.bg_color = Some(Animated::new(Color::new(0.9, 0.2, 0.2, 1.0)));
        parent.overflow = "hidden".to_string();
        let parent_id = d.scene.add_node(Box::new(parent));
        d.scene.add_child(root_id, parent_id);

        // Child box (blue) that overflows — 150x150, offset so it spills out
        let mut child = BoxNode::new();
        child.style.size.width = Dimension::length(150.0);
        child.style.size.height = Dimension::length(150.0);
        child.style.position = taffy::style::Position::Absolute;
        child.style.inset.left = LengthPercentageAuto::length(25.0);
        child.style.inset.top = LengthPercentageAuto::length(25.0);
        child.bg_color = Some(Animated::new(Color::new(0.2, 0.3, 0.9, 1.0)));
        let child_id = d.scene.add_node(Box::new(child));
        d.scene.add_child(parent_id, child_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "effects", "effect_overflow_hidden").unwrap();
}

// ============================================================================
// Cross-Feature Interaction Visual Regression Tests
// ============================================================================

/// Blur + shadow + overflow:hidden combined on a single parent box.
/// A rounded parent with blur and shadow clips an overflowing child.
#[test]
fn cross_blur_shadow_overflow() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 300);
    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.9, 0.9, 0.9, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        // Parent with blur, shadow, rounded corners, and overflow hidden
        let mut parent = BoxNode::new();
        parent.style.size.width = Dimension::length(120.0);
        parent.style.size.height = Dimension::length(120.0);
        parent.bg_color = Some(Animated::new(Color::new(0.2, 0.7, 0.4, 1.0)));
        parent.border_radius = Animated::new(24.0);
        parent.blur = Animated::new(8.0);
        parent.shadow_color = Some(Animated::new(Color::new(0.0, 0.0, 0.0, 0.5)));
        parent.shadow_blur = Animated::new(12.0);
        parent.shadow_offset_x = Animated::new(10.0);
        parent.shadow_offset_y = Animated::new(10.0);
        parent.overflow = "hidden".to_string();
        let parent_id = d.scene.add_node(Box::new(parent));
        d.scene.add_child(root_id, parent_id);

        // Child that overflows
        let mut child = BoxNode::new();
        child.style.size.width = Dimension::length(180.0);
        child.style.size.height = Dimension::length(180.0);
        child.style.position = taffy::style::Position::Absolute;
        child.style.inset.left = LengthPercentageAuto::length(20.0);
        child.style.inset.top = LengthPercentageAuto::length(20.0);
        child.bg_color = Some(Animated::new(Color::new(0.9, 0.3, 0.3, 1.0)));
        let child_id = d.scene.add_node(Box::new(child));
        d.scene.add_child(parent_id, child_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "effects", "cross_blur_shadow_overflow").unwrap();
}

/// Animated border on a rounded box: border_width grows while border_radius is already large.
/// Captured at t=1.0 (mid-animation) to verify border follows rounded contour.
#[test]
fn cross_animated_border_radius() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 300);
    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::WHITE));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let mut box_node = BoxNode::new();
        box_node.style.size.width = Dimension::length(120.0);
        box_node.style.size.height = Dimension::length(120.0);
        box_node.bg_color = Some(Animated::new(Color::new(0.3, 0.5, 0.9, 1.0)));
        box_node.border_radius = Animated::new(40.0);
        box_node.border_width = Animated::new(0.0);
        box_node.border_color = Some(Animated::new(Color::new(0.9, 0.2, 0.2, 1.0)));
        let box_id = d.scene.add_node(Box::new(box_node));
        d.scene.add_child(root_id, box_id);

        // Animate border width from 0 → 12 over 2s
        let node = d.scene.get_node_mut(box_id).unwrap();
        node.element
            .animate_property("border_width", 0.0, 12.0, 2.0, "linear");
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "effects", "cross_animated_border_radius").unwrap();
}

/// Text inside a rotated/scaled container.
/// The parent box is rotated 30° and scaled 1.2×; text should follow the transform.
#[test]
fn cross_text_in_rotated() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 300);
    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        // Parent box with rotation and scale
        let mut parent = BoxNode::new();
        parent.style.size.width = Dimension::length(160.0);
        parent.style.size.height = Dimension::length(100.0);
        parent.bg_color = Some(Animated::new(Color::new(0.2, 0.4, 0.7, 1.0)));
        let parent_id = d.scene.add_node(Box::new(parent));
        d.scene.add_child(root_id, parent_id);

        if let Some(n) = d.scene.get_node_mut(parent_id) {
            n.transform.rotation.current_value = 30.0;
            n.transform.scale_x.current_value = 1.2;
            n.transform.scale_y.current_value = 1.2;
        }

        // Text child
        let spans = vec![TextSpan {
            text: "Rotated".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(28.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(parent_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "effects", "cross_text_in_rotated").unwrap();
}
