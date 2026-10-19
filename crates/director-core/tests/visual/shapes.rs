// Circle fill and a filled triangle path.

use director_core::animation::Animated;
use director_core::node::{BoxNode, PathCommand, ShapeNode, ShapeType};
use director_core::types::Color;
use taffy::style::Dimension;

#[test]
fn shape_circle_fill() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        let mut shape = ShapeNode::new(ShapeType::Circle {
            cx: 150.0,
            cy: 100.0,
            r: 60.0,
        });
        shape.fill_color = Some(Animated::new(Color::new(0.9, 0.2, 0.2, 1.0)));
        let shape_id = d.scene.add_node(Box::new(shape));
        d.scene.add_child(root_id, shape_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "shapes", "shape_circle_fill").unwrap();
}

#[test]
fn shape_path_triangle() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        let commands = vec![
            PathCommand::MoveTo { x: 150.0, y: 30.0 },
            PathCommand::LineTo { x: 250.0, y: 160.0 },
            PathCommand::LineTo { x: 50.0, y: 160.0 },
            PathCommand::Close,
        ];
        let mut shape = ShapeNode::new(ShapeType::Path { commands });
        shape.fill_color = Some(Animated::new(Color::new(0.9, 0.6, 0.1, 1.0)));
        let shape_id = d.scene.add_node(Box::new(shape));
        d.scene.add_child(root_id, shape_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "shapes", "shape_path_triangle").unwrap();
}
