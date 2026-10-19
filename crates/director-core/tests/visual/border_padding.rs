// Padding inset and a card that combines border, radius, and padding.

use director_core::animation::Animated;
use director_core::node::BoxNode;
use director_core::types::Color;
use taffy::style::Dimension;

#[test]
fn padding_basic() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(200, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.15, 0.15, 0.2, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        let mut parent = BoxNode::new();
        parent.style.size.width = Dimension::length(160.0);
        parent.style.size.height = Dimension::length(120.0);
        parent.bg_color = Some(Animated::new(Color::new(0.3, 0.3, 0.35, 1.0)));
        parent.style.padding.left = taffy::style::LengthPercentage::length(24.0);
        parent.style.padding.right = taffy::style::LengthPercentage::length(24.0);
        parent.style.padding.top = taffy::style::LengthPercentage::length(16.0);
        parent.style.padding.bottom = taffy::style::LengthPercentage::length(16.0);

        let parent_id = d.scene.add_node(Box::new(parent));
        d.scene.add_child(root_id, parent_id);

        let mut child = BoxNode::new();
        child.style.size.width = Dimension::percent(1.0);
        child.style.size.height = Dimension::percent(1.0);
        child.bg_color = Some(Animated::new(Color::new(0.9, 0.5, 0.2, 1.0)));

        let child_id = d.scene.add_node(Box::new(child));
        d.scene.add_child(parent_id, child_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "border_padding", "padding_basic").unwrap();
}

#[test]
fn border_padding_combined() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 240);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.08, 0.08, 0.12, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        let mut card = BoxNode::new();
        card.style.size.width = Dimension::length(240.0);
        card.style.size.height = Dimension::length(160.0);
        card.bg_color = Some(Animated::new(Color::new(0.15, 0.15, 0.22, 1.0)));
        card.border_radius = Animated::new(20.0);
        card.border_width = Animated::new(2.0);
        card.border_color = Some(Animated::new(Color::new(0.4, 0.3, 0.6, 1.0)));
        card.style.padding.left = taffy::style::LengthPercentage::length(20.0);
        card.style.padding.right = taffy::style::LengthPercentage::length(20.0);
        card.style.padding.top = taffy::style::LengthPercentage::length(16.0);
        card.style.padding.bottom = taffy::style::LengthPercentage::length(16.0);

        let card_id = d.scene.add_node(Box::new(card));
        d.scene.add_child(root_id, card_id);

        let mut inner = BoxNode::new();
        inner.style.size.width = Dimension::percent(1.0);
        inner.style.size.height = Dimension::percent(1.0);
        inner.bg_color = Some(Animated::new(Color::new(0.25, 0.2, 0.35, 1.0)));
        inner.border_radius = Animated::new(8.0);

        let inner_id = d.scene.add_node(Box::new(inner));
        d.scene.add_child(card_id, inner_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "border_padding", "border_padding_combined").unwrap();
}
