use director_core::animation::Animated;
use director_core::director::TimelineItem;
use director_core::node::BoxNode;
use director_core::types::Color;
use skia_safe::BlendMode;
use taffy::style::Dimension;

// Standard Test (Ported)
#[test]
fn basic_box() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(200, 200);

    {
        let mut d = director_arc.lock().unwrap();

        // Root
        let mut root_node = BoxNode::new();
        root_node.style.size.width = Dimension::percent(1.0);
        root_node.style.size.height = Dimension::percent(1.0);
        let root_id = d.scene.add_node(Box::new(root_node));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
            freeze_time: None,
        });

        // Child
        let mut box_node = BoxNode::new();
        box_node.style.size.width = Dimension::length(100.0);
        box_node.style.size.height = Dimension::length(100.0);
        box_node.style.margin.left = taffy::style::LengthPercentageAuto::length(50.0);
        box_node.style.margin.top = taffy::style::LengthPercentageAuto::length(50.0);
        box_node.bg_color = Some(Animated::new(Color::new(0.0, 0.0, 1.0, 1.0)));

        let id = d.scene.add_node(Box::new(box_node));
        d.scene.add_child(root_id, id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "elements", "basic_box").unwrap();
}

// Matrix Test: Blend Modes
crate::visual_test_matrix!(
    name: blend_modes,
    suite: "elements",
    variations: [
        (blend_multiply, BlendMode::Multiply),
        (blend_screen, BlendMode::Screen),
        (blend_overlay, BlendMode::Overlay),
    ],
    setup: |d: &mut Director, mode: BlendMode| {
        // Setup a scene with two overlapping boxes to show blending
        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        // White background
        root.bg_color = Some(Animated::new(Color::new(1.0, 1.0, 1.0, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
            freeze_time: None,
        });

        // Bottom Box (Red)
        let mut box1 = BoxNode::new();
        box1.style.size.width = Dimension::length(100.0);
        box1.style.size.height = Dimension::length(100.0);
        box1.style.position = taffy::style::Position::Absolute;
        box1.style.inset.left = taffy::style::LengthPercentageAuto::length(50.0);
        box1.style.inset.top = taffy::style::LengthPercentageAuto::length(50.0);
        box1.bg_color = Some(Animated::new(Color::new(1.0, 0.0, 0.0, 1.0)));
        let id1 = d.scene.add_node(Box::new(box1));
        d.scene.add_child(root_id, id1);

        // Top Box (Blue with BlendMode)
        let mut box2 = BoxNode::new();
        box2.style.size.width = Dimension::length(100.0);
        box2.style.size.height = Dimension::length(100.0);
        box2.style.position = taffy::style::Position::Absolute;
        box2.style.inset.left = taffy::style::LengthPercentageAuto::length(100.0); // Overlap by 50px
        box2.style.inset.top = taffy::style::LengthPercentageAuto::length(100.0);
        box2.bg_color = Some(Animated::new(Color::new(0.0, 0.0, 1.0, 1.0)));
        // blend_mode is on SceneNode, not Element

        let id2 = d.scene.add_node(Box::new(box2));
        d.scene.add_child(root_id, id2);

        // Update SceneNode blend_mode
        if let Some(node) = d.scene.get_node_mut(id2) {
            node.blend_mode = mode;
        }
    }
);

// ============================================================================
// Box Gradient Visual Regression Test
// ============================================================================

use director_core::types::GradientConfig;

/// Box with a diagonal gradient background.
#[test]
fn box_gradient_diagonal() {
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
            freeze_time: None,
        });

        let mut box_node = BoxNode::new();
        box_node.style.size.width = Dimension::length(150.0);
        box_node.style.size.height = Dimension::length(150.0);
        box_node.border_radius = Animated::new(20.0);
        box_node.bg_gradient = Some(GradientConfig {
            colors: vec![
                Color::new(0.9, 0.2, 0.2, 1.0),
                Color::new(0.2, 0.2, 0.9, 1.0),
            ],
            positions: None,
            start: (0.0, 0.0),
            end: (1.0, 1.0),
            ..Default::default()
        });
        let box_id = d.scene.add_node(Box::new(box_node));
        d.scene.add_child(root_id, box_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "elements", "box_gradient_diagonal").unwrap();
}
