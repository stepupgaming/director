//! # Active Speaker Camera Node
//!
//! A video node that digitally reframes (pans/crops/zooms) footage to follow
//! a tracked speaker entity. It pre-solves a smooth camera trajectory from
//! entity tracks and continuity groups, then renders the video with a dynamic
//! source crop rectangle.
//!
//! ## Responsibilities
//! - **Trajectory Solving**: Convert entity tracks + continuity groups into a
//!   smooth sequence of crop centers and heights over time.
//! - **Smooth Tracking**: Apply temporal smoothing within a continuity group
//!   to avoid jitter from noisy detection.
//! - **Hard Cuts**: Do not smooth across continuity group boundaries.
//! - **Delegation**: Decode frames using an inner `VideoNode`, then render with
//!   a custom source crop rectangle.
//!
//! ## Key Types
//! - `ActiveSpeakerCamera`: The node wrapping `VideoNode` + `CameraTrajectory`.
//! - `ContinuityGroup`: A time range + target entity + framing preset.
//! - `Framing`: Close-up, medium, wide, or custom zoom multiplier.
//! - `CameraTrajectory`: Sampled crop center/height over time.

use crate::element::Element;
use crate::entity::{EntityId, EntityRegistry};
use crate::errors::RenderError;
use crate::node::video_node::{VideoNode, VideoSource};
use crate::video_wrapper::RenderMode;
use skia_safe::{Canvas, Color4f, Paint, Rect};
use std::any::Any;
use std::path::PathBuf;
use taffy::style::Style;
use tracing::warn;

/// Framing preset for a continuity group.
#[derive(Clone, Debug, PartialEq)]
pub enum Framing {
    /// Tight framing around the subject (2.5x bbox height).
    CloseUp,
    /// Medium shot (4x bbox height).
    Medium,
    /// Wide shot (6x bbox height).
    Wide,
    /// Custom multiplier relative to the tracked bbox height.
    Custom(f32),
}

impl Framing {
    /// Multiplier applied to the tracked bbox height to determine crop height.
    pub fn multiplier(&self) -> f32 {
        match self {
            Framing::CloseUp => 2.5,
            Framing::Medium => 4.0,
            Framing::Wide => 6.0,
            Framing::Custom(m) => m.max(0.1),
        }
    }
}

/// A continuity group defines which entity to follow and how to frame it
/// during a specific time range.
#[derive(Clone, Debug)]
pub struct ContinuityGroup {
    /// Start time in seconds (inclusive).
    pub start_time: f64,
    /// End time in seconds (exclusive).
    pub end_time: f64,
    /// Entity to track (typically a speaker).
    pub target_id: EntityId,
    /// Framing preset.
    pub framing: Framing,
    /// Optional face tracks to prioritize for framing. When faces are
    /// available, their bounding-box union is used for centering and crop
    /// height; otherwise the solver falls back to `target_id`.
    pub face_ids: Vec<EntityId>,
}

impl ContinuityGroup {
    /// Create a new continuity group.
    pub fn new(start_time: f64, end_time: f64, target_id: EntityId, framing: Framing) -> Self {
        Self {
            start_time,
            end_time,
            target_id,
            framing,
            face_ids: Vec::new(),
        }
    }

    /// Attach face tracks to this continuity group.
    pub fn with_face_ids(mut self, face_ids: Vec<EntityId>) -> Self {
        self.face_ids = face_ids;
        self
    }
}

/// A single sample of the camera trajectory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraSample {
    pub time: f64,
    /// Crop center x in source video pixels.
    pub center_x: f32,
    /// Crop center y in source video pixels.
    pub center_y: f32,
    /// Crop height in source video pixels.
    pub height: f32,
}

/// A camera trajectory: sampled crop centers and heights over time.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraTrajectory {
    samples: Vec<CameraSample>,
}

impl CameraTrajectory {
    /// Create a new trajectory, sorting samples by time.
    pub fn new(mut samples: Vec<CameraSample>) -> Self {
        samples.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Self { samples }
    }

