// Z-Index Visual Regression Tests
//
// Tests that z_index correctly controls render order independent of
// node insertion order. Without z_index, nodes render in document order.
// With z_index, higher values render on top of lower values.

use director_core::animation::Animated;
use director_core::node::BoxNode;
use director_core::types::Color;
use taffy::style::Dimension;

/// Two overlapping boxes: blue inserted first, then red.
/// Red has z_index=1, blue has z_index=0.
/// Expected: red box renders on top of blue box.
#[test]
fn z_index_higher_on_top() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(200, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        // Blue box (inserted first, z_index=0)
        let mut blue = BoxNode::new();
        blue.style.size.width = Dimension::length(120.0);
        blue.style.size.height = Dimension::length(120.0);
        blue.bg_color = Some(Animated::new(Color::new(0.2, 0.4, 0.9, 1.0)));
        blue.style.position = taffy::style::Position::Absolute;
        blue.style.inset.left = taffy::style::LengthPercentageAuto::length(30.0);
        blue.style.inset.top = taffy::style::LengthPercentageAuto::length(30.0);
        let blue_id = d.scene.add_node(Box::new(blue));
        d.scene.add_child(root_id, blue_id);
        if let Some(n) = d.scene.get_node_mut(blue_id) {
            n.z_index = 0;
        }

        // Red box (inserted second, z_index=1)
        let mut red = BoxNode::new();
        red.style.size.width = Dimension::length(120.0);
        red.style.size.height = Dimension::length(120.0);
        red.bg_color = Some(Animated::new(Color::new(0.9, 0.2, 0.2, 1.0)));
        red.style.position = taffy::style::Position::Absolute;
        red.style.inset.left = taffy::style::LengthPercentageAuto::length(50.0);
        red.style.inset.top = taffy::style::LengthPercentageAuto::length(50.0);
        let red_id = d.scene.add_node(Box::new(red));
        d.scene.add_child(root_id, red_id);
        if let Some(n) = d.scene.get_node_mut(red_id) {
            n.z_index = 1;
        }
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "z_index", "z_index_higher_on_top").unwrap();
}

/// Two overlapping boxes: red inserted first with z_index=2,
/// blue inserted second with z_index=1.
/// Expected: red (higher z_index) still renders on top despite
/// being inserted first.
#[test]
fn z_index_overrides_insertion_order() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(200, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        // Red box (inserted first, z_index=2)
        let mut red = BoxNode::new();
        red.style.size.width = Dimension::length(120.0);
        red.style.size.height = Dimension::length(120.0);
        red.bg_color = Some(Animated::new(Color::new(0.9, 0.2, 0.2, 1.0)));
        red.style.position = taffy::style::Position::Absolute;
        red.style.inset.left = taffy::style::LengthPercentageAuto::length(30.0);
        red.style.inset.top = taffy::style::LengthPercentageAuto::length(30.0);
        let red_id = d.scene.add_node(Box::new(red));
        d.scene.add_child(root_id, red_id);
        if let Some(n) = d.scene.get_node_mut(red_id) {
            n.z_index = 2;
        }

        // Blue box (inserted second, z_index=1)
        let mut blue = BoxNode::new();
        blue.style.size.width = Dimension::length(120.0);
        blue.style.size.height = Dimension::length(120.0);
        blue.bg_color = Some(Animated::new(Color::new(0.2, 0.4, 0.9, 1.0)));
        blue.style.position = taffy::style::Position::Absolute;
        blue.style.inset.left = taffy::style::LengthPercentageAuto::length(50.0);
        blue.style.inset.top = taffy::style::LengthPercentageAuto::length(50.0);
        let blue_id = d.scene.add_node(Box::new(blue));
        d.scene.add_child(root_id, blue_id);
        if let Some(n) = d.scene.get_node_mut(blue_id) {
            n.z_index = 1;
        }
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "z_index", "z_index_overrides_insertion_order").unwrap();
}
