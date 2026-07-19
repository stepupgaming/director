//! Static project manifest derived from a [`MovieRequest`] — agent-facing
//! summary of timeline, nodes, and asset references (no render required).
//!
//! Per-node bounding boxes require a layout pass and are a follow-up on top of
//! this static manifest.

use director_core::api_catalog::API_MANIFEST_VERSION;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::{EffectConfig, MovieRequest, Node, NodeKind};

/// Manifest version; bump when shape or semantics change.
pub const PROJECT_MANIFEST_VERSION: u32 = 1;

/// Agent-facing summary of a declarative `MovieRequest` project.
#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct ProjectManifest {
    pub version: u32,
    pub api_manifest_version: u32,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// Total timeline duration in seconds (scenes, loops, holds).
    pub duration_secs: f64,
    pub scene_count: usize,
    pub node_count: usize,
    pub scenes: Vec<SceneManifestEntry>,
    pub nodes: Vec<NodeManifestEntry>,
    pub assets: Vec<AssetRef>,
    pub audio_tracks: Vec<AudioTrackRef>,
}

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct SceneManifestEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub start_secs: f64,
    pub duration_secs: f64,
    pub z_index: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_hold_secs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<String>,
    pub root_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct NodeManifestEntry {
    pub id: String,
    /// `NodeKind` serde tag (`box`, `text`, `image`, …).
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub depth: u32,
    pub child_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct AssetRef {
    pub path: String,
    /// `image`, `video`, `audio`, `lottie`, `vector`, `caption`, `font`
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub referenced_by: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, JsonSchema)]
pub struct AudioTrackRef {
    pub id: String,
    pub src: String,
    pub start_time: f64,
}

/// Build a static manifest from a `MovieRequest`.
pub fn build_project_manifest(request: &MovieRequest) -> ProjectManifest {
    let mut nodes = Vec::new();

    for scene in &request.scenes {
        walk_node(&scene.root, None, 0, &mut nodes);
    }

    let mut scenes = Vec::new();
    let mut cumulative = 0.0_f64;
    for scene in &request.scenes {
        let plays = scene.loop_count.unwrap_or(1).max(1) as f64;
        let hold = scene
            .post_hold
            .as_ref()
            .map(|h| h.duration)
            .unwrap_or(0.0);
        let loop_span = if plays > 1.0 {
            scene.duration_secs + plays * scene.duration_secs
        } else {
            scene.duration_secs
        };
        let span = loop_span + hold;

        scenes.push(SceneManifestEntry {
            id: scene.id.clone(),
            name: scene.name.clone(),
            start_secs: cumulative,
            duration_secs: scene.duration_secs,
            z_index: scene.z_index,
            loop_count: scene.loop_count,
            post_hold_secs: scene.post_hold.as_ref().map(|h| h.duration),
            transition: scene
                .transition
                .as_ref()
                .map(|t| transition_type_tag(&t.kind)),
            root_id: scene.root.id.clone(),
        });

        cumulative += span;
    }

    let audio_tracks: Vec<AudioTrackRef> = request
        .audio_tracks
        .iter()
        .map(|t| AudioTrackRef {
            id: t.id.clone(),
            src: t.src.clone(),
            start_time: t.start_time,
        })
        .collect();

    let assets = collect_assets(request, &nodes);

    ProjectManifest {
        version: PROJECT_MANIFEST_VERSION,
        api_manifest_version: API_MANIFEST_VERSION,
        width: request.width,
        height: request.height,
        fps: request.fps,
        duration_secs: cumulative,
        scene_count: request.scenes.len(),
        node_count: nodes.len(),
        scenes,
        nodes,
        assets,
        audio_tracks,
    }
}

/// JSON manifest for agents and tooling.
pub fn export_project_manifest_json(request: &MovieRequest) -> String {
    serde_json::to_string_pretty(&build_project_manifest(request))
        .expect("ProjectManifest must serialize")
}

fn walk_node(
    node: &Node,
    parent_id: Option<&str>,
    depth: u32,
    out: &mut Vec<NodeManifestEntry>,
) {
    let (kind, assets, effect) = describe_kind(&node.kind);

    if let NodeKind::Composition { scenes, .. } = &node.kind {
        for scene in scenes {
            walk_node(&scene.root, Some(&node.id), depth + 1, out);
        }
    }

    let child_count = node.children.len();
    out.push(NodeManifestEntry {
        id: node.id.clone(),
        kind,
        parent_id: parent_id.map(str::to_string),
        depth,
        child_count,
        assets,
        effect,
    });

    for child in &node.children {
        walk_node(child, Some(&node.id), depth + 1, out);
    }
}

