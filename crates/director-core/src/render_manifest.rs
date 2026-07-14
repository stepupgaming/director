//! Layout-time render manifest — node tree, timeline, and per-node bounds for agents.
//!
//! Bounds are sampled at a single timeline instant (default `0.0`). Animated layout
//! may differ at other times; pass `--time` on the CLI to resample.
//!
//! # Verification
//!
//! `RenderManifest` can be verified with [`RenderManifest::verify`], which emits
//! machine-actionable [`VerificationIssue`]s such as caption-over-face, safe-zone
//! violations, and static stretches. Use [`RenderManifest::repair`] to recompile the
//! affected regions of the Director scene graph.

use crate::compiler::DependencyInput;
use crate::director::{Director, TimelineItem};
use crate::entity::{BoundingBox, EntityKind};
use crate::node::active_speaker_camera::ActiveSpeakerCamera;
use crate::node::smart_caption::{SmartCaptionConfig, SmartCaptionNode};
use crate::node::caption::scaled_safe_zone;
use crate::semantic::{CaptionTrajectory, SemanticManifest, SolverConfig, TrajectorySample};
use crate::element::element_kind;
use crate::systems::layout::LayoutEngine;
use crate::types::BoundsRect;
use crate::Element;
use serde::{Deserialize, Serialize};

pub const RENDER_MANIFEST_VERSION: u32 = 1;

#[derive(Serialize, Debug, Clone)]
pub struct RenderManifest {
    pub version: u32,
    pub width: i32,
    pub height: i32,
    pub fps: u32,
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

/// Severity level for a verification issue.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IssueSeverity {
    Error,
    Warning,
    Info,
}

impl IssueSeverity {
    /// Return the lowercase string representation of the severity.
    pub fn as_str(&self) -> &'static str {
        match self {
            IssueSeverity::Error => "error",
            IssueSeverity::Warning => "warning",
            IssueSeverity::Info => "info",
        }
    }
}

/// Type of verification issue detected in a render manifest.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationIssueType {
    /// A caption node overlaps a tracked face.
    CaptionOverFace,
    /// A node extends outside the platform safe zone.
    SafeZoneViolation,
    /// A region has low motion energy for too long.
    StaticStretch,
    /// A reframed camera crop would push the tracked subject out of frame.
    SubjectOutOfFrame,
}

/// A single machine-actionable verification issue.
#[derive(Serialize, Debug, Clone)]
pub struct VerificationIssue {
    pub issue_type: VerificationIssueType,
    pub severity: IssueSeverity,
    /// Start time of the affected region in seconds.
    pub start_time: f64,
    /// End time of the affected region in seconds.
    pub end_time: f64,
    /// IDs of scene nodes involved in the issue.
    pub node_ids: Vec<usize>,
    /// Human-readable description of the issue.
    pub description: String,
    /// Optional suggested fix (e.g., "move caption to top").
    pub suggested_fix: Option<String>,
    /// Bounding box of the problematic region.
    ///
    /// Semantics per issue type:
    /// - `CaptionOverFace`: bounds of the caption node that overlaps a face.
    /// - `SafeZoneViolation`: bounds of the node that extends outside the safe zone.
    /// - `StaticStretch`: full canvas, because the issue applies to the whole frame.
    pub region: BoundsRect,
}

/// Configuration for manifest verification.
#[derive(Serialize, Debug, Clone)]
pub struct VerificationConfig {
    /// Platform safe-zone preset to validate against.
    pub safe_zone_preset: String,
    /// Minimum motion-energy metric below which a stretch is considered static.
    /// The metric is the sum over all nodes of the Euclidean norm of the
    /// position/size delta between two sampled manifests (in px-equivalent units).
    pub static_stretch_threshold: f32,
    /// Duration in seconds over which to measure motion for static-stretch detection.
    pub static_stretch_window: f64,
    /// Minimum overlap area (in pixels) to report a caption-over-face issue.
    pub caption_face_overlap_threshold: f32,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            safe_zone_preset: "tiktok".to_string(),
            static_stretch_threshold: 5.0,
            static_stretch_window: 1.0,
            caption_face_overlap_threshold: 1.0,
        }
    }
}

