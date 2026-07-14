//! # Smart Caption Node
//!
//! A caption node that follows a temporally-smooth trajectory solved from
//! semantic entity tracks (faces, speakers, subjects). It wraps a `CaptionNode`
//! and switches the caption's anchor at runtime so the caption avoids faces
//! across a shot.
//!
//! ## Responsibilities
//! - **Trajectory Following**: Sample a `CaptionTrajectory` at the current time.
//! - **Anchor Switching**: Map the solved position to `CaptionPosition::Top`,
//!   `Bottom`, `Left`, `Right`, or `FreeXY` on the inner `CaptionNode`.
//! - **Delegation**: Forward all other `Element` behavior to the inner caption.
//!
//! ## Key Types
//! - `SmartCaptionNode`: Wrapper around `CaptionNode` + `CaptionTrajectory`.

use crate::element::Element;
use crate::entity::EntityRegistry;
use serde::{Deserialize, Serialize};
use crate::node::caption::{CaptionNode, CaptionPosition};
use crate::semantic::intersection_area;
use crate::semantic::{
    solve_caption_trajectory, solve_caption_trajectory_continuous, CaptionTrajectory,
    SemanticManifest,
};
use skia_safe::{Canvas, Rect};
use std::any::Any;
use std::fmt;
use taffy::geometry::Size;
use taffy::style::{AvailableSpace, Style};
use tracing::warn;

/// Configuration for solving a smart-caption trajectory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SmartCaptionConfig {
    /// Output canvas width in pixels.
    pub canvas_w: f32,
    /// Output canvas height in pixels.
    pub canvas_h: f32,
    /// Expected caption block width in pixels.
    pub caption_w: f32,
    /// Expected caption block height in pixels.
    pub caption_h: f32,
    /// Safe-zone margin from the top of the frame.
    pub safe_top: f32,
    /// Safe-zone margin from the bottom of the frame.
    pub safe_bottom: f32,
    /// Safe-zone margin from the left of the frame.
    pub safe_left: f32,
    /// Safe-zone margin from the right of the frame.
    pub safe_right: f32,
    /// Trajectory sampling rate in samples per second.
    pub sample_rate: f64,
    /// Whether to use the continuous temporal constraint solver.
    pub continuous: bool,
}

impl Default for SmartCaptionConfig {
    fn default() -> Self {
        Self {
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
        }
    }
}

/// A caption node that dynamically switches between top, bottom, left, and
/// right anchors based on a pre-solved semantic trajectory.
#[derive(Clone)]
pub struct SmartCaptionNode {
    pub inner: CaptionNode,
    pub trajectory: CaptionTrajectory,
    /// Canvas dimensions used when the trajectory was solved. Stored so the
    /// anchor decision boundaries match the solve coordinates.
    pub(crate) canvas_w: f32,
    pub(crate) canvas_h: f32,
    /// Whether the trajectory was produced by the continuous solver and should
    /// be applied as absolute (x, y) positions rather than snapped to an anchor.
    continuous: bool,
    /// Semantic manifest used to produce the trajectory. Stored so the node
    /// can be fully reconstructed after a project save/load round-trip.
    pub manifest: Option<SemanticManifest>,
    /// Solver configuration used to produce the trajectory. Stored alongside
    /// the manifest for full round-trip fidelity.
    pub config: Option<SmartCaptionConfig>,
}

impl SmartCaptionNode {
    /// Create a new smart caption from an existing caption node and a trajectory.
    ///
    /// # Note
    /// Assumes a 1080x1920 canvas. If the trajectory was solved with different
    /// dimensions, use [`SmartCaptionNode::new_with_canvas`] instead.
    pub fn new(inner: CaptionNode, trajectory: CaptionTrajectory) -> Self {
        let canvas_h = inner.style.reference_canvas_height;
        Self::new_with_canvas(inner, trajectory, 1080.0, canvas_h)
    }