    /// Number of samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether the trajectory is empty.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Access samples as a slice.
    pub fn as_slice(&self) -> &[CameraSample] {
        &self.samples
    }

    /// Sample the trajectory at a given time using linear interpolation.
    pub fn sample_at(&self, time: f64) -> Option<CameraSample> {
        let first = self.samples.first()?;
        let last = self.samples.last()?;
        if time <= first.time {
            return Some(first.clone());
        }
        if time >= last.time {
            return Some(last.clone());
        }
        let idx = self.samples.partition_point(|s| s.time < time);
        let prev = &self.samples[idx - 1];
        let next = &self.samples[idx];
        let t = if next.time > prev.time {
            ((time - prev.time) / (next.time - prev.time)) as f32
        } else {
            0.0
        };
        Some(CameraSample {
            time,
            center_x: prev.center_x + (next.center_x - prev.center_x) * t,
            center_y: prev.center_y + (next.center_y - prev.center_y) * t,
            height: prev.height + (next.height - prev.height) * t,
        })
    }
}

/// Configuration for solving the active-speaker camera trajectory.
#[derive(Clone, Debug)]
pub struct ActiveSpeakerCameraConfig {
    /// Output canvas width in pixels.
    pub canvas_w: f32,
    /// Output canvas height in pixels.
    pub canvas_h: f32,
    /// Trajectory sampling rate in samples per second.
    pub sample_rate: f64,
    /// Smoothing window size (number of samples). Larger = smoother.
    pub smoothing_window: usize,
    /// Vertical offset applied to the tracked bbox center (0.0 = center,
    /// negative = up, positive = down). Default biases slightly upward to
    /// keep the speaker's eyes in the upper third.
    pub center_offset_y: f32,
}

impl Default for ActiveSpeakerCameraConfig {
    fn default() -> Self {
        Self {
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            sample_rate: 10.0,
            smoothing_window: 5,
            center_offset_y: -0.15,
        }
    }
}

/// A video node that digitally reframes footage to follow a tracked speaker.
#[derive(Clone)]
pub struct ActiveSpeakerCamera {
    inner: VideoNode,
    trajectory: CameraTrajectory,
    /// Last known video dimensions, used for clamping the crop rectangle.
    video_size: (f32, f32),
    /// Time of the most recent `update()` call. Used to sample the trajectory
    /// because decoder timestamps may not match the Director timeline.
    last_time: f64,
    /// Continuity groups that define which entity to follow and when.
    groups: Vec<ContinuityGroup>,
}

impl ActiveSpeakerCamera {
    /// Set the object-fit mode on the inner video node.
    pub fn set_object_fit(&mut self, fit: crate::types::ObjectFit) {
        self.inner.object_fit = fit;
    }

    /// Set the border radius on the inner video node.
    pub fn set_border_radius(&mut self, radius: f32) {
        self.inner.border_radius = crate::animation::Animated::new(radius);
    }

    /// Mutable access to the inner video node for layout/style updates.
    pub fn inner_mut(&mut self) -> &mut VideoNode {
        &mut self.inner
    }

    /// Access the solved camera trajectory.
    pub fn trajectory(&self) -> &CameraTrajectory {
        &self.trajectory
    }

    /// Set the cached video dimensions used for clamping the crop rectangle.
    ///
    /// This is normally populated automatically when a frame is decoded, but
    /// it can be set explicitly for testing or verification.
    #[cfg(test)]
    pub(crate) fn set_video_size(&mut self, width: f32, height: f32) {
        self.video_size = (width, height);
    }