/// Result of a repair pass.
#[derive(Serialize, Debug, Clone, Default)]
pub struct RepairResult {
    /// Node IDs that were marked dirty by the repair.
    pub affected_nodes: Vec<usize>,
    /// Time ranges that were recompiled.
    pub affected_regions: Vec<(f64, f64)>,
    /// Number of issues that were queued for repair.
    pub queued_issue_count: usize,
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

impl RenderManifest {
    /// Verify the manifest against a Director and emit machine-actionable issues.
    ///
    /// The verification pass checks:
    /// - `CaptionOverFace`: caption nodes that overlap tracked face entities.
    /// - `SafeZoneViolation`: nodes that extend outside the configured platform safe zone.
    /// - `StaticStretch`: regions where the total on-screen motion is below the threshold.
    pub fn verify(&self, director: &mut Director, config: &VerificationConfig) -> Vec<VerificationIssue> {
        let mut issues = Vec::new();
        self.detect_caption_over_face(director, config, &mut issues);
        self.detect_safe_zone_violations(director, config, &mut issues);
        self.detect_static_stretches(director, config, &mut issues);
        self.detect_subject_out_of_frame(director, config, &mut issues);
        issues
    }

    /// Repair the issues by recompiling affected regions of the Director scene graph.
    ///
    /// This calls `Director::compile()` and `Director::invalidate()` for the nodes
    /// referenced by the given issues. The caller can then re-render the affected
    /// regions.
    ///
    /// TODO: For `SmartCaptionNode`s, also re-run the semantic trajectory solver
    /// using the current entity tracks and replace the node's trajectory.
    pub fn repair(&self, director: &mut Director, issues: &[VerificationIssue]) -> RepairResult {
        // For caption-over-face issues involving SmartCaptionNodes, re-solve
        // the trajectory using the current entity tracks before invalidation.
        for issue in issues {
            if issue.issue_type != VerificationIssueType::CaptionOverFace {
                continue;
            }
            for &node_id in &issue.node_ids {
                if let Some(node) = director.scene.get_node_mut(node_id) {
                    if let Some(smart) = node
                        .element
                        .as_any_mut()
                        .downcast_mut::<SmartCaptionNode>()
                    {
                        smart.re_solve(&director.entities);
                    }
                }
            }
        }

        director.compile();
        let mut affected_nodes = std::collections::HashSet::new();
        let mut affected_regions = Vec::new();

        for issue in issues {
            for &node_id in &issue.node_ids {
                affected_nodes.insert(node_id);
            }
            affected_regions.push((issue.start_time, issue.end_time));
        }

        if !affected_nodes.is_empty() {
            // Use the dependency graph to also mark transitive dependents dirty.
            let inputs: Vec<DependencyInput> = affected_nodes
                .iter()
                .map(|&node_id| DependencyInput::Node(node_id))
                .collect();
            let transitive = director.invalidate(inputs);
            affected_nodes.extend(transitive);
            // Ensure the originally affected nodes are dirty even if the graph
            // did not record a self-dependency for them.
            director.dirty_nodes.extend(&affected_nodes);
        }

        RepairResult {
            affected_nodes: affected_nodes.into_iter().collect(),
            affected_regions,
            queued_issue_count: issues.len(),
        }
    }

