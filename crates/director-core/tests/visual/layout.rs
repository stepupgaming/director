use director_core::animation::Animated;
use director_core::node::BoxNode;
use director_core::types::Color;
use director_core::Director;
use taffy::style::{AlignItems, Dimension, FlexDirection, JustifyContent};

// Matrix Test: Flex Row with JustifyContent
crate::visual_test_matrix!(
    name: layout_flex_row,
    suite: "layout",
    variations: [
        (row_space_between, JustifyContent::SpaceBetween),
    ],
    setup: |d: &mut Director, justify: JustifyContent| {
        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.style.display = taffy::style::Display::Flex;
        root.style.flex_direction = FlexDirection::Row;
        root.style.align_items = Some(AlignItems::Center);
        root.style.justify_content = Some(justify);
        root.bg_color = Some(Animated::new(Color::new(0.9, 0.9, 0.9, 1.0)));

        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        let colors = [
            Color::new(0.9, 0.2, 0.2, 1.0),
            Color::new(0.2, 0.8, 0.2, 1.0),
            Color::new(0.2, 0.2, 0.9, 1.0),
        ];

        for (i, color) in colors.iter().enumerate() {
            let mut item = BoxNode::new();
            let w = 40.0 + (i as f32 * 30.0);
            item.style.size.width = Dimension::length(w);
            item.style.size.height = Dimension::length(60.0);
            item.bg_color = Some(Animated::new(*color));

            let id = d.scene.add_node(Box::new(item));
            d.scene.add_child(root_id, id);
        }
    }
);

// Matrix Test: Flex Layout Alignments
crate::visual_test_matrix!(
    name: layout_alignments,
    suite: "layout",
    variations: [
        (align_center, AlignItems::Center),
    ],
    setup: |d: &mut Director, align: AlignItems| {
        // Root Container
        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.style.display = taffy::style::Display::Flex;
        root.style.flex_direction = FlexDirection::Column;
        root.style.align_items = Some(align); // Apply variation
        root.style.justify_content = Some(JustifyContent::Center);
        root.bg_color = Some(Animated::new(Color::new(0.9, 0.9, 0.9, 1.0)));

        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        // Child Items of different sizes
        for i in 0..3 {
            let mut item = BoxNode::new();
            let size = 50.0 + (i as f32 * 20.0);
            item.style.size.width = Dimension::length(size);
            item.style.size.height = Dimension::length(50.0);
            item.bg_color = Some(Animated::new(Color::new(0.0, 0.0, 0.0, 1.0)));
            // Margin to see separation
            item.style.margin.bottom = taffy::style::LengthPercentageAuto::length(10.0);

            let id = d.scene.add_node(Box::new(item));
            d.scene.add_child(root_id, id);
        }
    }
);