    /// Check whether the tracked subject is fully inside the reframed crop at
    /// the given time. Returns the subject's bounding box if it is (partially)
    /// outside the crop rectangle.
    ///
    /// `output_rect` should be the node's actual layout bounds so the crop
    /// aspect ratio matches what will be rendered.
    pub fn subject_out_of_frame_at(
        &self,
        time: f64,
        registry: &EntityRegistry,
        output_rect: Rect,
    ) -> Option<crate::types::BoundsRect> {
        let (video_w, video_h) = self.video_size;
        if video_w <= 0.0 || video_h <= 0.0 {
            return None;
        }

        let sample = self.trajectory.sample_at(time)?;
        let crop = self.compute_crop_rect(&sample, output_rect, video_w, video_h)?;

        let group = self
            .groups
            .iter()
            .find(|g| g.start_time <= time && time < g.end_time)?;
        let bbox = group_framing_bbox(registry, group, time)?;

        let left_in = bbox.x >= crop.left;
        let top_in = bbox.y >= crop.top;
        let right_in = bbox.x + bbox.width <= crop.right;
        let bottom_in = bbox.y + bbox.height <= crop.bottom;

        if left_in && top_in && right_in && bottom_in {
            None
        } else {
            Some(crate::types::BoundsRect {
                x: bbox.x,
                y: bbox.y,
                w: bbox.width,
                h: bbox.height,
            })
        }
    }

    /// Create a new active-speaker camera from a video source and continuity groups.
    pub fn from_source(
        source: VideoSource,
        mode: RenderMode,
        registry: &EntityRegistry,
        groups: Vec<ContinuityGroup>,
        config: ActiveSpeakerCameraConfig,
    ) -> Self {
        let inner = VideoNode::new(source, mode);
        let trajectory = solve_camera_trajectory(registry, &groups, &config);
        if trajectory.is_empty() {
            warn!(
                "ActiveSpeakerCamera trajectory is empty (groups: {}); \
                 video will render without reframing",
                groups.len()
            );
        }
        Self {
            inner,
            trajectory,
            video_size: (0.0, 0.0),
            last_time: 0.0,
            groups,
        }
    }

    /// Create from a file path.
    pub fn from_path(
        path: impl Into<PathBuf>,
        mode: RenderMode,
        registry: &EntityRegistry,
        groups: Vec<ContinuityGroup>,
        config: ActiveSpeakerCameraConfig,
    ) -> Self {
        Self::from_source(VideoSource::Path(path.into()), mode, registry, groups, config)
    }

    /// Create from raw bytes.
    pub fn from_bytes(
        data: Vec<u8>,
        mode: RenderMode,
        registry: &EntityRegistry,
        groups: Vec<ContinuityGroup>,
        config: ActiveSpeakerCameraConfig,
    ) -> Self {
        Self::from_source(VideoSource::Bytes(data), mode, registry, groups, config)
    }

    /// Compute the source crop rectangle for the given sample and output rect.
    ///
    /// `video_w`/`video_h` are the source dimensions. The returned rectangle
    /// is clamped to the source bounds and preserves the output aspect ratio.
    fn compute_crop_rect(
        &self,
        sample: &CameraSample,
        output_rect: Rect,
        video_w: f32,
        video_h: f32,
    ) -> Option<Rect> {
        if video_w <= 0.0 || video_h <= 0.0 || sample.height <= 0.0 {
            return None;
        }

        let output_aspect = output_rect.width() / output_rect.height();
        let crop_w = sample.height * output_aspect;
        let crop_h = sample.height;

        let half_w = crop_w / 2.0;
        let half_h = crop_h / 2.0;

        // Start with the ideal crop centered on the sample.
        let mut left = sample.center_x - half_w;
        let mut top = sample.center_y - half_h;
        let mut right = left + crop_w;
        let mut bottom = top + crop_h;

        // Clamp to video bounds and then shift to preserve size if possible.
        if right > video_w {
            left -= right - video_w;
            right = video_w;
        }
        if left < 0.0 {
            right -= left;
            left = 0.0;
        }
        if bottom > video_h {
            top -= bottom - video_h;
            bottom = video_h;
        }
        if top < 0.0 {
            bottom -= top;
            top = 0.0;
        }

        // Final clamp in case the crop is larger than the video.
        Some(Rect::from_ltrb(
            left.max(0.0),
            top.max(0.0),
            right.min(video_w),
            bottom.min(video_h),
        ))
    }
}

