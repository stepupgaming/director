//! Export a live [`Director`] into a [`director_scene::SceneDocument`].
//!
//! The document stores channels and kind-specific values. It does not store
//! Skia handles, layout results, or audio-smoothing state.

use crate::animation::{Animated, EasingType};
use crate::director::Director;
use crate::node::{BoxNode, TextNode};
use crate::scene::SceneNode;
use crate::types::{Color, NodeId};
use director_scene::{
    Channel, Keyframe, NodeBody, NodeRecord, NodeRef, Rgba, SceneDocument, TransformChannels,
    VERSION,
};

/// Copy the occupied scene graph into a scene document.
pub fn export_scene(director: &Director) -> SceneDocument {
    let mut nodes = Vec::new();
    for index in 0..director.scene.nodes.len() {
        let Some(node) = director.scene.nodes[index].as_ref() else {
            continue;
        };
        let id = NodeId::new(index as u32, director.scene.generation_at(index));
        nodes.push(export_node(id, node));
    }
    SceneDocument {
        version: VERSION,
        width: director.width,
        height: director.height,
        fps: director.fps,
        nodes,
    }
}

fn export_node(id: NodeId, node: &SceneNode) -> NodeRecord {
    let (opacity, body) = body_of(node);
    NodeRecord {
        id: node_ref(id),
        parent: node.parent.map(node_ref),
        children: node.children.iter().copied().map(node_ref).collect(),
        kind: node.element.kind().to_string(),
        z_index: node.z_index,
        blend_mode: format!("{:?}", node.blend_mode),
        is_static: node.is_static,
        transform: TransformChannels {
            translate_x: channel(&node.transform.translate_x),
            translate_y: channel(&node.transform.translate_y),
            scale_x: channel(&node.transform.scale_x),
            scale_y: channel(&node.transform.scale_y),
            rotation: channel(&node.transform.rotation),
            skew_x: channel(&node.transform.skew_x),
            skew_y: channel(&node.transform.skew_y),
            pivot_x: node.transform.pivot_x,
            pivot_y: node.transform.pivot_y,
        },
        opacity,
        body,
    }
}

fn body_of(node: &SceneNode) -> (Channel, NodeBody) {
    if let Some(box_node) = node.element.as_any().downcast_ref::<BoxNode>() {
        return (
            channel(&box_node.opacity),
            NodeBody::Box {
                background: box_node.bg_color.as_ref().map(|color| rgba(color.current_value)),
                width: length_px(box_node.style.size.width),
                height: length_px(box_node.style.size.height),
            },
        );
    }
    if let Some(text) = node.element.as_any().downcast_ref::<TextNode>() {
        let content = text
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>();
        let color = text
            .spans
            .iter()
            .find_map(|span| span.color)
            .unwrap_or(text.default_color.current_value);
        let font_size = text
            .spans
            .iter()
            .find_map(|span| span.font_size)
            .unwrap_or(text.default_font_size.current_value);
        return (
            channel(&text.opacity),
            NodeBody::Text {
                content,
                font_size,
                color: rgba(color),
            },
        );
    }
    (Channel::still(1.0), NodeBody::Unspecified)
}

fn channel(animated: &Animated<f32>) -> Channel {
    if animated.raw_keyframes.is_empty() {
        return Channel::still(animated.current_value);
    }
    Channel {
        keyframes: animated
            .raw_keyframes
            .iter()
            .map(|(value, time, easing)| Keyframe {
                time: *time,
                value: *value,
                easing: easing_name(easing),
            })
            .collect(),
    }
}

fn easing_name(easing: &EasingType) -> String {
    match easing {
        EasingType::Linear => "linear".to_string(),
        EasingType::EaseIn => "ease_in".to_string(),
        EasingType::EaseOut => "ease_out".to_string(),
        EasingType::EaseInOut => "ease_in_out".to_string(),
        EasingType::BounceOut => "bounce_out".to_string(),
        EasingType::BounceIn => "bounce_in".to_string(),
        EasingType::BounceInOut => "bounce_in_out".to_string(),
        EasingType::ElasticOut => "elastic_out".to_string(),
        EasingType::ElasticIn => "elastic_in".to_string(),
        EasingType::ElasticInOut => "elastic_in_out".to_string(),
        EasingType::BackOut => "back_out".to_string(),
        EasingType::BackIn => "back_in".to_string(),
        EasingType::BackInOut => "back_in_out".to_string(),
        EasingType::Hold => "hold".to_string(),
        EasingType::CubicBezier { x1, y1, x2, y2 } => {
            format!("cubic_bezier({x1},{y1},{x2},{y2})")
        }
        EasingType::Expression { expr } => format!("expr({expr})"),
    }
}

fn rgba(color: Color) -> Rgba {
    Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

fn node_ref(id: NodeId) -> NodeRef {
    NodeRef {
        index: id.index,
        generation: id.generation,
    }
}

fn length_px(size: taffy::style::Dimension) -> Option<f32> {
    size.into_option()
}
