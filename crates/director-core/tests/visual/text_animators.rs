// Per-glyph opacity reveal, captured while the stagger is in progress.

use director_core::animation::Animated;
use director_core::element::{Element, TextSpan};
use director_core::node::{BoxNode, TextNode};
use director_core::types::Color;
use taffy::style::Dimension;

#[test]
fn karaoke_fade() {
    use crate::visual::{assert_visual_match, setup_test_director};

    let director_arc = setup_test_director(400, 150);
    {
        let mut d = director_arc.lock().unwrap();

        let mut root = BoxNode::new();
        root.style.size.width = Dimension::percent(1.0);
        root.style.size.height = Dimension::percent(1.0);
        root.bg_color = Some(Animated::new(Color::new(0.1, 0.1, 0.1, 1.0)));
        let root_id = d.scene.add_node(Box::new(root));

        d.push_scene_root(root_id, 5.0);

        let spans = vec![TextSpan {
            text: "KARAOKE".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(48.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.style.size.width = Dimension::auto();
        text_node.style.size.height = Dimension::auto();

        let char_count = text_node.get_total_char_count();
        text_node.add_text_animator_full(
            0,
            char_count,
            "opacity".to_string(),
            0.0,
            1.0,
            2.0,
            "linear",
            0.15,
        );

        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 1.0, "text_animators", "karaoke_fade").unwrap();
}
