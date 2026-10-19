// Blur, drop shadow, color matrix, and vignette.

use director_core::animation::Animated;
use director_core::node::{BoxNode, EffectType};
use director_core::types::Color;
use taffy::style::Dimension;

fn setup_scene_with_box(
    box_w: f32,
    box_h: f32,
    box_color: Color,
) -> (
    std::sync::Arc<std::sync::Mutex<director_core::Director>>,
    director_core::types::NodeId,
) {
    use crate::visual::setup_test_director;

    let director_arc = setup_test_director(300, 300);
    let box_id;
    {
        let mut director = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::WHITE));
        let root_id = director.scene.add_node(Box::new(root));
        director.push_scene_root(root_id, 5.0);

        let mut box_node = BoxNode::new();
        box_node.style.size.width = Dimension::length(box_w);
        box_node.style.size.height = Dimension::length(box_h);
        box_node.bg_color = Some(Animated::new(box_color));
        box_id = director.scene.add_node(Box::new(box_node));
        director.scene.add_child(root_id, box_id);
    }
    (director_arc, box_id)
}

#[test]
fn effect_blur() {
    use crate::visual::assert_visual_match;

    let (director_arc, box_id) =
        setup_scene_with_box(120.0, 120.0, Color::new(0.8, 0.2, 0.2, 1.0));

    {
        let mut director = director_arc.lock().unwrap();
        let node = director.scene.get_node_mut(box_id).unwrap();
        let box_node = director_core::Element::as_any_mut(node.element.as_mut())
            .downcast_mut::<BoxNode>()
            .unwrap();
        box_node.blur = Animated::new(15.0);
    }

    let mut director = director_arc.lock().unwrap();
    assert_visual_match(&mut director, 0.0, "effects", "effect_blur").unwrap();
}

#[test]
fn effect_shadow() {
    use crate::visual::assert_visual_match;

    let (director_arc, box_id) =
        setup_scene_with_box(120.0, 120.0, Color::new(0.2, 0.6, 0.9, 1.0));

    {
        let mut director = director_arc.lock().unwrap();
        let node = director.scene.get_node_mut(box_id).unwrap();
        let box_node = director_core::Element::as_any_mut(node.element.as_mut())
            .downcast_mut::<BoxNode>()
            .unwrap();
        box_node.shadow_color = Some(Animated::new(Color::new(0.0, 0.0, 0.0, 0.6)));
        box_node.shadow_blur = Animated::new(12.0);
        box_node.shadow_offset_x = Animated::new(15.0);
        box_node.shadow_offset_y = Animated::new(15.0);
    }

    let mut director = director_arc.lock().unwrap();
    assert_visual_match(&mut director, 0.0, "effects", "effect_shadow").unwrap();
}

#[test]
fn effect_color_matrix() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(240, 160);
    {
        let mut director = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.style.display = taffy::style::Display::Flex;
        root.style.flex_direction = taffy::style::FlexDirection::Row;
        root.bg_color = Some(Animated::new(Color::WHITE));
        let root_id = director.scene.add_node(Box::new(root));
        director.push_scene_root(root_id, 2.0);

        let mut group = BoxNode::new();
        group.style.size.width = Dimension::percent(1.0);
        group.style.size.height = Dimension::percent(1.0);
        group.style.display = taffy::style::Display::Flex;
        group.style.flex_direction = taffy::style::FlexDirection::Row;
        let group_id = director.scene.add_node(Box::new(group));
        director.scene.add_child(root_id, group_id);

        for color in [
            Color::new(0.9, 0.15, 0.15, 1.0),
            Color::new(0.15, 0.75, 0.2, 1.0),
            Color::new(0.15, 0.25, 0.9, 1.0),
        ] {
            let mut band = BoxNode::new();
            band.style.size.width = Dimension::length(80.0);
            band.style.size.height = Dimension::percent(1.0);
            band.bg_color = Some(Animated::new(color));
            let band_id = director.scene.add_node(Box::new(band));
            director.scene.add_child(group_id, band_id);
        }

        director_core::scripting::utils::apply_effect_to_node(
            &mut director,
            group_id,
            EffectType::ColorMatrix(vec![
                0.2126, 0.7152, 0.0722, 0.0, 0.0, 0.2126, 0.7152, 0.0722, 0.0, 0.0, 0.2126,
                0.7152, 0.0722, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
            ]),
        );
    }

    let mut director = director_arc.lock().unwrap();
    assert_visual_match(&mut director, 0.0, "effects", "effect_color_matrix").unwrap();
}

#[test]
fn effect_vignette() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(240, 160);
    {
        let mut director = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = director.scene.add_node(Box::new(root));
        director.push_scene_root(root_id, 2.0);

        let mut plate = BoxNode::new();
        plate.style.size.width = Dimension::percent(1.0);
        plate.style.size.height = Dimension::percent(1.0);
        plate.bg_color = Some(Animated::new(Color::new(0.95, 0.85, 0.55, 1.0)));
        let plate_id = director.scene.add_node(Box::new(plate));
        director.scene.add_child(root_id, plate_id);

        director_core::scripting::utils::apply_effect_to_node(
            &mut director,
            plate_id,
            EffectType::Vignette {
                intensity: Animated::new(0.95),
                radius: Animated::new(0.7),
                feather: Animated::new(0.45),
            },
        );
    }

    let mut director = director_arc.lock().unwrap();
    assert_visual_match(&mut director, 0.0, "effects", "effect_vignette").unwrap();
}