impl std::fmt::Debug for ActiveSpeakerCamera {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveSpeakerCamera")
            .field("trajectory_samples", &self.trajectory.len())
            .field("video_size", &self.video_size)
            .finish()
    }
}

impl Element for ActiveSpeakerCamera {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn kind(&self) -> &'static str {
        "active_speaker_camera"
    }

    fn layout_style(&self) -> Style {
        self.inner.layout_style()
    }

    fn set_layout_style(&mut self, style: Style) {
        self.inner.set_layout_style(style);
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.inner.set_opacity(opacity);
    }

    fn update(&mut self, time: f64) -> bool {
        self.last_time = time;
        // Update the inner video node so it decodes the current frame.
        let changed = self.inner.update(time);
        // Cache video dimensions from the decoded frame.
        if let Some((_, img)) = self.inner.get_current_frame() {
            self.video_size = (img.width() as f32, img.height() as f32);
        }
        changed
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        parent_opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        let Some((_, img)) = self.inner.get_current_frame() else {
            // No frame decoded yet; draw a placeholder and children.
            let paint = Paint::new(Color4f::new(0.0, 0.0, 0.0, parent_opacity), None);
            canvas.draw_rect(rect, &paint);
            draw_children(canvas);
            return Ok(());
        };

        let video_w = img.width() as f32;
        let video_h = img.height() as f32;

        // Sample the trajectory at the last update time. We use the time
        // passed to `update()` rather than the decoded frame timestamp because
        // decoder timestamps may not match the Director's local timeline.
        let sample = self.trajectory.sample_at(self.last_time).unwrap_or_default();

        let paint = Paint::new(Color4f::new(1.0, 1.0, 1.0, parent_opacity), None);

        if let Some(src_rect) = self.compute_crop_rect(&sample, rect, video_w, video_h) {
            canvas.save();
            canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, true);
            canvas.draw_image_rect(
                &img,
                Some((&src_rect, skia_safe::canvas::SrcRectConstraint::Strict)),
                rect,
                &paint,
            );
            canvas.restore();
        } else {
            // Fallback: draw the full frame.
            canvas.draw_image_rect(&img, None, rect, &paint);
        }

        draw_children(canvas);
        Ok(())
    }

    fn animate_property(
        &mut self,
        property: &str,
        start: f32,
        target: f32,
        duration: f64,
        easing: &str,
    ) {
        self.inner.animate_property(property, start, target, duration, easing);
    }

    fn animate_property_spring(
        &mut self,
        property: &str,
        start: Option<f32>,
        target: f32,
        config: crate::animation::SpringConfig,
    ) {
        self.inner.animate_property_spring(property, start, target, config);
    }
}

/// Compute the effective bounding box used for framing a continuity group
/// at a given time.
///
/// If the group has face tracks and any of them has a sample at `time`, the
/// union of those face bounding boxes is returned. Otherwise, the group's
/// target entity bounding box is used as a fallback.
pub(crate) fn group_framing_bbox(
    registry: &EntityRegistry,
    group: &ContinuityGroup,
    time: f64,
) -> Option<crate::entity::BoundingBox> {
    let mut faces: Vec<crate::entity::BoundingBox> = Vec::new();
    for &face_id in &group.face_ids {
        // Only use a face track if its sample range overlaps this continuity
        // group. `sample_at` otherwise returns the nearest sample, which
        // would incorrectly pull in faces from unrelated moments.
        let Some(track) = registry.get(face_id) else { continue };
        let Some((track_start, track_end)) = track.time_range() else { continue };
        if track_end < group.start_time || track_start > group.end_time {
            continue;
        }
        if let Some(sample) = track.sample_at(time) {
            faces.push(sample.bbox);
        }
    }

    if !faces.is_empty() {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for face in &faces {
            min_x = min_x.min(face.x);
            min_y = min_y.min(face.y);
            max_x = max_x.max(face.x + face.width);
            max_y = max_y.max(face.y + face.height);
        }
        Some(crate::entity::BoundingBox::new(
            min_x,
            min_y,
            max_x - min_x,
            max_y - min_y,
        ))
    } else {
        registry
            .sample_at(group.target_id, time)
            .map(|sample| sample.bbox)
    }
}

