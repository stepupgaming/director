// Text and typography: face, wrap, alignment, weight, italic, stroke, shrink-to-fit.

use director_core::animation::Animated;
use director_core::element::{TextFit, TextSpan};
use director_core::node::{BoxNode, TextNode};
use director_core::types::Color;
use taffy::style::Dimension;

#[test]
fn text_basic() {
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

        let spans = vec![TextSpan {
            text: "Hello World".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(32.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_basic").unwrap();
}

#[test]
fn text_multiline() {
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

        let spans = vec![TextSpan {
            text: "This is a longer piece of text that should wrap across \
                   multiple lines when rendered in a constrained layout box."
                .to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(20.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.style.size.width = Dimension::length(250.0);
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_multiline").unwrap();
}

#[test]
fn text_align_center() {
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

        let spans = vec![TextSpan {
            text: "Center aligned text should be centered within its container, with each line wrapping and maintaining equal space on both sides.".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(20.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.set_text_align(skia_safe::textlayout::TextAlign::Center);
        text_node.style.size.width = Dimension::length(260.0);
        text_node.style.size.height = Dimension::auto();
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_align_center").unwrap();
}

#[test]
fn text_stroke() {
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

        let spans = vec![TextSpan {
            text: "STROKE".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(48.0),
            color: Some(Color::WHITE),
            stroke_width: Some(3.0),
            stroke_color: Some(Color::new(0.9, 0.2, 0.2, 1.0)),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_stroke").unwrap();
}

#[test]
fn text_textfit_shrink() {
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

        let spans = vec![TextSpan {
            text: "This text is too large for the box and should shrink.".to_string(),
            font_family: Some("Roboto".to_string()),
            font_size: Some(80.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }];

        let fc = d.assets.font_collection.clone();
        let mut text_node = TextNode::new(spans, fc);
        text_node.fit_mode = TextFit::Shrink;
        text_node.min_size = 8.0;
        text_node.max_size = 80.0;
        text_node.style.size.width = Dimension::length(200.0);
        text_node.style.size.height = Dimension::length(60.0);
        let text_id = d.scene.add_node(Box::new(text_node));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "text_textfit_shrink").unwrap();
}

#[test]
fn font_weight_matrix() {
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

        let spans = vec![
            TextSpan {
                text: "400 ".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(24.0),
                font_weight: Some(400),
                color: Some(Color::WHITE),
                ..Default::default()
            },
            TextSpan {
                text: "700 ".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(24.0),
                font_weight: Some(700),
                color: Some(Color::WHITE),
                ..Default::default()
            },
            TextSpan {
                text: "900".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(24.0),
                font_weight: Some(900),
                color: Some(Color::WHITE),
                ..Default::default()
            },
        ];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "font_weight_matrix").unwrap();
}

#[test]
fn font_style_italic() {
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

        let spans = vec![
            TextSpan {
                text: "Upright ".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(28.0),
                color: Some(Color::WHITE),
                ..Default::default()
            },
            TextSpan {
                text: "Italic".to_string(),
                font_family: Some("Roboto".to_string()),
                font_size: Some(28.0),
                font_style: Some("italic".to_string()),
                color: Some(Color::WHITE),
                ..Default::default()
            },
        ];

        let fc = d.assets.font_collection.clone();
        let text_id = d.scene.add_node(Box::new(TextNode::new(spans, fc)));
        d.scene.add_child(root_id, text_id);
    }

    let mut d = director_arc.lock().unwrap();
    assert_visual_match(&mut d, 0.0, "text", "font_style_italic").unwrap();
}