    /// Create a new smart caption with explicit canvas dimensions.
    pub fn new_with_canvas(
        inner: CaptionNode,
        trajectory: CaptionTrajectory,
        canvas_w: f32,
        canvas_h: f32,
    ) -> Self {
        Self {
            inner,
            trajectory,
            canvas_w,
            canvas_h,
            continuous: false,
            manifest: None,
            config: None,
        }
    }

    /// Whether the trajectory was produced by the continuous solver.
    pub fn is_continuous(&self) -> bool {
        self.continuous
    }

    /// Re-solve the caption trajectory from the current entity tracks.
    ///
    /// This is used by the render manifest repair pass when a caption-over-face
    /// issue is detected: the entity registry may have been updated since the
    /// trajectory was first computed, so we re-run the solver with the stored
    /// manifest/config and replace the trajectory in place.
    ///
    /// Returns `true` if a new trajectory was computed, `false` if the node
    /// was built without a manifest/config and cannot be re-solved.
    pub fn re_solve(&mut self, registry: &EntityRegistry) -> bool {
        let (Some(manifest), Some(config)) = (&self.manifest, &self.config) else {
            tracing::warn!(
                "Cannot re_solve SmartCaptionNode without a stored manifest and config"
            );
            return false;
        };
        self.trajectory = Self::solve(registry, manifest, config, self.continuous);
        if self.trajectory.is_empty() {
            tracing::warn!("SmartCaption re_solve produced an empty trajectory");
        }
        true
    }

    /// Create a smart caption from pre-built parts, including the manifest and
    /// config used to produce the trajectory. This is used by project load to
    /// reconstruct a node without re-solving.
    pub fn from_parts(
        inner: CaptionNode,
        trajectory: CaptionTrajectory,
        canvas_w: f32,
        canvas_h: f32,
        continuous: bool,
        manifest: Option<SemanticManifest>,
        config: Option<SmartCaptionConfig>,
    ) -> Self {
        Self {
            inner,
            trajectory,
            canvas_w,
            canvas_h,
            continuous,
            manifest,
            config,
        }
    }

    /// Build a smart caption by solving a trajectory from face/subject tracks.
    ///
    /// The solver places the caption in negative space while avoiding overlap
    /// with tracked faces. It uses dynamic programming to choose between top,
    /// bottom, left, and right anchors over time, then smooths the resulting
    /// trajectory.
    ///
    /// `caption_w`/`caption_h` are the expected render sizes of the caption
    /// block; they can be obtained from `CaptionNode::measure` once layout is
    /// known, or left at defaults if the caption size is not yet available.
    pub fn from_manifest(
        inner: CaptionNode,
        registry: &EntityRegistry,
        manifest: &SemanticManifest,
        config: SmartCaptionConfig,
    ) -> Self {
        Self::build(inner, registry, manifest, config, false)
    }

    /// Build a smart caption using the continuous temporal constraint solver.
    ///
    /// The resulting trajectory contains absolute canvas (x, y) coordinates. The
    /// inner caption is placed with [`CaptionPosition::FreeXY`] so it follows
    /// the solved trajectory exactly.
    ///
    /// # Note
    /// Because [`CaptionPosition::FreeXY`] stores absolute canvas coordinates,
    /// the caption node should fill the full output canvas for the trajectory
    /// to map correctly. If the node is placed inside a smaller layout rect,
    /// the coordinates will be interpreted relative to the canvas origin and
    /// may not align with the intended safe area.
    pub fn from_manifest_continuous(
        inner: CaptionNode,
        registry: &EntityRegistry,
        manifest: &SemanticManifest,
        config: SmartCaptionConfig,
    ) -> Self {
        Self::build(inner, registry, manifest, config, true)
    }

    fn build(
        inner: CaptionNode,
        registry: &EntityRegistry,
        manifest: &SemanticManifest,
        config: SmartCaptionConfig,
        continuous: bool,
    ) -> Self {
        Self::warn_missing_entities(registry, manifest);
        let trajectory = Self::solve(registry, manifest, &config, continuous);
        if trajectory.is_empty() {
            warn!(
                "SmartCaption {}trajectory is empty (manifest time range {:.3}..{:.3}); \
                 caption will use its default position",
                if continuous { "continuous " } else { "" },
                manifest.start_time,
                manifest.end_time
            );
        }
        Self {
            inner,
            trajectory,
            canvas_w: config.canvas_w,
            canvas_h: config.canvas_h,
            continuous,
            manifest: Some(manifest.clone()),
            config: Some(config),
        }
    }

