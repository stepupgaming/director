use director_core::animation::Animated;
use director_core::node::BoxNode;
use director_core::types::{Color, GradientConfig};
use taffy::style::Dimension;

#[test]
fn basic_box() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(200, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root_node = BoxNode::new();
        root_node.style.size.width = Dimension::percent(1.0);
        root_node.style.size.height = Dimension::percent(1.0);
        let root_id = d.scene.add_node(Box::new(root_node));

        d.push_scene_root(root_id, 5.0);

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

        d.push_scene_root(root_id, 5.0);

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