    fn detect_caption_over_face(
        &self,
        director: &Director,
        config: &VerificationConfig,
        issues: &mut Vec<VerificationIssue>,
    ) {
        let face_ids = director.entities.find_by_kind(&EntityKind::Face);
        if face_ids.is_empty() {
            return;
        }

        for node in &self.nodes {
            if node.kind != "caption" && node.kind != "smart_caption" {
                continue;
            }

            let mut total_overlap = 0.0f32;
            let mut overlapping_faces = Vec::new();
            for &face_id in face_ids {
                let Some(sample) = director.entities.sample_at(face_id, self.sample_time) else {
                    continue;
                };
                let face_bounds = bbox_to_bounds_rect(&sample.bbox);
                let overlap = node.bounds.overlap_area(&face_bounds);
                if overlap >= config.caption_face_overlap_threshold {
                    total_overlap += overlap;
                    overlapping_faces.push(face_id);
                }
            }

            if !overlapping_faces.is_empty() {
                issues.push(VerificationIssue {
                    issue_type: VerificationIssueType::CaptionOverFace,
                    severity: IssueSeverity::Error,
                    start_time: self.sample_time,
                    end_time: self.sample_time,
                    node_ids: vec![node.id],
                    description: format!(
                        "Caption node {} overlaps {} face(s) by {:.1} px² at t={:.2}s",
                        node.id, overlapping_faces.len(), total_overlap, self.sample_time
                    ),
                    suggested_fix: Some("Move caption to avoid face; consider using SmartCaptionNode".to_string()),
                    region: node.bounds,
                });
            }
        }
    }

    fn detect_safe_zone_violations(
        &self,
        director: &Director,
        config: &VerificationConfig,
        issues: &mut Vec<VerificationIssue>,
    ) {
        let Some(zone) = scaled_safe_zone(&config.safe_zone_preset, director.height as f32) else {
            tracing::warn!(
                "Unknown safe-zone preset '{}'; skipping safe-zone verification.",
                config.safe_zone_preset
            );
            return;
        };

        let canvas_w = director.width as f32;
        let canvas_h = director.height as f32;

        for node in &self.nodes {
            let left = node.bounds.x;
            let top = node.bounds.y;
            let right = node.bounds.x + node.bounds.w;
            let bottom = node.bounds.y + node.bounds.h;

            let mut violated_edges = Vec::new();
            let mut max_amount = 0.0f32;

            if left < zone.left {
                let amount = zone.left - left;
                violated_edges.push(("left", amount));
                max_amount = max_amount.max(amount);
            }
            if top < zone.top {
                let amount = zone.top - top;
                violated_edges.push(("top", amount));
                max_amount = max_amount.max(amount);
            }
            if right > canvas_w - zone.right {
                let amount = right - (canvas_w - zone.right);
                violated_edges.push(("right", amount));
                max_amount = max_amount.max(amount);
            }
            if bottom > canvas_h - zone.bottom {
                let amount = bottom - (canvas_h - zone.bottom);
                violated_edges.push(("bottom", amount));
                max_amount = max_amount.max(amount);
            }

            if !violated_edges.is_empty() {
                let edge_names: Vec<_> = violated_edges.iter().map(|(e, _)| *e).collect();
                issues.push(VerificationIssue {
                    issue_type: VerificationIssueType::SafeZoneViolation,
                    severity: IssueSeverity::Warning,
                    start_time: self.sample_time,
                    end_time: self.sample_time,
                    node_ids: vec![node.id],
                    description: format!(
                        "Node {} extends {:.1}px outside {} safe-zone edge(s) at t={:.2}s",
                        node.id,
                        max_amount,
                        edge_names.join(", "),
                        self.sample_time
                    ),
                    suggested_fix: Some(format!(
                        "Reposition node away from {} safe-zone edge(s)",
                        edge_names.join(", ")
                    )),
                    region: node.bounds,
                });
            }
        }
    }

    fn detect_static_stretches(
        &self,
        director: &mut Director,
        config: &VerificationConfig,
        issues: &mut Vec<VerificationIssue>,
    ) {
        let start = self.sample_time;
        let end = (start + config.static_stretch_window).min(self.duration_secs);
        if end <= start {
            return;
        }

        // Sample the layout at the end of the window without cloning the
        // whole Director. `self` already represents the layout at `start`.
        let snapshot_end = director.layout_snapshot(end);

        let motion = total_motion_energy(self, &snapshot_end);
        if motion < config.static_stretch_threshold {
            issues.push(VerificationIssue {
                issue_type: VerificationIssueType::StaticStretch,
                severity: IssueSeverity::Info,
                start_time: start,
                end_time: end,
                node_ids: self.nodes.iter().map(|n| n.id).collect(),
                description: format!(
                    "Static stretch detected from t={:.2}s to t={:.2}s (motion energy: {:.1}px)",
                    start, end, motion
                ),
                suggested_fix: Some("Add motion, cut, or B-roll to break the static stretch".to_string()),
                region: BoundsRect {
                    x: 0.0,
                    y: 0.0,
                    w: director.width as f32,
                    h: director.height as f32,
                },
            });
        }
    }