    fn warn_missing_entities(registry: &EntityRegistry, manifest: &SemanticManifest) {
        let missing: Vec<_> = manifest
            .entity_refs
            .iter()
            .filter(|id| !registry.contains(**id))
            .collect();
        if !missing.is_empty() {
            warn!(
                "SmartCaption manifest references {} entity id(s) not present in the registry",
                missing.len()
            );
        }
    }

    fn solve(
        registry: &EntityRegistry,
        manifest: &SemanticManifest,
        config: &SmartCaptionConfig,
        continuous: bool,
    ) -> CaptionTrajectory {
        if continuous {
            solve_caption_trajectory_continuous(
                registry,
                manifest,
                config.canvas_w,
                config.canvas_h,
                config.caption_w,
                config.caption_h,
                config.safe_top,
                config.safe_bottom,
                config.safe_left,
                config.safe_right,
                config.sample_rate,
            )
        } else {
            solve_caption_trajectory(
                registry,
                manifest,
                config.canvas_w,
                config.canvas_h,
                config.caption_w,
                config.caption_h,
                config.safe_top,
                config.safe_bottom,
                config.safe_left,
                config.safe_right,
                config.sample_rate,
            )
        }
    }

    /// Update the inner caption's position based on the trajectory sample at `time`.
    fn apply_trajectory(&mut self, time: f64) {
        if let Some((x, y)) = self.trajectory.sample_at(time) {
            if self.continuous {
                self.inner.set_position(CaptionPosition::FreeXY { x, y });
            } else {
                // Map the solved anchor position to the nearest discrete anchor.
                let position = Self::anchor_from_position(x, y, self.canvas_w, self.canvas_h);
                self.inner.set_position(position);
            }
        }
    }

    /// Map an absolute canvas position to the nearest discrete caption anchor.
    ///
    /// The discrete solver can now place captions at left/center/right variants
    /// of the top and bottom anchors. Off-center top/bottom positions are
    /// mapped to the new `TopLeft`, `TopRight`, `BottomLeft`, and `BottomRight`
    /// variants so responsive margins are preserved.
    fn anchor_from_position(x: f32, y: f32, canvas_w: f32, canvas_h: f32) -> CaptionPosition {
        let center_x = canvas_w / 2.0;
        let center_y = canvas_h / 2.0;
        let dx = (x - center_x).abs();
        let dy = (y - center_y).abs();

        // Side anchors: when x is farther from center than y, use Left/Right.
        if dx > dy {
            return if x < center_x {
                CaptionPosition::Left
            } else {
                CaptionPosition::Right
            };
        }

        // Vertical anchors. Use a small threshold (5% of canvas width) so
        // smoothed samples that are only slightly off-center still map to the
        // centered Top/Bottom. This avoids flickering when the solver briefly
        // transitions between a center and a side anchor.
        let horizontal_threshold = canvas_w * 0.05;
        let is_centered = dx < horizontal_threshold;

        if y < center_y {
            if is_centered {
                CaptionPosition::Top
            } else if x < center_x {
                CaptionPosition::TopLeft
            } else {
                CaptionPosition::TopRight
            }
        } else if is_centered {
            CaptionPosition::Bottom
        } else if x < center_x {
            CaptionPosition::BottomLeft
        } else {
            CaptionPosition::BottomRight
        }
    }
}

impl fmt::Debug for SmartCaptionNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmartCaptionNode")
            .field("inner", &self.inner)
            .field("trajectory_samples", &self.trajectory.len())
            .finish()
    }
}

