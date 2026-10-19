// Animation mid-frames: opacity, scale, rotation, and translation at t=1.0.

use director_core::animation::{Animated, EasingType};
use director_core::node::BoxNode;
use director_core::types::Color;
use taffy::style::Dimension;

fn setup_scene_with_box(
    bg: Color,
    box_color: Color,
) -> (
    std::sync::Arc<std::sync::Mutex<director_core::Director>>,
    director_core::types::NodeId,
) {
    use crate::visual::setup_test_director;

    let director_arc = setup_test_director(300, 300);
    let box_id;
    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(bg));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        let mut box_node = BoxNode::new();
        box_node.style.size.width = Dimension::length(100.0);
        box_node.style.size.height = Dimension::length(100.0);
        box_node.bg_color = Some(Animated::new(box_color));
        box_id = d.scene.add_node(Box::new(box_node));
        d.scene.add_child(root_id, box_id);
    }
    (director_arc, box_id)
}

#[test]
fn anim_opacity_mid() {
    use crate::visual::assert_visual_match;

    let (director_arc, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.8, 0.2, 0.2, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        director_core::Element::animate_property(
            node.element.as_mut(),
            "opacity",
            1.0,
            0.0,
            2.0,
            "linear",
        )
        .expect("animate opacity");
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "animations", "anim_opacity_mid").unwrap();
}

#[test]
fn anim_scale_mid() {
    use crate::visual::assert_visual_match;

    let (director_arc, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.2, 0.8, 0.2, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        node.transform
            .scale_x
            .add_segment(1.0, 3.0, 2.0, EasingType::Linear);
        node.transform
            .scale_y
            .add_segment(1.0, 3.0, 2.0, EasingType::Linear);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "animations", "anim_scale_mid").unwrap();
}

#[test]
fn anim_rotation_45() {
    use crate::visual::assert_visual_match;

    let (director_arc, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.2, 0.2, 0.8, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        node.transform
            .rotation
            .add_segment(0.0, 90.0, 2.0, EasingType::Linear);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "animations", "anim_rotation_45").unwrap();
}

#[test]
fn anim_translate_mid() {
    use crate::visual::assert_visual_match;

    let (director_arc, box_id) =
        setup_scene_with_box(Color::new(0.85, 0.85, 0.85, 1.0), Color::new(0.9, 0.6, 0.1, 1.0));

    {
        let mut d = director_arc.lock().unwrap();
        let node = d.scene.get_node_mut(box_id).unwrap();
        node.transform
            .translate_x
            .add_segment(0.0, 150.0, 2.0, EasingType::Linear);
        node.transform
            .translate_y
            .add_segment(0.0, 100.0, 2.0, EasingType::Linear);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "animations", "anim_translate_mid").unwrap();
}
