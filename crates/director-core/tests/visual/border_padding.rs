// Border & Padding Visual Regression Tests
//
// Tests for BoxNode border rendering, padding content inset, and
// template-like pattern scenarios (cards, pills).
//
// NOTE: All 11 templates (title_card, lower_third, split_screen,
// reaction_overlay, bento_grid, watermark, subscribe_button,
// progress_bar, countdown_timer, quote_card, social_handle_overlay)
// are tested via Rhai script fixtures in `tests/visual/fixtures/templates/`
// and executed by `tests/visual/templates.rs`.

use director_core::animation::Animated;
use director_core::node::{BoxNode, TextNode};
use director_core::element::TextSpan;
use director_core::types::Color;
use skia_safe::textlayout::TextAlign;
use taffy::style::Dimension;

/// Basic border: red box with a 4px white border.
#[test]
fn border_basic() {
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

        let mut box_node = BoxNode::new();
        box_node.style.size.width = Dimension::length(100.0);
        box_node.style.size.height = Dimension::length(100.0);
        box_node.bg_color = Some(Animated::new(Color::new(0.9, 0.2, 0.2, 1.0)));
        box_node.border_width = Animated::new(4.0);
        box_node.border_color = Some(Animated::new(Color::new(1.0, 1.0, 1.0, 1.0)));

        let id = d.scene.add_node(Box::new(box_node));
        d.scene.add_child(root_id, id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "border_padding", "border_basic").unwrap();
}

/// Border with radius: rounded red box with white border.
#[test]
fn border_radius() {
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

        let mut box_node = BoxNode::new();
        box_node.style.size.width = Dimension::length(120.0);
        box_node.style.size.height = Dimension::length(80.0);
        box_node.bg_color = Some(Animated::new(Color::new(0.2, 0.5, 0.9, 1.0)));
        box_node.border_radius = Animated::new(16.0);
        box_node.border_width = Animated::new(3.0);
        box_node.border_color = Some(Animated::new(Color::new(1.0, 1.0, 1.0, 1.0)));

        let id = d.scene.add_node(Box::new(box_node));
        d.scene.add_child(root_id, id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "border_padding", "border_radius").unwrap();
}

/// Padding: inner content should be inset from edges by padding amount.
/// A dark parent contains a bright child; padding on the parent creates a visible gap.
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

/// Combined border + padding: a card-like box with rounded corners,
/// border, and padding that pushes inner content inward.
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

/// Template-style pill button: rounded pill with border and horizontal padding,
/// mimicking the subscribe button template pattern.
#[test]
fn template_pill_button() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(400, 120);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.05, 0.05, 0.08, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        let mut pill = BoxNode::new();
        pill.style.size.width = Dimension::auto();
        pill.style.size.height = Dimension::auto();
        pill.bg_color = Some(Animated::new(Color::new(0.9, 0.15, 0.15, 1.0)));
        pill.border_radius = Animated::new(32.0);
        pill.border_width = Animated::new(2.0);
        pill.border_color = Some(Animated::new(Color::new(1.0, 0.4, 0.4, 1.0)));
        pill.style.padding.left = taffy::style::LengthPercentage::length(28.0);
        pill.style.padding.right = taffy::style::LengthPercentage::length(28.0);
        pill.style.padding.top = taffy::style::LengthPercentage::length(12.0);
        pill.style.padding.bottom = taffy::style::LengthPercentage::length(12.0);
        pill.style.flex_direction = taffy::style::FlexDirection::Row;
        pill.style.align_items = Some(taffy::style::AlignItems::Center);
        pill.style.justify_content = Some(taffy::style::JustifyContent::Center);

        let pill_id = d.scene.add_node(Box::new(pill));
        d.scene.add_child(root_id, pill_id);

        let spans = vec![TextSpan {
            text: "SUBSCRIBE".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(24.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(pill_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "border_padding", "template_pill_button").unwrap();
}

/// Overflow hidden + border radius: child content should be clipped to the
/// rounded corners of the parent. Mimics reaction_overlay facecam pattern.
#[test]
fn overflow_hidden_border_radius() {
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

        // Container with rounded corners and overflow:hidden
        let mut container = BoxNode::new();
        container.style.size.width = Dimension::length(160.0);
        container.style.size.height = Dimension::length(160.0);
        container.bg_color = Some(Animated::new(Color::new(0.2, 0.2, 0.25, 1.0)));
        container.border_radius = Animated::new(24.0);
        container.border_width = Animated::new(3.0);
        container.border_color = Some(Animated::new(Color::new(1.0, 1.0, 1.0, 1.0)));
        container.overflow = "hidden".to_string();

        let container_id = d.scene.add_node(Box::new(container));
        d.scene.add_child(root_id, container_id);

        // Large child that extends beyond the rounded corners
        let mut child = BoxNode::new();
        child.style.size.width = Dimension::length(200.0);
        child.style.size.height = Dimension::length(200.0);
        child.bg_color = Some(Animated::new(Color::new(0.9, 0.3, 0.3, 1.0)));
        child.style.margin.left = taffy::style::LengthPercentageAuto::length(-20.0);
        child.style.margin.top = taffy::style::LengthPercentageAuto::length(-20.0);

        let child_id = d.scene.add_node(Box::new(child));
        d.scene.add_child(container_id, child_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "border_padding", "overflow_hidden_border_radius").unwrap();
}