    fn detect_subject_out_of_frame(
        &self,
        director: &Director,
        _config: &VerificationConfig,
        issues: &mut Vec<VerificationIssue>,
    ) {
        for (id, slot) in director.scene.nodes.iter().enumerate() {
            let Some(node) = slot else { continue };
            let Some(camera) = node.element.as_any().downcast_ref::<ActiveSpeakerCamera>() else {
                continue;
            };
            let Some(node_entry) = self.nodes.iter().find(|n| n.id == id) else { continue };
            let output_rect = skia_safe::Rect::from_wh(
                node_entry.bounds.w,
                node_entry.bounds.h,
            );
            let Some(region) = camera.subject_out_of_frame_at(
                self.sample_time,
                &director.entities,
                output_rect,
            ) else {
                continue;
            };
            issues.push(VerificationIssue {
                issue_type: VerificationIssueType::SubjectOutOfFrame,
                severity: IssueSeverity::Warning,
                start_time: self.sample_time,
                end_time: self.sample_time,
                node_ids: vec![id],
                description: format!(
                    "ActiveSpeakerCamera node {} reframed crop pushes subject out of frame at t={:.2}s",
                    id, self.sample_time
                ),
                suggested_fix: Some("Reduce zoom multiplier, move subject away from frame edges, or add padding".to_string()),
                region,
            });
        }
    }
}

fn bbox_to_bounds_rect(bbox: &BoundingBox) -> BoundsRect {
    BoundsRect {
        x: bbox.x,
        y: bbox.y,
        w: bbox.width,
        h: bbox.height,
    }
}

fn total_motion_energy(a: &RenderManifest, b: &crate::director::LayoutSnapshot) -> f32 {
    let mut total = 0.0f32;
    for node_a in &a.nodes {
        let Some(node_b) = b.nodes.iter().find(|n| n.id == node_a.id) else {
            continue;
        };
        let dx = node_b.bounds.x - node_a.bounds.x;
        let dy = node_b.bounds.y - node_a.bounds.y;
        let dw = node_b.bounds.w - node_a.bounds.w;
        let dh = node_b.bounds.h - node_a.bounds.h;
        total += (dx * dx + dy * dy + dw * dw + dh * dh).sqrt();
    }
    total
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



#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{BoundingBox, EntityKind};
    use crate::node::caption::{CaptionLine, CaptionNode, CaptionWord};
    use crate::node::BoxNode;
    use crate::video_wrapper::RenderMode;
    use crate::{DefaultAssetLoader, Director};
    use std::sync::{Arc, Mutex};

    fn empty_font_collection() -> Arc<Mutex<skia_safe::textlayout::FontCollection>> {
        Arc::new(Mutex::new(skia_safe::textlayout::FontCollection::new()))
    }

    fn single_word_line(text: &str, start: f64, end: f64) -> CaptionLine {
        CaptionLine {
            text: text.to_string(),
            words: vec![CaptionWord {
                text: text.to_string(),
                start_sec: start,
                end_sec: end,
                display_line_idx: 0,
            }],
            start_sec: start,
            end_sec: end,
        }
    }

    #[test]
    fn render_manifest_includes_bounds() {
        let mut director = Director::new(
            640,
            360,
            30,
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
        });

        let manifest = build_render_manifest(&mut director, 0.0);
        assert_eq!(manifest.nodes.len(), 1);
        assert_eq!(manifest.nodes[0].kind, "box");
        assert_eq!(manifest.timeline.len(), 1);
        assert_eq!(manifest.duration_secs, 2.0);
    }

