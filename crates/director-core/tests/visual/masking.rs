// Masking Visual Regression Tests
//
// Tests for SceneNode.mask_node which renders a mask with DstIn blend mode
// to clip the masked node to the mask's alpha shape.

use director_core::animation::Animated;
use director_core::director::TimelineItem;
use director_core::node::BoxNode;
use director_core::types::Color;
use taffy::style::Dimension;

/// A red box masked by a smaller white circle (via large border_radius).
/// The result should be a red circle.
#[test]
fn mask_circle() {
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

        // Mask node: a white box with large border radius → circular mask
        let mut mask = BoxNode::new();
        mask.style.size.width = Dimension::length(100.0);
        mask.style.size.height = Dimension::length(100.0);
        mask.bg_color = Some(Animated::new(Color::WHITE));
        mask.border_radius = Animated::new(50.0);
        // Mask is an orphan node (not a visible child) — it only participates via mask_node reference.
        let mask_id = d.scene.add_node(Box::new(mask));

        // Masked node: a larger red square
        let mut masked = BoxNode::new();
        masked.style.size.width = Dimension::length(120.0);
        masked.style.size.height = Dimension::length(120.0);
        masked.bg_color = Some(Animated::new(Color::new(0.9, 0.2, 0.2, 1.0)));
        let masked_id = d.scene.add_node(Box::new(masked));
        d.scene.add_child(root_id, masked_id);

        // Set mask relationship
        if let Some(n) = d.scene.get_node_mut(masked_id) {
            n.mask_node = Some(mask_id);
        }
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "masking", "mask_circle").unwrap();
}

/// A wide horizontal box masked by a vertical strip.
/// The result should be a vertical red bar in the center.
#[test]
fn mask_strip() {
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

        // Mask node: a thin vertical white strip
        let mut mask = BoxNode::new();
        mask.style.size.width = Dimension::length(40.0);
        mask.style.size.height = Dimension::length(200.0);
        mask.bg_color = Some(Animated::new(Color::WHITE));
        // Mask is an orphan node (not a visible child) — it only participates via mask_node reference.
        let mask_id = d.scene.add_node(Box::new(mask));

        // Masked node: a wide horizontal blue bar
        let mut masked = BoxNode::new();
        masked.style.size.width = Dimension::length(200.0);
        masked.style.size.height = Dimension::length(60.0);
        masked.bg_color = Some(Animated::new(Color::new(0.2, 0.3, 0.9, 1.0)));
        let masked_id = d.scene.add_node(Box::new(masked));
        d.scene.add_child(root_id, masked_id);

        // Set mask relationship
        if let Some(n) = d.scene.get_node_mut(masked_id) {
            n.mask_node = Some(mask_id);
        }
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "masking", "mask_strip").unwrap();
}