/// Solve a smooth camera trajectory from continuity groups and entity tracks.
///
/// Samples each group independently at `config.sample_rate`, applies temporal
/// smoothing within each group, and concatenates the results. Boundaries
/// between groups are hard cuts (no smoothing across them).
pub fn solve_camera_trajectory(
    registry: &EntityRegistry,
    groups: &[ContinuityGroup],
    config: &ActiveSpeakerCameraConfig,
) -> CameraTrajectory {
    if groups.is_empty() {
        return CameraTrajectory::default();
    }

    let mut all_samples: Vec<CameraSample> = Vec::new();

    for group in groups {
        let duration = (group.end_time - group.start_time).max(0.0);
        if duration <= 0.0 {
            continue;
        }

        if !registry.contains(group.target_id) {
            warn!(
                "ActiveSpeakerCamera continuity group {:.3}..{:.3} references missing entity {}; skipping",
                group.start_time,
                group.end_time,
                group.target_id
            );
            continue;
        }

        let num_samples = (duration * config.sample_rate).ceil().max(1.0) as usize + 1;
        let mut group_samples = Vec::with_capacity(num_samples);

        for i in 0..num_samples {
            let t = group.start_time + i as f64 / config.sample_rate;
            let clamped_t = t.min(group.end_time);

            if let Some(bbox) = group_framing_bbox(registry, group, clamped_t) {
                let crop_height = bbox.height * group.framing.multiplier();
                // Apply vertical offset relative to bbox height.
                let offset_y = bbox.height * config.center_offset_y;
                group_samples.push(CameraSample {
                    time: clamped_t,
                    center_x: bbox.center().0,
                    center_y: bbox.center().1 + offset_y,
                    height: crop_height,
                });
            }
        }

        // Smooth within the group.
        group_samples = smooth_camera_samples(group_samples, config.smoothing_window);
        all_samples.extend(group_samples);
    }

    CameraTrajectory::new(all_samples)
}