    #[test]
    fn render_manifest_reports_active_speaker_camera_kind() {
        use crate::node::active_speaker_camera::{
            ActiveSpeakerCamera, ActiveSpeakerCameraConfig, ContinuityGroup, Framing,
        };
        use crate::video_wrapper::RenderMode;

        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        let speaker = director.entities.create(EntityKind::Speaker, "mara".to_string());
        director.entities.add_sample(
            speaker,
            0.0,
            BoundingBox::new(500.0, 500.0, 200.0, 200.0),
            1.0,
        );

        let camera = ActiveSpeakerCamera::from_bytes(
            vec![],
            RenderMode::Preview,
            &director.entities,
            vec![ContinuityGroup::new(0.0, 2.0, speaker, Framing::Medium)],
            ActiveSpeakerCameraConfig::default(),
        );
        let camera_id = director.scene.add_node(Box::new(camera));
        director.timeline.push(TimelineItem {
            scene_root: camera_id,
            name: Some("active_speaker_camera".into()),
            start_time: 0.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let manifest = build_render_manifest(&mut director, 0.5);
        assert_eq!(manifest.nodes.len(), 1, "manifest should contain exactly the camera node");
        let camera_entry = manifest
            .nodes
            .iter()
            .find(|n| n.id == camera_id)
            .expect("manifest should contain the camera node");
        assert_eq!(camera_entry.kind, "active_speaker_camera");
    }

    #[test]
    fn verify_detects_caption_over_face() {
        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        // Place a face in the top-center of the frame where the caption
        // block is laid out (the caption node's bounds include its margin).
        let face = director.entities.create(EntityKind::Face, "mara".to_string());
        director.entities.add_sample(
            face,
            0.0,
            BoundingBox::new(400.0, 0.0, 200.0, 100.0),
            1.0,
        );

        // Place a caption node that overlaps the face.
        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let caption = CaptionNode::new(lines, empty_font_collection());
        let caption_id = director.scene.add_node(Box::new(caption));
        director.timeline.push(TimelineItem {
            scene_root: caption_id,
            name: Some("caption".into()),
            start_time: 0.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let manifest = build_render_manifest(&mut director, 0.5);
        let config = VerificationConfig::default();
        let issues = manifest.verify(&mut director, &config);

        let caption_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.issue_type == VerificationIssueType::CaptionOverFace)
            .collect();
        assert!(
            !caption_issues.is_empty(),
            "should detect caption overlapping face"
        );
        assert_eq!(caption_issues[0].node_ids, vec![caption_id]);
        assert_eq!(caption_issues[0].severity, IssueSeverity::Error);
    }

    #[test]
    fn verify_detects_smart_caption_over_face() {
        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        // Place a face in the top-center of the frame.
        let face = director.entities.create(EntityKind::Face, "mara".to_string());
        director.entities.add_sample(
            face,
            0.0,
            BoundingBox::new(400.0, 20.0, 200.0, 200.0),
            1.0,
        );

        // Build a SmartCaptionNode and force its trajectory to overlap the face.
        let manifest = SemanticManifest {
            entity_refs: vec![face],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };
        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let inner = CaptionNode::new(lines, empty_font_collection());
        let mut smart = SmartCaptionNode::from_manifest(
            inner,
            &director.entities,
            &manifest,
            SmartCaptionConfig::default(),
        );
        smart.trajectory = CaptionTrajectory::new(vec![
            TrajectorySample { time: 0.0, x: 540.0, y: 160.0 },
            TrajectorySample { time: 2.0, x: 540.0, y: 160.0 },
        ]);

        let smart_id = director.scene.add_node(Box::new(smart));
        director.timeline.push(TimelineItem {
            scene_root: smart_id,
            name: Some("smart_caption".into()),
            start_time: 0.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let manifest = build_render_manifest(&mut director, 0.5);
        let config = VerificationConfig::default();
        let issues = manifest.verify(&mut director, &config);

        let caption_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.issue_type == VerificationIssueType::CaptionOverFace)
            .collect();
        assert!(
            !caption_issues.is_empty(),
            "should detect smart caption overlapping face"
        );
        assert_eq!(caption_issues[0].node_ids, vec![smart_id]);
        assert_eq!(caption_issues[0].severity, IssueSeverity::Error);
        assert!(caption_issues[0].description.contains(&format!("Caption node {}", smart_id)));
    }

    #[test]
    fn verify_detects_safe_zone_violation() {
        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        // Place a box that extends into the TikTok bottom safe zone.
        let mut box_node = BoxNode::new();
        box_node.style.size.width = taffy::style::Dimension::length(1080.0);
        box_node.style.size.height = taffy::style::Dimension::length(200.0);
        let root = director.scene.add_node(Box::new(box_node));
        director.timeline.push(TimelineItem {
            scene_root: root,
            name: Some("box".into()),
            start_time: 0.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let manifest = build_render_manifest(&mut director, 0.0);
        let config = VerificationConfig::default();
        let issues = manifest.verify(&mut director, &config);

        let safe_zone_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.issue_type == VerificationIssueType::SafeZoneViolation)
            .collect();
        assert!(
            !safe_zone_issues.is_empty(),
            "should detect safe-zone violation"
        );
        assert_eq!(safe_zone_issues[0].severity, IssueSeverity::Warning);
    }

    #[test]
    fn verify_detects_static_stretch() {
        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        // Static box with no animation.
        let mut box_node = BoxNode::new();
        box_node.style.size.width = taffy::style::Dimension::length(100.0);
        box_node.style.size.height = taffy::style::Dimension::length(100.0);
        let root = director.scene.add_node(Box::new(box_node));
        director.timeline.push(TimelineItem {
            scene_root: root,
            name: Some("static".into()),
            start_time: 0.0,
            duration: 5.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let manifest = build_render_manifest(&mut director, 0.0);
        let config = VerificationConfig::default();
        let issues = manifest.verify(&mut director, &config);

        let static_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.issue_type == VerificationIssueType::StaticStretch)
            .collect();
        assert!(!static_issues.is_empty(), "should detect static stretch");
        assert_eq!(static_issues[0].severity, IssueSeverity::Info);
    }

    #[test]
    fn verify_groups_safe_zone_edges_per_node() {
        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        // Box that extends beyond both left and bottom safe zones.
        let mut box_node = BoxNode::new();
        box_node.style.size.width = taffy::style::Dimension::length(1080.0);
        box_node.style.size.height = taffy::style::Dimension::length(200.0);
        let root = director.scene.add_node(Box::new(box_node));
        director.timeline.push(TimelineItem {
            scene_root: root,
            name: Some("box".into()),
            start_time: 0.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let manifest = build_render_manifest(&mut director, 0.0);
        let config = VerificationConfig::default();
        let issues = manifest.verify(&mut director, &config);

        let safe_zone_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.issue_type == VerificationIssueType::SafeZoneViolation)
            .collect();
        assert_eq!(safe_zone_issues.len(), 1, "should emit one issue per node");
        let desc = &safe_zone_issues[0].description;
        assert!(desc.contains("left") || desc.contains("bottom"));
    }

    #[test]
    fn verify_groups_multiple_faces_per_caption() {
        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        // Two faces inside the caption block area.
        let face_a = director.entities.create(EntityKind::Face, "mara".to_string());
        director.entities.add_sample(
            face_a,
            0.0,
            BoundingBox::new(100.0, 0.0, 150.0, 80.0),
            1.0,
        );
        let face_b = director.entities.create(EntityKind::Face, "alex".to_string());
        director.entities.add_sample(
            face_b,
            0.0,
            BoundingBox::new(700.0, 0.0, 150.0, 80.0),
            1.0,
        );

        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let caption = CaptionNode::new(lines, empty_font_collection());
        let caption_id = director.scene.add_node(Box::new(caption));
        director.timeline.push(TimelineItem {
            scene_root: caption_id,
            name: Some("caption".into()),
            start_time: 0.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let manifest = build_render_manifest(&mut director, 0.5);
        let config = VerificationConfig::default();
        let issues = manifest.verify(&mut director, &config);

        let caption_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.issue_type == VerificationIssueType::CaptionOverFace)
            .collect();
        assert_eq!(caption_issues.len(), 1, "should emit one issue per caption node");
        assert!(caption_issues[0].description.contains("2 face(s)"));
    }

    #[test]
    fn repair_resolves_smart_caption_trajectory() {
        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        // Face at the top-center of the frame. Position it high enough that
        // the top-anchored smart caption's small (empty-font) layout rect
        // actually overlaps it.
        let face = director.entities.create(EntityKind::Face, "mara".to_string());
        director.entities.add_sample(
            face,
            0.0,
            BoundingBox::new(400.0, 20.0, 200.0, 200.0),
            1.0,
        );

        let manifest = SemanticManifest {
            entity_refs: vec![face],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };
        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let inner = CaptionNode::new(lines, empty_font_collection());
        let config = SmartCaptionConfig {
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            safe_top: 60.0,
            safe_bottom: 110.0,
            safe_left: 60.0,
            safe_right: 60.0,
            sample_rate: 10.0,
            continuous: false,
        };
        let mut smart = SmartCaptionNode::from_manifest(inner, &director.entities, &manifest, config);
        // Force the trajectory to the top so it overlaps the face.
        smart.trajectory = CaptionTrajectory::new(vec![
            TrajectorySample { time: 0.0, x: 540.0, y: 160.0 },
            TrajectorySample { time: 2.0, x: 540.0, y: 160.0 },
        ]);

        let smart_id = director.scene.add_node(Box::new(smart));
        director.timeline.push(TimelineItem {
            scene_root: smart_id,
            name: Some("smart_caption".into()),
            start_time: 0.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let manifest = build_render_manifest(&mut director, 0.5);
        let config = VerificationConfig::default();
        let issues = manifest.verify(&mut director, &config);

        let caption_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.issue_type == VerificationIssueType::CaptionOverFace)
            .collect();
        assert!(
            !caption_issues.is_empty(),
            "should detect smart caption overlapping face"
        );

        // Before repair, the trajectory is at the top.
        let smart = director
            .scene
            .get_node(smart_id)
            .unwrap()
            .element
            .as_any()
            .downcast_ref::<SmartCaptionNode>()
            .unwrap();
        let (_x, y) = smart.trajectory.sample_at(0.5).unwrap();
        assert!(y < 960.0, "pre-repair trajectory should be at top, got y={}", y);

        // Repair should re-solve the trajectory to avoid the face.
        let _result = manifest.repair(&mut director, &issues);

        let smart = director
            .scene
            .get_node(smart_id)
            .unwrap()
            .element
            .as_any()
            .downcast_ref::<SmartCaptionNode>()
            .unwrap();
        let (_x, y) = smart.trajectory.sample_at(0.5).unwrap();
        assert!(
            y > 960.0,
            "post-repair trajectory should avoid top face, got y={}",
            y
        );
    }

    #[test]
    fn verify_detects_subject_out_of_frame() {
        use crate::node::active_speaker_camera::{
            ActiveSpeakerCamera, ActiveSpeakerCameraConfig, ContinuityGroup, Framing,
        };
        use crate::video_wrapper::RenderMode;

        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        // A wide subject near the center. The vertical output aspect ratio
        // (9:16) makes the reframed crop much narrower than the subject, so
        // the subject's sides are pushed out of frame even with a normal
        // close-up framing.
        let speaker = director.entities.create(EntityKind::Speaker, "mara".to_string());
        director.entities.add_sample(
            speaker,
            0.0,
            BoundingBox::new(340.0, 500.0, 400.0, 100.0),
            1.0,
        );

        let camera = ActiveSpeakerCamera::from_bytes(
            vec![],
            RenderMode::Preview,
            &director.entities,
            vec![ContinuityGroup::new(0.0, 2.0, speaker, Framing::CloseUp)],
            ActiveSpeakerCameraConfig {
                canvas_w: 1080.0,
                canvas_h: 1920.0,
                sample_rate: 10.0,
                smoothing_window: 1,
                center_offset_y: 0.0,
            },
        );
        // Set the (decoded) video dimensions explicitly for verification.
        let mut camera = camera;
        camera.set_video_size(1000.0, 1000.0);
        // Force the camera node to fill the vertical canvas so the output
        // aspect ratio matches the intended 9:16 reframing.
        camera.inner_mut().style.size.width = taffy::style::Dimension::length(1080.0);
        camera.inner_mut().style.size.height = taffy::style::Dimension::length(1920.0);

        let camera_id = director.scene.add_node(Box::new(camera));
        director.timeline.push(TimelineItem {
            scene_root: camera_id,
            name: Some("active_speaker_camera".into()),
            start_time: 0.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let manifest = build_render_manifest(&mut director, 0.5);
        let config = VerificationConfig::default();
        let issues = manifest.verify(&mut director, &config);

        let subject_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.issue_type == VerificationIssueType::SubjectOutOfFrame)
            .collect();
        assert!(
            !subject_issues.is_empty(),
            "should detect subject pushed out of frame by reframed crop"
        );
        assert_eq!(subject_issues[0].severity, IssueSeverity::Warning);
        assert_eq!(subject_issues[0].node_ids, vec![camera_id]);
    }

    #[test]
    fn verify_no_subject_out_of_frame_when_subject_fits() {
        use crate::node::active_speaker_camera::{
            ActiveSpeakerCamera, ActiveSpeakerCameraConfig, ContinuityGroup, Framing,
        };
        use crate::video_wrapper::RenderMode;

        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        // Speaker in the center of a large video frame.
        let speaker = director.entities.create(EntityKind::Speaker, "mara".to_string());
        director.entities.add_sample(
            speaker,
            0.0,
            BoundingBox::new(500.0, 500.0, 200.0, 200.0),
            1.0,
        );

        // Wide framing keeps the subject well inside the crop.
        let camera = ActiveSpeakerCamera::from_bytes(
            vec![],
            RenderMode::Preview,
            &director.entities,
            vec![ContinuityGroup::new(0.0, 2.0, speaker, Framing::Wide)],
            ActiveSpeakerCameraConfig {
                canvas_w: 1080.0,
                canvas_h: 1920.0,
                sample_rate: 10.0,
                smoothing_window: 1,
                center_offset_y: 0.0,
            },
        );
        let mut camera = camera;
        camera.set_video_size(2000.0, 2000.0);

        let camera_id = director.scene.add_node(Box::new(camera));
        director.timeline.push(TimelineItem {
            scene_root: camera_id,
            name: Some("active_speaker_camera".into()),
            start_time: 0.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let manifest = build_render_manifest(&mut director, 0.5);
        let config = VerificationConfig::default();
        let issues = manifest.verify(&mut director, &config);

        let subject_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.issue_type == VerificationIssueType::SubjectOutOfFrame)
            .collect();
        assert!(
            subject_issues.is_empty(),
            "should not flag subject out of frame when subject fits inside crop"
        );
    }

    #[test]
    fn repair_marks_affected_nodes_dirty() {
        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        let face = director.entities.create(EntityKind::Face, "mara".to_string());
        director.entities.add_sample(
            face,
            0.0,
            BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
            1.0,
        );

        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let caption = CaptionNode::new(lines, empty_font_collection());
        let caption_id = director.scene.add_node(Box::new(caption));
        director.timeline.push(TimelineItem {
            scene_root: caption_id,
            name: Some("caption".into()),
            start_time: 0.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let manifest = build_render_manifest(&mut director, 0.5);
        let config = VerificationConfig::default();
        let issues = manifest.verify(&mut director, &config);

        let result = manifest.repair(&mut director, &issues);
        assert!(!result.affected_nodes.is_empty());
        assert!(result.affected_nodes.contains(&caption_id));
        assert!(!result.affected_regions.is_empty());
        assert_eq!(result.queued_issue_count, issues.len());
        assert!(director.dirty_nodes.contains(&caption_id));
    }
}