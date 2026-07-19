// Composition (Nested Director) Visual Regression Tests
//
// Tests for CompositionNode which renders an isolated sub-timeline to a surface.

use director_core::animation::Animated;
use director_core::director::{Director};
use director_core::node::{BoxNode, CompositionNode};
use director_core::types::Color;
use taffy::style::Dimension;

/// A nested composition containing a blue box rendered inside a parent scene.
#[test]
fn composition_nested() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 300);
    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.9, 0.9, 0.9, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        // Build inner director
        let mut inner = Director::new(
            150,
            150,
            30.0,
            d.assets.loader.clone(),
            director_core::video_wrapper::RenderMode::Preview,
            None,
        );

        let mut inner_root = BoxNode::new();
        inner_root.style.size.width = Dimension::percent(1.0);
        inner_root.style.size.height = Dimension::percent(1.0);
        inner_root.bg_color = Some(Animated::new(Color::new(0.2, 0.5, 0.9, 1.0)));
        let inner_root_id = inner.scene.add_node(Box::new(inner_root));

        inner.push_scene_root(inner_root_id, 5.0);

        let comp_node = CompositionNode::new(inner);
        let comp_id = d.scene.add_node(Box::new(comp_node));
        d.scene.add_child(root_id, comp_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "composition", "composition_nested").unwrap();
}

/// Nested composition with a scaled inner scene showing a colored circle.
#[test]
fn composition_scaled() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 300);
    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::WHITE));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        // Inner director with a smaller canvas and a rounded box
        let mut inner = Director::new(
            100,
            100,
            30.0,
            d.assets.loader.clone(),
            director_core::video_wrapper::RenderMode::Preview,
            None,
        );

        let mut inner_root = BoxNode::new();
        inner_root.style.size.width = Dimension::percent(1.0);
        inner_root.style.size.height = Dimension::percent(1.0);
        inner_root.bg_color = Some(Animated::new(Color::new(0.9, 0.3, 0.3, 1.0)));
        inner_root.border_radius = Animated::new(50.0); // circle-ish
        let inner_root_id = inner.scene.add_node(Box::new(inner_root));

        inner.push_scene_root(inner_root_id, 5.0);

        let mut comp_node = CompositionNode::new(inner);
        // Make the composition fill the parent (scaled up from 100x100 to 300x300)
        comp_node.style.size.width = Dimension::length(200.0);
        comp_node.style.size.height = Dimension::length(200.0);

        let comp_id = d.scene.add_node(Box::new(comp_node));
        d.scene.add_child(root_id, comp_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "composition", "composition_scaled").unwrap();
}