impl Element for SmartCaptionNode {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn kind(&self) -> &'static str {
        "smart_caption"
    }

    fn needs_measure(&self) -> bool {
        self.inner.needs_measure()
    }

    fn measure(
        &self,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
    ) -> Size<f32> {
        self.inner.measure(known_dimensions, available_space)
    }

    fn layout_style(&self) -> Style {
        self.inner.layout_style()
    }

    fn set_layout_style(&mut self, style: Style) {
        self.inner.set_layout_style(style);
    }

    fn update(&mut self, time: f64) -> bool {
        self.apply_trajectory(time);
        self.inner.update(time)
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), crate::RenderError> {
        self.inner.render(canvas, rect, opacity, draw_children)
    }

    fn animate_property(
        &mut self,
        property: &str,
        start: f32,
        target: f32,
        duration: f64,
        easing: &str,
    ) {
        self.inner
            .animate_property(property, start, target, duration, easing);
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.inner.set_opacity(opacity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::caption::{CaptionLine, CaptionNode, CaptionPosition, CaptionWord};
    use crate::semantic::{CaptionTrajectory, TrajectorySample};
    use skia_safe::textlayout::FontCollection;
    use std::sync::{Arc, Mutex};

    fn empty_font_collection() -> Arc<Mutex<FontCollection>> {
        Arc::new(Mutex::new(FontCollection::new()))
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
    fn switches_to_top_when_trajectory_is_top() {
        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let inner = CaptionNode::new(lines, empty_font_collection());
        let trajectory = CaptionTrajectory::new(vec![
            TrajectorySample { time: 0.0, x: 540.0, y: 100.0 },
            TrajectorySample { time: 2.0, x: 540.0, y: 100.0 },
        ]);
        let mut node = SmartCaptionNode::new(inner, trajectory);
        node.update(0.5);
        assert_eq!(node.inner.style.position, CaptionPosition::Top);
    }

    #[test]
    fn switches_to_bottom_when_trajectory_is_bottom() {
        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let inner = CaptionNode::new(lines, empty_font_collection());
        let trajectory = CaptionTrajectory::new(vec![
            TrajectorySample { time: 0.0, x: 540.0, y: 1700.0 },
            TrajectorySample { time: 2.0, x: 540.0, y: 1700.0 },
        ]);
        let mut node = SmartCaptionNode::new(inner, trajectory);
        node.update(0.5);
        assert_eq!(node.inner.style.position, CaptionPosition::Bottom);
    }

    #[test]
    fn switches_mid_shot_when_trajectory_changes() {
        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let inner = CaptionNode::new(lines, empty_font_collection());
        let trajectory = CaptionTrajectory::new(vec![
            TrajectorySample { time: 0.0, x: 540.0, y: 100.0 },
            TrajectorySample { time: 1.0, x: 540.0, y: 1700.0 },
            TrajectorySample { time: 2.0, x: 540.0, y: 1700.0 },
        ]);
        let mut node = SmartCaptionNode::new(inner, trajectory);
        node.update(0.2);
        assert_eq!(node.inner.style.position, CaptionPosition::Top);
        node.update(1.5);
        assert_eq!(node.inner.style.position, CaptionPosition::Bottom);
    }

    fn make_manifest(entity_refs: Vec<crate::entity::EntityId>) -> crate::semantic::SemanticManifest {
        use crate::semantic::SolverConfig;
        crate::semantic::SemanticManifest {
            entity_refs,
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        }
    }

    fn default_config() -> SmartCaptionConfig {
        SmartCaptionConfig {
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
        }
    }

    #[test]
    fn from_manifest_avoids_face_at_bottom() {
        use crate::entity::{BoundingBox, EntityKind, EntityRegistry};

        let mut registry = EntityRegistry::new();
        let face = registry.create(EntityKind::Face, "mara".to_string());
        // Face occupies the bottom-center of a 1080×1920 frame.
        registry.add_sample(face, 0.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);
        registry.add_sample(face, 2.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);

        let manifest = make_manifest(vec![face]);
        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let inner = CaptionNode::new(lines, empty_font_collection());
        let mut node = SmartCaptionNode::from_manifest(inner, &registry, &manifest, default_config());

        node.update(0.5);
        // Caption should be placed at the top to avoid the bottom face.
        assert!(
            matches!(node.inner.style.position, CaptionPosition::Top
                | CaptionPosition::TopLeft
                | CaptionPosition::TopRight),
            "caption should use a top anchor, got {:?}",
            node.inner.style.position
        );
        // The solved trajectory y should be in the top half of the frame.
        let (_x, y) = node.trajectory.sample_at(0.5).unwrap();
        assert!(y < 960.0, "trajectory should stay in top half, got y={}", y);
    }

    #[test]
    fn from_manifest_avoids_face_at_top() {
        use crate::entity::{BoundingBox, EntityKind, EntityRegistry};

        let mut registry = EntityRegistry::new();
        let face = registry.create(EntityKind::Face, "mara".to_string());
        // Face occupies the top-center of the frame.
        registry.add_sample(face, 0.0, BoundingBox::new(400.0, 60.0, 200.0, 200.0), 1.0);
        registry.add_sample(face, 2.0, BoundingBox::new(400.0, 60.0, 200.0, 200.0), 1.0);

        let manifest = make_manifest(vec![face]);
        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let inner = CaptionNode::new(lines, empty_font_collection());
        let mut node = SmartCaptionNode::from_manifest(inner, &registry, &manifest, default_config());

        node.update(0.5);
        // Caption should be placed at the bottom to avoid the top face.
        assert!(
            matches!(node.inner.style.position, CaptionPosition::Bottom
                | CaptionPosition::BottomLeft
                | CaptionPosition::BottomRight),
            "caption should use a bottom anchor, got {:?}",
            node.inner.style.position
        );
        let (_x, y) = node.trajectory.sample_at(0.5).unwrap();
        assert!(y >= 960.0, "trajectory should stay in bottom half, got y={}", y);
    }

    #[test]
    fn from_manifest_defaults_to_top_when_no_faces() {
        use crate::entity::EntityRegistry;

        let registry = EntityRegistry::new();
        let manifest = make_manifest(vec![]);
        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let inner = CaptionNode::new(lines, empty_font_collection());
        let mut node = SmartCaptionNode::from_manifest(inner, &registry, &manifest, default_config());

        node.update(0.5);
        // With no faces the center bias should keep the caption at the top.
        assert_eq!(node.inner.style.position, CaptionPosition::Top);
    }

    #[test]
    fn from_manifest_avoids_moving_face() {
        // The solver may choose any anchor; verify the rendered caption
        // block never overlaps the tracked face.
        use crate::entity::{BoundingBox, EntityKind, EntityRegistry};

        let mut registry = EntityRegistry::new();
        let face = registry.create(EntityKind::Face, "mara".to_string());
        // Face starts at the bottom and moves to the top.
        registry.add_sample(face, 0.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);
        registry.add_sample(face, 2.0, BoundingBox::new(400.0, 60.0, 200.0, 200.0), 1.0);

        let manifest = make_manifest(vec![face]);
        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let inner = CaptionNode::new(lines, empty_font_collection());
        let mut node = SmartCaptionNode::from_manifest(inner, &registry, &manifest, default_config());

        // Sample several times and verify the chosen anchor block never overlaps the face.
        let config = default_config();
        for t in [0.0, 0.5, 1.0, 1.5, 2.0] {
            node.update(t);
            let face_sample = registry.sample_at(face, t).unwrap();
            let (x, y) = anchor_center_for_position(&node.inner.style.position, &config);
            let caption_bbox = BoundingBox::new(
                x - config.caption_w / 2.0,
                y - config.caption_h / 2.0,
                config.caption_w,
                config.caption_h,
            );
            let overlap = intersection_area(&caption_bbox, &face_sample.bbox) > 0.0;
            assert!(
                !overlap,
                "caption at t={} overlaps face (caption x={}, y={}, face y={}-{})",
                t, x, y, face_sample.bbox.y, face_sample.bbox.y + face_sample.bbox.height
            );
        }
    }

    /// Compute the canonical center for a discrete caption anchor given the
    /// solve config. Used by tests to verify face avoidance.
    fn anchor_center_for_position(position: &CaptionPosition, config: &SmartCaptionConfig) -> (f32, f32) {
        let center_x = config.canvas_w / 2.0;
        let center_y = config.canvas_h / 2.0;
        match position {
            CaptionPosition::Top => (center_x, config.safe_top + config.caption_h / 2.0),
            CaptionPosition::Bottom => (center_x, config.canvas_h - config.safe_bottom - config.caption_h / 2.0),
            CaptionPosition::Left => (config.safe_left + config.caption_w / 2.0, center_y),
            CaptionPosition::Right => (config.canvas_w - config.safe_right - config.caption_w / 2.0, center_y),
            CaptionPosition::TopLeft => (config.safe_left + config.caption_w / 2.0, config.safe_top + config.caption_h / 2.0),
            CaptionPosition::TopRight => (config.canvas_w - config.safe_right - config.caption_w / 2.0, config.safe_top + config.caption_h / 2.0),
            CaptionPosition::BottomLeft => (config.safe_left + config.caption_w / 2.0, config.canvas_h - config.safe_bottom - config.caption_h / 2.0),
            CaptionPosition::BottomRight => (config.canvas_w - config.safe_right - config.caption_w / 2.0, config.canvas_h - config.safe_bottom - config.caption_h / 2.0),
            CaptionPosition::Free(y) => (center_x, *y),
            CaptionPosition::FreeXY { x, y } => (*x, *y),
            CaptionPosition::Center => (center_x, center_y),
        }
    }

    #[test]
    fn from_manifest_dodges_to_top_side_when_center_is_blocked() {
        use crate::entity::{BoundingBox, EntityKind, EntityRegistry};

        let mut registry = EntityRegistry::new();
        let face = registry.create(EntityKind::Face, "top_center".to_string());
        // Face at the top-center; with a narrow caption the solver can dodge
        // to the top-left or top-right.
        registry.add_sample(face, 0.0, BoundingBox::new(300.0, 60.0, 200.0, 300.0), 1.0);
        registry.add_sample(face, 2.0, BoundingBox::new(300.0, 60.0, 200.0, 300.0), 1.0);

        let manifest = make_manifest(vec![face]);
        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let inner = CaptionNode::new(lines, empty_font_collection());
        let mut config = default_config();
        config.caption_w = 400.0;
        let mut node = SmartCaptionNode::from_manifest(inner, &registry, &manifest, config);

        node.update(0.5);
        // The face blocks the top-center; the solver should dodge to one of
        // the top side anchors (the exact side depends on clearance geometry).
        assert!(
            matches!(
                node.inner.style.position,
                CaptionPosition::TopLeft | CaptionPosition::TopRight
            ),
            "caption should dodge to a top side anchor, got {:?}",
            node.inner.style.position
        );
    }

    #[test]
    fn from_manifest_continuous_uses_free_xy_position() {
        use crate::entity::{BoundingBox, EntityKind, EntityRegistry};

        let mut registry = EntityRegistry::new();
        let face = registry.create(EntityKind::Face, "mara".to_string());
        // Face at bottom center.
        registry.add_sample(face, 0.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);
        registry.add_sample(face, 2.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);

        let manifest = make_manifest(vec![face]);
        let lines = vec![single_word_line("hello", 0.0, 2.0)];
        let inner = CaptionNode::new(lines, empty_font_collection());
        let mut node = SmartCaptionNode::from_manifest_continuous(
            inner,
            &registry,
            &manifest,
            default_config(),
        );

        node.update(0.5);
        // Continuous solver should place caption above the bottom face.
        let (_x, y) = node.trajectory.sample_at(0.5).unwrap();
        assert!(y < 1500.0, "continuous trajectory should avoid bottom face, got y={}", y);
        // The inner caption should be using the FreeXY position.
        assert!(matches!(node.inner.style.position, CaptionPosition::FreeXY { .. }));
    }
}
