//! A live Director exports into a scene document that survives JSON.

use director_core::animation::EasingType;
use director_core::node::{BoxNode, TextNode};
use director_core::scene_export::export_scene;
use director_core::types::Color;
use director_core::video_wrapper::RenderMode;
use director_core::{DefaultAssetLoader, Director};
use director_scene::layout::Dimension;
use director_scene::{Easing, NodeBody, Rgba, SceneDocument};
use std::sync::Arc;
use taffy::style::Dimension as TaffyDimension;

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
    let document = export_scene(&director()).unwrap();
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
    box_node.style.size.width = TaffyDimension::length(40.0);
    box_node.style.size.height = TaffyDimension::length(20.0);
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

    let document = export_scene(&director).unwrap();
    let parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);

    let exported_box = parsed
        .nodes
        .iter()
        .find(|node| node.kind == "box")
        .unwrap();
    let NodeBody::Box(body) = &exported_box.body else {
        panic!("box exported as {:?}", exported_box.body);
    };
    let background = body.background.as_ref().unwrap();
    assert_eq!(background.keyframes[0].value, Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 });
    assert_eq!(exported_box.layout.size.width, Dimension::Px(40.0));
    assert_eq!(exported_box.layout.size.height, Dimension::Px(20.0));
    assert_eq!(exported_box.opacity.keyframes.len(), 2);
    assert_eq!(exported_box.opacity.keyframes[1].easing, Easing::EaseOut);
    assert_eq!(exported_box.children.len(), 1);

    let exported_text = parsed
        .nodes
        .iter()
        .find(|node| node.kind == "text")
        .unwrap();
    let NodeBody::Text(text) = &exported_text.body else {
        panic!("text exported as {:?}", exported_text.body);
    };
    assert_eq!(text.spans[0].text, "Hi");
    assert_eq!(text.spans[0].font_size, Some(18.0));
    assert_eq!(text.spans[0].color, Some(Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }));
    assert_eq!(exported_text.parent.unwrap().index, exported_box.id.index);
}
