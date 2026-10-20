//! A live Director exports into a scene document that survives JSON.

use director_core::animation::EasingType;
use director_core::node::{BoxNode, TextNode};
use director_core::scene_export::export_scene;
use director_core::types::Color;
use director_core::video_wrapper::RenderMode;
use director_core::{DefaultAssetLoader, Director};
use director_scene::{NodeBody, SceneDocument};
use std::sync::Arc;
use taffy::style::Dimension;

fn director() -> Director {
    Director::new(
        320,
        180,
        24.0,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    )
}

#[test]
fn an_empty_director_round_trips() {
    let document = export_scene(&director());
    let parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    assert!(parsed.nodes.is_empty());
    assert_eq!(parsed.width, 320);
    assert_eq!(parsed.fps, 24.0);
}

#[test]
fn a_box_and_text_keep_channels_and_body() {
    let mut director = director();
    let mut box_node = BoxNode::new();
    box_node.style.size.width = Dimension::length(40.0);
    box_node.style.size.height = Dimension::length(20.0);
    box_node.bg_color = Some(director_core::animation::Animated::new(Color::new(1.0, 0.0, 0.0, 1.0)));
    box_node
        .opacity
        .add_keyframe(0.25, 1.0, EasingType::EaseOut);
    let box_id = director.scene.add_node(Box::new(box_node));

    let text = TextNode::new(
        vec![director_core::element::TextSpan {
            text: "Hi".to_string(),
            font_size: Some(18.0),
            color: Some(Color::WHITE),
            ..Default::default()
        }],
        director.assets.font_collection.clone(),
    );
    let text_id = director.scene.add_node(Box::new(text));
    director.scene.add_child(box_id, text_id);

    let document = export_scene(&director);
    let parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);

    let exported_box = parsed
        .nodes
        .iter()
        .find(|node| node.kind == "box")
        .unwrap();
    assert_eq!(
        exported_box.body,
        NodeBody::Box {
            background: Some(director_scene::Rgba {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }),
            width: Some(40.0),
            height: Some(20.0),
        }
    );
    assert_eq!(exported_box.opacity.keyframes.len(), 2);
    assert_eq!(exported_box.opacity.keyframes[1].easing, "ease_out");
    assert_eq!(exported_box.children.len(), 1);

    let exported_text = parsed
        .nodes
        .iter()
        .find(|node| node.kind == "text")
        .unwrap();
    assert_eq!(
        exported_text.body,
        NodeBody::Text {
            content: "Hi".to_string(),
            font_size: 18.0,
            color: director_scene::Rgba {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
        }
    );
    assert_eq!(exported_text.parent.unwrap().index, exported_box.id.index);
}