/// Apply a moving-average smoothing to a sequence of camera samples.
fn smooth_camera_samples(samples: Vec<CameraSample>, window: usize) -> Vec<CameraSample> {
    if window <= 1 || samples.len() < 2 {
        return samples;
    }

    let mut smoothed = Vec::with_capacity(samples.len());
    for i in 0..samples.len() {
        let start = i.saturating_sub(window / 2);
        let end = (i + window / 2 + 1).min(samples.len());
        let mut sum_x = 0.0f32;
        let mut sum_y = 0.0f32;
        let mut sum_h = 0.0f32;
        let mut count = 0usize;
        for sample in samples.iter().take(end).skip(start) {
            sum_x += sample.center_x;
            sum_y += sample.center_y;
            sum_h += sample.height;
            count += 1;
        }
        smoothed.push(CameraSample {
            time: samples[i].time,
            center_x: sum_x / count as f32,
            center_y: sum_y / count as f32,
            height: sum_h / count as f32,
        });
    }
    smoothed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{BoundingBox, EntityKind, EntityRegistry};
    use crate::video_wrapper::RenderMode;

    fn default_config() -> ActiveSpeakerCameraConfig {
        ActiveSpeakerCameraConfig {
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            sample_rate: 10.0,
            smoothing_window: 5,
            center_offset_y: -0.15,
        }
    }

    #[test]
    fn trajectory_follows_single_speaker() {
        let mut registry = EntityRegistry::new();
        let speaker = registry.create(EntityKind::Speaker, "mara".to_string());
        registry.add_sample(speaker, 0.0, BoundingBox::new(400.0, 500.0, 200.0, 200.0), 1.0);
        registry.add_sample(speaker, 2.0, BoundingBox::new(600.0, 700.0, 200.0, 200.0), 1.0);

        let groups = vec![ContinuityGroup::new(
            0.0,
            2.0,
            speaker,
            Framing::Medium,
        )];

        let trajectory = solve_camera_trajectory(&registry, &groups, &default_config());
        assert!(!trajectory.is_empty());

        let sample = trajectory.sample_at(1.0).unwrap();
        // Center should be interpolated between the two keyframes.
        // bbox x goes 400 -> 600, so center_x goes 500 -> 700; midpoint is 600.
        assert!(sample.center_x >= 500.0 && sample.center_x <= 700.0);
        assert!(sample.center_y >= 500.0 && sample.center_y <= 700.0);
        // Medium framing = 4x bbox height.
        assert!((sample.height - 800.0).abs() < 50.0);
    }

    #[test]
    fn continuity_groups_create_hard_cut() {
        let mut registry = EntityRegistry::new();
        let speaker_a = registry.create(EntityKind::Speaker, "mara".to_string());
        let speaker_b = registry.create(EntityKind::Speaker, "alex".to_string());

        registry.add_sample(speaker_a, 0.0, BoundingBox::new(200.0, 500.0, 200.0, 200.0), 1.0);
        registry.add_sample(speaker_a, 2.0, BoundingBox::new(200.0, 500.0, 200.0, 200.0), 1.0);
        registry.add_sample(speaker_b, 2.0, BoundingBox::new(800.0, 500.0, 200.0, 200.0), 1.0);
        registry.add_sample(speaker_b, 4.0, BoundingBox::new(800.0, 500.0, 200.0, 200.0), 1.0);

        let groups = vec![
            ContinuityGroup::new(0.0, 2.0, speaker_a, Framing::Medium),
            ContinuityGroup::new(2.0, 4.0, speaker_b, Framing::Medium),
        ];

        let trajectory = solve_camera_trajectory(&registry, &groups, &default_config());

        let before_cut = trajectory.sample_at(1.99).unwrap();
        let after_cut = trajectory.sample_at(2.01).unwrap();

        // Hard cut: center should jump from left to right.
        assert!(before_cut.center_x < 400.0, "should be on left before cut");
        assert!(after_cut.center_x > 600.0, "should be on right after cut");
    }

    #[test]
    fn smoothing_reduces_jitter() {
        let mut registry = EntityRegistry::new();
        let speaker = registry.create(EntityKind::Speaker, "mara".to_string());

        // Speaker bbox oscillates rapidly.
        for i in 0..=20 {
            let t = i as f64 / 10.0;
            let x = 500.0 + (if i % 2 == 0 { 50.0 } else { -50.0 });
            registry.add_sample(speaker, t, BoundingBox::new(x, 500.0, 200.0, 200.0), 1.0);
        }

        let groups = vec![ContinuityGroup::new(
            0.0,
            2.0,
            speaker,
            Framing::Medium,
        )];

        let trajectory = solve_camera_trajectory(&registry, &groups, &default_config());

        let mut max_jump = 0.0f32;
        for window in trajectory.as_slice().windows(2) {
            let jump = (window[1].center_x - window[0].center_x).abs();
            if jump > max_jump {
                max_jump = jump;
            }
        }

        // Raw bbox jumps 100 px per sample; smoothing should reduce that.
        assert!(max_jump < 100.0, "smoothing should reduce jitter, got max jump {}", max_jump);
    }

    #[test]
    fn framing_multipliers() {
        assert!((Framing::CloseUp.multiplier() - 2.5).abs() < f32::EPSILON);
        assert!((Framing::Medium.multiplier() - 4.0).abs() < f32::EPSILON);
        assert!((Framing::Wide.multiplier() - 6.0).abs() < f32::EPSILON);
        assert!((Framing::Custom(3.5).multiplier() - 3.5).abs() < f32::EPSILON);
    }

    #[test]
    fn face_priority_uses_face_bbox() {
        let mut registry = EntityRegistry::new();
        let speaker = registry.create(EntityKind::Speaker, "mara".to_string());
        let face = registry.create(EntityKind::Face, "mara_face".to_string());

        // Speaker body in the lower half of the frame.
        registry.add_sample(speaker, 0.0, BoundingBox::new(400.0, 600.0, 200.0, 400.0), 1.0);
        // Face in the upper half, offset from the body center.
        registry.add_sample(face, 0.0, BoundingBox::new(420.0, 300.0, 160.0, 200.0), 1.0);

        let group = ContinuityGroup::new(0.0, 2.0, speaker, Framing::Medium)
            .with_face_ids(vec![face]);

        let trajectory = solve_camera_trajectory(&registry, &[group], &default_config());
        assert!(!trajectory.is_empty());

        let sample = trajectory.sample_at(0.5).unwrap();
        // Crop center should follow the face (y ~ 400), not the body (y ~ 800).
        assert!(sample.center_y < 600.0, "crop center should follow face, not body");
        // Crop height should be based on face height (200 * 4 = 800).
        assert!((sample.height - 800.0).abs() < 50.0);
    }

    #[test]
    fn face_priority_falls_back_to_speaker_bbox() {
        let mut registry = EntityRegistry::new();
        let speaker = registry.create(EntityKind::Speaker, "mara".to_string());
        registry.add_sample(speaker, 0.0, BoundingBox::new(400.0, 500.0, 200.0, 200.0), 1.0);

        // No face tracks provided.
        let group = ContinuityGroup::new(0.0, 2.0, speaker, Framing::Medium);

        let trajectory = solve_camera_trajectory(&registry, &[group], &default_config());
        assert!(!trajectory.is_empty());

        let sample = trajectory.sample_at(0.5).unwrap();
        // Crop center should follow the speaker bbox (with upward offset).
        assert!((sample.center_x - 500.0).abs() < 10.0);
        // Speaker center y=600, offset by -0.15 * 200 = -30 => 570.
        assert!((sample.center_y - 570.0).abs() < 10.0);
        // Crop height should be based on speaker bbox height.
        assert!((sample.height - 800.0).abs() < 50.0);
    }

    #[test]
    fn face_priority_falls_back_when_face_has_no_sample() {
        let mut registry = EntityRegistry::new();
        let speaker = registry.create(EntityKind::Speaker, "mara".to_string());
        let face = registry.create(EntityKind::Face, "mara_face".to_string());

        registry.add_sample(speaker, 0.0, BoundingBox::new(400.0, 500.0, 200.0, 200.0), 1.0);
        // Face track exists but has no sample at the sampled time.
        registry.add_sample(face, 10.0, BoundingBox::new(420.0, 300.0, 160.0, 200.0), 1.0);

        let group = ContinuityGroup::new(0.0, 2.0, speaker, Framing::Medium)
            .with_face_ids(vec![face]);

        let trajectory = solve_camera_trajectory(&registry, &[group], &default_config());
        assert!(!trajectory.is_empty());

        let sample = trajectory.sample_at(0.5).unwrap();
        // Should fall back to speaker bbox center (with upward offset).
        assert!((sample.center_x - 500.0).abs() < 10.0);
        assert!((sample.center_y - 570.0).abs() < 10.0);
    }

    #[test]
    fn face_priority_uses_union_of_multiple_faces() {
        let mut registry = EntityRegistry::new();
        let speaker = registry.create(EntityKind::Speaker, "mara".to_string());
        let face_a = registry.create(EntityKind::Face, "face_a".to_string());
        let face_b = registry.create(EntityKind::Face, "face_b".to_string());

        registry.add_sample(speaker, 0.0, BoundingBox::new(400.0, 500.0, 200.0, 200.0), 1.0);
        // Two faces on opposite sides of the frame.
        registry.add_sample(face_a, 0.0, BoundingBox::new(200.0, 300.0, 100.0, 100.0), 1.0);
        registry.add_sample(face_b, 0.0, BoundingBox::new(700.0, 300.0, 100.0, 100.0), 1.0);

        let group = ContinuityGroup::new(0.0, 2.0, speaker, Framing::Medium)
            .with_face_ids(vec![face_a, face_b]);

        let trajectory = solve_camera_trajectory(&registry, &[group], &default_config());
        assert!(!trajectory.is_empty());

        let sample = trajectory.sample_at(0.5).unwrap();
        // Union center should be around x=500 (midpoint of 200..800).
        assert!((sample.center_x - 500.0).abs() < 50.0);
        // Union width is 600, height 100; crop height based on union height.
        assert!((sample.height - 400.0).abs() < 50.0);
    }

    #[test]
    fn face_priority_keeps_eyes_in_upper_third() {
        let mut registry = EntityRegistry::new();
        let speaker = registry.create(EntityKind::Speaker, "mara".to_string());
        let face = registry.create(EntityKind::Face, "mara_face".to_string());

        registry.add_sample(speaker, 0.0, BoundingBox::new(400.0, 500.0, 200.0, 400.0), 1.0);
        registry.add_sample(face, 0.0, BoundingBox::new(420.0, 300.0, 160.0, 200.0), 1.0);

        let group = ContinuityGroup::new(0.0, 2.0, speaker, Framing::CloseUp)
            .with_face_ids(vec![face]);

        let trajectory = solve_camera_trajectory(&registry, &[group], &default_config());
        assert!(!trajectory.is_empty());

        let sample = trajectory.sample_at(0.5).unwrap();
        // Crop center should be biased upward (negative offset) so the eyes
        // (top of face bbox) sit in the upper third of the crop.
        let face_center_y = 300.0 + 200.0 * 0.5;
        assert!(
            sample.center_y < face_center_y,
            "crop center should be above face center to keep eyes in upper third"
        );
    }

    #[test]
    fn group_framing_bbox_uses_face_when_track_overlaps_group() {
        let mut registry = EntityRegistry::new();
        let speaker = registry.create(EntityKind::Speaker, "mara".to_string());
        let face = registry.create(EntityKind::Face, "mara_face".to_string());

        // Speaker body in the lower half of the frame.
        registry.add_sample(speaker, 0.0, BoundingBox::new(400.0, 600.0, 200.0, 400.0), 1.0);
        // Face in the upper half, offset from the body center.
        registry.add_sample(face, 0.0, BoundingBox::new(420.0, 300.0, 160.0, 200.0), 1.0);

        let group = ContinuityGroup::new(0.0, 2.0, speaker, Framing::Medium)
            .with_face_ids(vec![face]);

        // Face track overlaps the continuity group -> use face bbox.
        let bbox = group_framing_bbox(&registry, &group, 0.5).unwrap();
        assert!((bbox.center().1 - 400.0).abs() < 10.0, "should use face bbox center");
    }

    #[test]
    fn crop_rect_clamps_to_video_bounds() {
        let mut registry = EntityRegistry::new();
        let speaker = registry.create(EntityKind::Speaker, "mara".to_string());
        registry.add_sample(speaker, 0.0, BoundingBox::new(50.0, 50.0, 100.0, 100.0), 1.0);

        let groups = vec![ContinuityGroup::new(
            0.0,
            2.0,
            speaker,
            Framing::Medium,
        )];

        let camera = ActiveSpeakerCamera::from_bytes(
            vec![],
            RenderMode::Preview,
            &registry,
            groups,
            default_config(),
        );

        let sample = CameraSample {
            time: 0.0,
            center_x: 50.0,
            center_y: 50.0,
            height: 400.0,
        };
        let output_rect = Rect::from_wh(1080.0, 1920.0);
        let crop = camera.compute_crop_rect(&sample, output_rect, 1000.0, 1000.0).unwrap();

        // Crop should be clamped to video bounds.
        assert!(crop.left >= 0.0);
        assert!(crop.top >= 0.0);
        assert!(crop.right <= 1000.0);
        assert!(crop.bottom <= 1000.0);
    }
}
