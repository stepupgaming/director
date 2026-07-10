//! Layout-time render manifest — node tree, timeline, and per-node bounds for agents.
//!
//! Bounds are sampled at a single timeline instant (default `0.0`). Animated layout
//! may differ at other times; pass `--time` on the CLI to resample.

use crate::director::{Director, TimelineItem};
use crate::node::{
    box_node::BoxNode, caption::CaptionNode, composition::CompositionNode, effect::EffectNode,
    image_node::ImageNode, lottie::LottieNode, text::TextNode, vector::VectorNode,
    video_node::VideoNode,
};
#[cfg(feature = "wgpu")]
use crate::node::WgpuNode;
use crate::systems::layout::LayoutEngine;
use crate::Element;
use serde::Serialize;

pub const RENDER_MANIFEST_VERSION: u32 = 1;

#[derive(Serialize, Debug, Clone)]
pub struct RenderManifest {
    pub version: u32,
    pub width: i32,
    pub height: i32,
    /// Frames per second. Stored as `f64` so non-integer rates (23.976, 29.97)
    /// round-trip without truncating.
    pub fps: f64,
    pub sample_time: f64,
    pub duration_secs: f64,
    pub timeline: Vec<TimelineManifestEntry>,
    pub nodes: Vec<RenderNodeEntry>,
}

#[derive(Serialize, Debug, Clone)]
pub struct TimelineManifestEntry {
    pub scene_root: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub start_time: f64,
    pub duration: f64,
    pub z_index: i32,
}

#[derive(Serialize, Debug, Clone)]
pub struct RenderNodeEntry {
    pub id: usize,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<usize>,
    pub depth: u32,
    pub child_count: usize,
    pub z_index: i32,
    pub bounds: BoundsRect,
}

#[derive(Serialize, Debug, Clone, Copy, Default)]
pub struct BoundsRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Build a render manifest after layout at `sample_time`.
pub fn build_render_manifest(director: &mut Director, sample_time: f64) -> RenderManifest {
    director.update(sample_time);
    let mut layout_engine = LayoutEngine::new();
    layout_engine.compute_layout(
        &mut director.scene,
        director.width,
        director.height,
        sample_time,
    );

    let mut nodes = Vec::new();
    for (id, slot) in director.scene.nodes.iter().enumerate() {
        let Some(node) = slot else { continue };
        let depth = parent_depth(&director.scene, id);
        let r = node.layout_rect;
        nodes.push(RenderNodeEntry {
            id,
            kind: element_kind(node.element.as_ref()),
            parent_id: node.parent,
            depth,
            child_count: node.children.len(),
            z_index: node.z_index,
            bounds: BoundsRect {
                x: r.left(),
                y: r.top(),
                w: r.width(),
                h: r.height(),
            },
        });
    }

    let timeline: Vec<TimelineManifestEntry> = collect_timeline(director)
        .into_iter()
        .map(|t| TimelineManifestEntry {
            scene_root: t.scene_root,
            name: t.name,
            start_time: t.start_time,
            duration: t.duration,
            z_index: t.z_index,
        })
        .collect();

    RenderManifest {
        version: RENDER_MANIFEST_VERSION,
        width: director.width,
        height: director.height,
        fps: director.fps,
        sample_time,
        duration_secs: total_duration(director),
        timeline,
        nodes,
    }
}

pub fn render_manifest_json(director: &mut Director, sample_time: f64) -> String {
    serde_json::to_string_pretty(&build_render_manifest(director, sample_time))
        .expect("RenderManifest must serialize")
}

fn collect_timeline(director: &Director) -> Vec<TimelineItem> {
    let mut items = director.timeline.clone();
    for track in &director.tracks {
        for clip in &track.clips {
            items.push(TimelineItem {
                scene_root: clip.scene_root,
                name: clip.name.clone(),
                start_time: clip.start_time,
                duration: clip.duration,
                z_index: clip.z_index,
                audio_tracks: clip.audio_tracks.clone(),
                freeze_time: clip.freeze_time,
            });
        }
    }
    items
}

fn total_duration(director: &Director) -> f64 {
    collect_timeline(director)
        .into_iter()
        .map(|t| t.start_time + t.duration)
        .fold(0.0_f64, f64::max)
}

fn parent_depth(scene: &crate::scene::SceneGraph, id: usize) -> u32 {
    let mut depth = 0u32;
    let mut current = scene.get_node(id).and_then(|n| n.parent);
    while let Some(pid) = current {
        depth += 1;
        current = scene.get_node(pid).and_then(|n| n.parent);
    }
    depth
}

#[cfg(feature = "wgpu")]
fn wgpu_kind(any: &dyn std::any::Any) -> bool {
    any.is::<WgpuNode>()
}

#[cfg(not(feature = "wgpu"))]
fn wgpu_kind(_any: &dyn std::any::Any) -> bool {
    false
}

fn element_kind(element: &dyn Element) -> &'static str {
    let any = element.as_any();
    if any.is::<BoxNode>() {
        "box"
    } else if any.is::<TextNode>() {
        "text"
    } else if any.is::<ImageNode>() {
        "image"
    } else if any.is::<VideoNode>() {
        "video"
    } else if any.is::<VectorNode>() {
        "vector"
    } else if any.is::<LottieNode>() {
        "lottie"
    } else if any.is::<EffectNode>() {
        "effect"
    } else if any.is::<CompositionNode>() {
        "composition"
    } else if any.is::<CaptionNode>() {
        "caption"
    } else if wgpu_kind(any) {
        "wgpu"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::BoxNode;
    use crate::video_wrapper::RenderMode;
    use crate::{DefaultAssetLoader, Director};
    use std::sync::Arc;

    #[test]
    fn render_manifest_includes_bounds() {
        let mut director = Director::new(
            640,
            360,
            30.0,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );
        let root = director.scene.add_node(Box::new(BoxNode::new()));
        director.timeline.push(TimelineItem {
            scene_root: root,
            name: Some("main".into()),
            start_time: 0.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![],
            freeze_time: None,
        });

        let manifest = build_render_manifest(&mut director, 0.0);
        assert_eq!(manifest.nodes.len(), 1);
        assert_eq!(manifest.nodes[0].kind, "box");
        assert_eq!(manifest.timeline.len(), 1);
        assert_eq!(manifest.duration_secs, 2.0);
    }
}