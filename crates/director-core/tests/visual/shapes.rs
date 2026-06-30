// Shape Node Visual Regression Tests
//
// Tests for geometric shape rendering: circles, ellipses, paths,
// with fill, stroke, and gradient support.

use director_core::animation::Animated;
use director_core::director::TimelineItem;
use director_core::node::{BoxNode, PathCommand, ShapeNode, ShapeType};
use director_core::types::{Color, GradientConfig};
use taffy::style::Dimension;

/// Solid red circle on a dark background.
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

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let shape = ShapeNode::new(ShapeType::Circle { cx: 150.0, cy: 100.0, r: 60.0 });
        let mut shape = shape;
        shape.fill_color = Some(Animated::new(Color::new(0.9, 0.2, 0.2, 1.0)));
        let shape_id = d.scene.add_node(Box::new(shape));
        d.scene.add_child(root_id, shape_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "shapes", "shape_circle_fill").unwrap();
}

/// Circle with stroke but no fill.
#[test]
fn shape_circle_stroke_only() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

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

        let mut shape = ShapeNode::new(ShapeType::Circle { cx: 150.0, cy: 100.0, r: 60.0 });
        shape.fill_color = None;
        shape.stroke_color = Some(Animated::new(Color::new(0.2, 0.8, 0.2, 1.0)));
        shape.stroke_width = Animated::new(4.0);
        let shape_id = d.scene.add_node(Box::new(shape));
        d.scene.add_child(root_id, shape_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "shapes", "shape_circle_stroke_only").unwrap();
}

/// Circle with both fill and stroke.
#[test]
fn shape_circle_fill_and_stroke() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

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

        let mut shape = ShapeNode::new(ShapeType::Circle { cx: 150.0, cy: 100.0, r: 60.0 });
        shape.fill_color = Some(Animated::new(Color::new(0.2, 0.5, 0.9, 1.0)));
        shape.stroke_color = Some(Animated::new(Color::new(1.0, 1.0, 1.0, 1.0)));
        shape.stroke_width = Animated::new(3.0);
        let shape_id = d.scene.add_node(Box::new(shape));
        d.scene.add_child(root_id, shape_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "shapes", "shape_circle_fill_and_stroke").unwrap();
}

/// Solid blue ellipse.
#[test]
fn shape_ellipse_fill() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

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

        let mut shape = ShapeNode::new(ShapeType::Ellipse { cx: 150.0, cy: 100.0, rx: 100.0, ry: 50.0 });
        shape.fill_color = Some(Animated::new(Color::new(0.2, 0.4, 0.9, 1.0)));
        let shape_id = d.scene.add_node(Box::new(shape));
        d.scene.add_child(root_id, shape_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "shapes", "shape_ellipse_fill").unwrap();
}

/// Ellipse with gradient fill.
#[test]
fn shape_ellipse_gradient() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

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

        let mut shape = ShapeNode::new(ShapeType::Ellipse { cx: 150.0, cy: 100.0, rx: 100.0, ry: 50.0 });
        shape.fill_gradient = Some(GradientConfig {
            colors: vec![
                Color::new(0.9, 0.2, 0.2, 1.0),
                Color::new(0.2, 0.2, 0.9, 1.0),
            ],
            positions: None,
            start: (0.0, 0.0),
            end: (1.0, 1.0),
        });
        let shape_id = d.scene.add_node(Box::new(shape));
        d.scene.add_child(root_id, shape_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "shapes", "shape_ellipse_gradient").unwrap();
}

/// Simple triangle path with fill.
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

        d.timeline.push(TimelineItem {
            scene_root: root_id,
            name: None,
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

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

/// Path with quadratic bezier curve, fill and stroke.
#[test]
fn shape_path_quad_curve() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

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

        let commands = vec![
            PathCommand::MoveTo { x: 30.0, y: 100.0 },
            PathCommand::QuadTo { cx: 150.0, cy: 20.0, x: 270.0, y: 100.0 },
            PathCommand::QuadTo { cx: 150.0, cy: 180.0, x: 30.0, y: 100.0 },
            PathCommand::Close,
        ];
        let mut shape = ShapeNode::new(ShapeType::Path { commands });
        shape.fill_color = Some(Animated::new(Color::new(0.2, 0.7, 0.4, 1.0)));
        shape.stroke_color = Some(Animated::new(Color::new(1.0, 1.0, 1.0, 1.0)));
        shape.stroke_width = Animated::new(2.0);
        let shape_id = d.scene.add_node(Box::new(shape));
        d.scene.add_child(root_id, shape_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "shapes", "shape_path_quad_curve").unwrap();
}

/// Path with cubic bezier curve forming a heart-like shape.
#[test]
fn shape_path_cubic_curve() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

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

        let commands = vec![
            PathCommand::MoveTo { x: 150.0, y: 60.0 },
            PathCommand::CubicTo { c1x: 60.0, c1y: 10.0, c2x: 30.0, c2y: 110.0, x: 150.0, y: 170.0 },
            PathCommand::CubicTo { c1x: 270.0, c1y: 110.0, c2x: 240.0, c2y: 10.0, x: 150.0, y: 60.0 },
            PathCommand::Close,
        ];
        let mut shape = ShapeNode::new(ShapeType::Path { commands });
        shape.fill_color = Some(Animated::new(Color::new(0.9, 0.2, 0.4, 1.0)));
        let shape_id = d.scene.add_node(Box::new(shape));
        d.scene.add_child(root_id, shape_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "shapes", "shape_path_cubic_curve").unwrap();
}

/// Path with arc command forming a pie slice.
#[test]
fn shape_path_arc() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(300, 200);

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

        let commands = vec![
            PathCommand::MoveTo { x: 150.0, y: 100.0 },
            PathCommand::ArcTo { cx: 150.0, cy: 100.0, rx: 80.0, ry: 80.0, start_angle: 0.0, sweep_angle: 120.0 },
            PathCommand::Close,
        ];
        let mut shape = ShapeNode::new(ShapeType::Path { commands });
        shape.fill_color = Some(Animated::new(Color::new(0.5, 0.2, 0.8, 1.0)));
        shape.stroke_color = Some(Animated::new(Color::new(1.0, 1.0, 1.0, 1.0)));
        shape.stroke_width = Animated::new(2.0);
        let shape_id = d.scene.add_node(Box::new(shape));
        d.scene.add_child(root_id, shape_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "shapes", "shape_path_arc").unwrap();
}