fn collect_assets(request: &MovieRequest, nodes: &[NodeManifestEntry]) -> Vec<AssetRef> {
    let mut idx: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
    for node in nodes {
        for path in &node.assets {
            let kind = asset_kind_for_path(path);
            record_asset(&mut idx, path.clone(), kind, node.id.clone());
        }
    }
    if let Some(font) = &request.default_font {
        record_asset(&mut idx, font.clone(), "font", "movie".to_string());
    }
    for track in &request.audio_tracks {
        record_asset(
            &mut idx,
            track.src.clone(),
            "audio",
            format!("audio:{}", track.id),
        );
    }
    idx.into_iter()
        .map(|(path, (kind, refs))| AssetRef {
            path,
            kind,
            referenced_by: refs.into_iter().collect(),
        })
        .collect()
}

fn describe_kind(kind: &NodeKind) -> (String, Vec<String>, Option<String>) {
    match kind {
        NodeKind::Box { .. } => ("box".into(), vec![], None),
        NodeKind::Text { .. } => ("text".into(), vec![], None),
        NodeKind::Image { src, .. } => ("image".into(), vec![src.clone()], None),
        NodeKind::Video { src, .. } => ("video".into(), vec![src.clone()], None),
        NodeKind::Vector { src } => ("vector".into(), vec![src.clone()], None),
        NodeKind::Lottie { src, .. } => ("lottie".into(), vec![src.clone()], None),
        NodeKind::Effect { effect_type } => (
            "effect".into(),
            vec![],
            Some(effect_type_tag(effect_type)),
        ),
        NodeKind::Caption { source, .. } => ("caption".into(), vec![source.clone()], None),
        NodeKind::Composition { .. } => ("composition".into(), vec![], None),
    }
}

fn effect_type_tag(effect: &EffectConfig) -> String {
    let json = serde_json::to_value(effect).expect("EffectConfig serializes");
    json.get("effect")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string()
}

fn transition_type_tag(kind: &crate::TransitionType) -> String {
    let json = serde_json::to_value(kind).expect("TransitionType serializes");
    json.get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string()
}

fn asset_kind_for_path(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".mp4")
        || lower.ends_with(".webm")
        || lower.ends_with(".mov")
        || lower.ends_with(".mkv")
    {
        "video"
    } else if lower.ends_with(".mp3")
        || lower.ends_with(".wav")
        || lower.ends_with(".aac")
        || lower.ends_with(".m4a")
        || lower.ends_with(".ogg")
    {
        "audio"
    } else if lower.ends_with(".json") && lower.contains("lottie") {
        "lottie"
    } else if lower.ends_with(".json") {
        "lottie"
    } else if lower.ends_with(".svg") {
        "vector"
    } else if lower.ends_with(".srt") || lower.ends_with(".vtt") {
        "caption"
    } else if lower.ends_with(".ttf") || lower.ends_with(".otf") || lower.ends_with(".woff2") {
        "font"
    } else {
        "image"
    }
}

fn record_asset(
    index: &mut BTreeMap<String, (String, BTreeSet<String>)>,
    path: String,
    kind: &str,
    referenced_by: String,
) {
    index
        .entry(path)
        .and_modify(|(_, refs)| {
            refs.insert(referenced_by.clone());
        })
        .or_insert_with(|| (kind.to_string(), BTreeSet::from([referenced_by])));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeKind, Scene, StyleMap, TransformMap};

    #[test]
    fn project_manifest_collects_assets_and_timeline() {
        let request = MovieRequest {
            width: 1920,
            height: 1080,
            fps: 30.0,
            default_font: None,
            asset_search_paths: vec![],
            scenes: vec![Scene {
                id: "main".into(),
                name: Some("Main".into()),
                duration_secs: 5.0,
                z_index: 0,
                background: None,
                root: Node {
                    id: "root".into(),
                    kind: NodeKind::Box { border_radius: 0.0 },
                    style: StyleMap::default(),
                    transform: TransformMap::default(),
                    animations: vec![],
                    spring_animations: vec![],
                    audio_bindings: vec![],
                    children: vec![Node {
                        id: "img".into(),
                        kind: NodeKind::Image {
                            src: "hero.png".into(),
                            object_fit: None,
                        },
                        style: StyleMap::default(),
                        transform: TransformMap::default(),
                        animations: vec![],
                        spring_animations: vec![],
                        audio_bindings: vec![],
                        children: vec![],
                    }],
                },
                transition: None,
                loop_count: Some(2),
                post_hold: Some(crate::ScenePostHold {
                    freeze_at: None,
                    duration: 1.0,
                }),
            }],
            audio_tracks: vec![],
        };

        let manifest = build_project_manifest(&request);
        assert_eq!(manifest.node_count, 2);
        assert_eq!(manifest.scenes.len(), 1);
        assert_eq!(manifest.scenes[0].loop_count, Some(2));
        assert_eq!(manifest.scenes[0].post_hold_secs, Some(1.0));
        assert!(manifest.duration_secs > 5.0);
        assert!(manifest.assets.iter().any(|a| a.path == "hero.png"));
    }
}