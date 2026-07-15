//! # Semantic Manifest Module
//!
//! High-level semantic description of a shot or scene, used by smart nodes
//! such as `SmartCaptionNode` to make content-aware layout decisions.
//!
//! ## Responsibilities
//! - **Entity References**: Track which semantic entities (faces, speakers,
//!   subjects, objects) are relevant to a shot.
//! - **Shot Boundaries**: Define the time range the manifest applies to.
//! - **Temporal Solver**: Given entity tracks and caption bounds, compute a
//!   smooth caption trajectory that avoids faces and stays in safe areas.
//!
//! ## Key Types
//! - `SemanticManifest`: References to entity tracks + solver configuration.
//! - `CaptionTrajectory`: Sampled (x, y) anchor positions over time.
//! - `solve_caption_trajectory`: The temporal constraint solver.

use crate::entity::{BoundingBox, EntityId, EntityRegistry};
use serde::{Deserialize, Serialize};

/// A semantic manifest for a shot or scene.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SemanticManifest {
    /// Entity IDs this manifest cares about (faces, speakers, subjects).
    pub entity_refs: Vec<EntityId>,
    /// Time range in seconds the manifest covers.
    pub start_time: f64,
    pub end_time: f64,
    /// Solver configuration.
    pub solver: SolverConfig,
}

/// Configuration for the caption placement solver.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolverConfig {
    /// Minimum clearance between caption and face bbox in pixels.
    pub clearance: f32,

    /// Smoothing window size (number of samples).
    pub smoothing_window: usize,
    /// Penalty for moving the caption (higher = smoother, less reactive).
    pub motion_penalty: f32,
    /// Penalty for caption overlapping a face/subject bbox.
    pub overlap_penalty: f32,
    /// Penalty for caption leaving the safe zone.
    pub safe_zone_penalty: f32,

    // ---- Continuous solver options ----
    /// Number of grid steps per axis for the continuous temporal solver.
    /// The total state space is `grid_steps^2`, so higher values give finer
    /// position resolution at the cost of runtime.
    pub grid_steps: usize,
    /// Weight for preferring negative space (areas far from tracked entities).
    pub negative_space_weight: f32,
    /// Weight for temporal smoothness (penalizes velocity between samples).
    pub smoothness_weight: f32,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            clearance: 40.0,
            smoothing_window: 5,
            motion_penalty: 1.0,
            overlap_penalty: 1000.0,
            safe_zone_penalty: 500.0,
            grid_steps: 16,
            negative_space_weight: 10.0,
            smoothness_weight: 0.5,
        }
    }
}

/// Static parameters shared by the caption placement cost functions.
///
/// Grouping these values avoids the long positional argument lists in
/// `candidate_cost` and `state_cost` and makes the call sites read like
/// `cost(x, y, faces, &config)`.
struct CaptionCostConfig<'a> {
    caption_w: f32,
    caption_h: f32,
    canvas_w: f32,
    canvas_h: f32,
    safe_top: f32,
    safe_bottom: f32,
    safe_left: f32,
    safe_right: f32,
    solver: &'a SolverConfig,
}

/// Parameters that control how the caption trajectory solver places a caption.
///
/// This struct replaces the long positional argument list of
/// `solve_caption_trajectory` and `solve_caption_trajectory_continuous` with
/// a single named bundle. It is intentionally separate from `EntityRegistry`
/// and `SemanticManifest`, which are the data sources the solver operates on.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolverParams {
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
}

impl Default for SolverParams {
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
        }
    }
}



/// A sampled 2D position at a point in time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TrajectorySample {
    pub time: f64,
    pub x: f32,
    pub y: f32,
    /// The discrete anchor chosen by the solver, if any. The continuous solver
    /// does not use anchors, so this field is `None` for continuous trajectories.
    ///
    /// When smoothing averages `x`/`y` over a window, the anchor is kept from
    /// the center sample of that window. It therefore reflects the solver's
    /// original discrete decision, not a recomputation from the smoothed
    /// coordinates.
    pub anchor: Option<Anchor>,
}

/// A caption trajectory: a list of sampled anchor positions over time.
///
/// Samples are always stored sorted by time. Use [`CaptionTrajectory::new`]
/// to construct one; the constructor sorts the input for you.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CaptionTrajectory {
    samples: Vec<TrajectorySample>,
}

impl CaptionTrajectory {
    /// Create a new trajectory, sorting samples by time so `sample_at` can
    /// assume a monotonic timeline.
    pub fn new(mut samples: Vec<TrajectorySample>) -> Self {
        samples.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Self { samples }
    }

    /// Number of samples in the trajectory.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether the trajectory contains no samples.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }    /// Access the sorted samples as a slice (useful for tests and debug output).
    pub fn as_slice(&self) -> &[TrajectorySample] {
        &self.samples
    }

    /// Sample the trajectory at a given time using linear interpolation.
    pub fn sample_at(&self, time: f64) -> Option<(f32, f32)> {
        self.sample_at_with_anchor(time).map(|(x, y, _)| (x, y))
    }

    /// Sample the trajectory at a given time, returning the interpolated
    /// position and the anchor chosen by the discrete solver (if any).
    ///
    /// The anchor is taken from the previous surrounding sample, so the
    /// solver's discrete decision is preserved exactly rather than blended.
    /// For trajectories produced by the continuous solver, the anchor is
    /// always `None`.
    pub fn sample_at_with_anchor(&self, time: f64) -> Option<(f32, f32, Option<Anchor>)> {
        let first = self.samples.first()?;
        let last = self.samples.last()?;
        if time <= first.time {
            return Some((first.x, first.y, first.anchor));
        }
        if time >= last.time {
            return Some((last.x, last.y, last.anchor));
        }
        let idx = self.samples.partition_point(|s| s.time < time);
        let prev = &self.samples[idx - 1];
        let next = &self.samples[idx];
        let t = if next.time > prev.time {
            ((time - prev.time) / (next.time - prev.time)) as f32
        } else {
            0.0
        };
        let x = prev.x + (next.x - prev.x) * t;
        let y = prev.y + (next.y - prev.y) * t;
        // Anchor is taken from the previous sample. This preserves the
        // solver's discrete decision rather than blending between anchors.
        Some((x, y, prev.anchor))
    }
}

/// Candidate anchor for the caption.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Anchor {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
    Left,
    Right,
}

/// Solve a smooth caption trajectory that avoids faces/subjects.
///
/// # Arguments
/// * `registry` - Entity registry containing face/subject tracks.
/// * `manifest` - Semantic manifest with entity refs and solver config.
/// * `params` - Placement parameters (canvas size, caption size, safe margins,
///   sample rate).
pub fn solve_caption_trajectory(
    registry: &EntityRegistry,
    manifest: &SemanticManifest,
    params: &SolverParams,
) -> CaptionTrajectory {
    let duration = (manifest.end_time - manifest.start_time).max(0.0);
    if duration <= 0.0 || params.sample_rate <= 0.0 {
        return CaptionTrajectory::default();
    }

    let num_samples = (duration * params.sample_rate).ceil().max(1.0) as usize + 1;
    let center_x = params.canvas_w / 2.0;
    let center_y = params.canvas_h / 2.0;

    // Candidate anchors: top/bottom with left/center/right horizontal
    // variants, plus side anchors. Center variants are listed first so the
    // solver prefers them when all else is equal, keeping output stable. When
    // both left and right are clear, left is preferred simply because it
    // appears first; this is an arbitrary but deterministic tie-break.
    let anchors = [
        Anchor::TopCenter,
        Anchor::TopLeft,
        Anchor::TopRight,
        Anchor::BottomCenter,
        Anchor::BottomLeft,
        Anchor::BottomRight,
        Anchor::Left,
        Anchor::Right,
    ];

    // Precompute face bboxes per sample.
    let per_sample_faces = sample_faces_per_time(registry, manifest, num_samples, params.sample_rate);

    // Static cost configuration shared across all samples.
    let cost_config = CaptionCostConfig {
        caption_w: params.caption_w,
        caption_h: params.caption_h,
        canvas_w: params.canvas_w,
        canvas_h: params.canvas_h,
        safe_top: params.safe_top,
        safe_bottom: params.safe_bottom,
        safe_left: params.safe_left,
        safe_right: params.safe_right,
        solver: &manifest.solver,
    };

    // Compute candidate positions for each sample.
    let mut candidate_positions: Vec<Vec<(Anchor, f32, f32)>> = Vec::with_capacity(num_samples);
    for _ in 0..num_samples {
        let mut positions = Vec::new();
        for anchor in &anchors {
            let (x, y) = match anchor {
                Anchor::TopLeft => (params.safe_left + params.caption_w / 2.0, params.safe_top + params.caption_h / 2.0),
                Anchor::TopCenter => (center_x, params.safe_top + params.caption_h / 2.0),
                Anchor::TopRight => (params.canvas_w - params.safe_right - params.caption_w / 2.0, params.safe_top + params.caption_h / 2.0),
                Anchor::BottomLeft => (params.safe_left + params.caption_w / 2.0, params.canvas_h - params.safe_bottom - params.caption_h / 2.0),
                Anchor::BottomCenter => (center_x, params.canvas_h - params.safe_bottom - params.caption_h / 2.0),
                Anchor::BottomRight => (params.canvas_w - params.safe_right - params.caption_w / 2.0, params.canvas_h - params.safe_bottom - params.caption_h / 2.0),
                Anchor::Left => (params.safe_left + params.caption_w / 2.0, center_y),
                Anchor::Right => (params.canvas_w - params.safe_right - params.caption_w / 2.0, center_y),
            };
            positions.push((*anchor, x, y));
        }
        candidate_positions.push(positions);
    }

    // Dynamic programming over time.
    // dp[i][a] = minimum cost to reach sample i with anchor a.
    // We use a simple Viterbi-like DP.
    let mut dp: Vec<Vec<f32>> = Vec::with_capacity(num_samples);
    let mut prev: Vec<Vec<Option<usize>>> = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let faces = &per_sample_faces[i];
        let mut sample_dp = Vec::with_capacity(anchors.len());
        let mut sample_prev = Vec::with_capacity(anchors.len());

        for (_anchor, x, y) in &candidate_positions[i] {
            let cost = candidate_cost(*x, *y, faces, &cost_config);

            if i == 0 {
                sample_dp.push(cost);
                sample_prev.push(None);
            } else {
                let mut best_cost = f32::INFINITY;
                let mut best_prev: Option<usize> = None;
                for prev_a in 0..anchors.len() {
                    let prev_pos = &candidate_positions[i - 1][prev_a];
                    let motion = ((*x - prev_pos.1).powi(2) + (*y - prev_pos.2).powi(2)).sqrt();
                    let total = dp[i - 1][prev_a] + cost + motion * manifest.solver.motion_penalty;
                    if total < best_cost {
                        best_cost = total;
                        best_prev = Some(prev_a);
                    }
                }
                sample_dp.push(best_cost);
                sample_prev.push(best_prev);
            }
        }

        dp.push(sample_dp);
        prev.push(sample_prev);
    }

    // Backtrack to find the best anchor sequence.
    // Normalize any NaN costs to infinity so comparisons never panic.
    for row in &mut dp {
        for cost in row.iter_mut() {
            if cost.is_nan() {
                *cost = f32::INFINITY;
            }
        }
    }
    let mut best_sequence: Vec<usize> = Vec::with_capacity(num_samples);
    if num_samples > 0 {
        let last_idx = (0..anchors.len())
            .min_by(|a, b| dp[num_samples - 1][*a].total_cmp(&dp[num_samples - 1][*b]))
            .unwrap_or(0);
        best_sequence.push(last_idx);
        for i in (1..num_samples).rev() {
            let p = prev[i][best_sequence[best_sequence.len() - 1]];
            if let Some(p_idx) = p {
                best_sequence.push(p_idx);
            } else {
                best_sequence.push(0);
            }
        }
        best_sequence.reverse();
    }

    // Build trajectory from the best sequence.
    let mut samples = Vec::with_capacity(num_samples);
    for (i, pos) in candidate_positions.iter().enumerate().take(num_samples) {
        let anchor_idx = best_sequence.get(i).copied().unwrap_or(0);
        let pos = &pos[anchor_idx];
        samples.push(TrajectorySample {
            time: manifest.start_time + i as f64 / params.sample_rate,
            x: pos.1,
            y: pos.2,
            anchor: Some(pos.0),
        });
    }

    // Apply smoothing.
    samples = smooth_trajectory(samples, manifest.solver.smoothing_window);

    CaptionTrajectory::new(samples)
}

fn candidate_cost(x: f32, y: f32, faces: &[BoundingBox], config: &CaptionCostConfig<'_>) -> f32 {
    let half_w = config.caption_w / 2.0;
    let half_h = config.caption_h / 2.0;
    let caption_bbox = BoundingBox::new(
        (x - half_w).max(0.0),
        (y - half_h).max(0.0),
        config.caption_w.min(config.canvas_w),
        config.caption_h.min(config.canvas_h),
    );

    let mut cost = 0.0f32;

    // Overlap penalty with faces.
    for face in faces {
        let overlap_area = intersection_area(&caption_bbox, face);
        if overlap_area > 0.0 {
            cost += overlap_area * config.solver.overlap_penalty;
        }
        // Also penalize being too close even if not overlapping.
        let clearance_cost = expanded_overlap_area(&caption_bbox, face, config.solver.clearance);
        cost += clearance_cost * config.solver.overlap_penalty * 0.1;
    }

    // Safe zone penalty.
    if caption_bbox.y < config.safe_top {
        cost += (config.safe_top - caption_bbox.y) * config.solver.safe_zone_penalty;
    }
    if caption_bbox.y + caption_bbox.height > config.canvas_h - config.safe_bottom {
        cost += (caption_bbox.y + caption_bbox.height - (config.canvas_h - config.safe_bottom))
            * config.solver.safe_zone_penalty;
    }
    if caption_bbox.x < config.safe_left {
        cost += (config.safe_left - caption_bbox.x) * config.solver.safe_zone_penalty;
    }
    if caption_bbox.x + caption_bbox.width > config.canvas_w - config.safe_right {
        cost += (caption_bbox.x + caption_bbox.width - (config.canvas_w - config.safe_right))
            * config.solver.safe_zone_penalty;
    }

    cost
}

pub(crate) fn intersection_area(a: &BoundingBox, b: &BoundingBox) -> f32 {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    if right > left && bottom > top {
        (right - left) * (bottom - top)
    } else {
        0.0
    }
}

/// Compute the overlap area between `caption` and a face bbox expanded by `clearance`.
///
/// This is used to penalize captions that are too close to a face even when they
/// do not strictly overlap.
fn expanded_overlap_area(caption: &BoundingBox, face: &BoundingBox, clearance: f32) -> f32 {
    let expanded_face = BoundingBox::new(
        face.x - clearance,
        face.y - clearance,
        face.width + clearance * 2.0,
        face.height + clearance * 2.0,
    );
    intersection_area(caption, &expanded_face)
}

/// Sample all tracked face/subject bounding boxes at each time step.
fn sample_faces_per_time(
    registry: &EntityRegistry,
    manifest: &SemanticManifest,
    num_samples: usize,
    sample_rate: f64,
) -> Vec<Vec<BoundingBox>> {
    let mut per_sample_faces: Vec<Vec<BoundingBox>> = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let t = manifest.start_time + i as f64 / sample_rate;
        let mut faces = Vec::new();
        for id in &manifest.entity_refs {
            if let Some(sample) = registry.sample_at(*id, t) {
                faces.push(sample.bbox);
            }
        }
        per_sample_faces.push(faces);
    }
    per_sample_faces
}

fn smooth_trajectory(samples: Vec<TrajectorySample>, window: usize) -> Vec<TrajectorySample> {
    if window <= 1 || samples.len() < 2 {
        return samples;
    }
    let mut smoothed = Vec::with_capacity(samples.len());
    for i in 0..samples.len() {
        let start = i.saturating_sub(window / 2);
        let end = (i + window / 2 + 1).min(samples.len());
        let mut sum_x = 0.0f32;
        let mut sum_y = 0.0f32;
        let mut count = 0usize;
        for sample in samples.iter().take(end).skip(start) {
            sum_x += sample.x;
            sum_y += sample.y;
            count += 1;
        }
        smoothed.push(TrajectorySample {
            time: samples[i].time,
            x: sum_x / count as f32,
            y: sum_y / count as f32,
            anchor: samples[i].anchor,
        });
    }
    smoothed
}

// ---------------------------------------------------------------------------
// Continuous temporal constraint solver
// ---------------------------------------------------------------------------

/// Solve a smooth, continuous caption trajectory over an entire shot using a
/// discretized dynamic-programming (Viterbi) approach.
///
/// Unlike [`solve_caption_trajectory`], which only chooses between discrete
/// anchors, this solver optimizes a continuous 2D position at each time sample.
/// It balances:
///
/// * **Face avoidance**: high penalty for overlapping tracked faces/subjects.
/// * **Safe zones**: hard boundaries from `safe_top`/`safe_bottom`.
/// * **Negative space**: soft preference for areas far from tracked entities.
/// * **Temporal smoothness**: penalizes large frame-to-frame velocity.
///
/// The state space is a 2D grid with `solver.grid_steps` positions per axis.
/// Runtime is `O(T * N^2)` where `T` is the number of samples and `N` is the
/// number of grid steps per axis. For typical short-form video this is well
/// under a millisecond.
pub fn solve_caption_trajectory_continuous(
    registry: &EntityRegistry,
    manifest: &SemanticManifest,
    params: &SolverParams,
) -> CaptionTrajectory {
    let duration = (manifest.end_time - manifest.start_time).max(0.0);
    if duration <= 0.0 || params.sample_rate <= 0.0 {
        return CaptionTrajectory::default();
    }

    let num_samples = (duration * params.sample_rate).ceil().max(1.0) as usize + 1;
    let grid_steps = manifest.solver.grid_steps.max(2);

    // 2D ranges for the caption center.
    let min_y = params.safe_top + params.caption_h / 2.0;
    let max_y = params.canvas_h - params.safe_bottom - params.caption_h / 2.0;
    let min_x = params.safe_left + params.caption_w / 2.0;
    let max_x = params.canvas_w - params.safe_right - params.caption_w / 2.0;
    if max_y <= min_y || max_x <= min_x {
        return CaptionTrajectory::default();
    }

    // Build evenly spaced candidate (x, y) positions.
    let mut positions: Vec<(f32, f32)> = Vec::with_capacity(grid_steps * grid_steps);
    for iy in 0..grid_steps {
        let ty = iy as f32 / (grid_steps.saturating_sub(1)).max(1) as f32;
        let y = min_y + (max_y - min_y) * ty;
        for ix in 0..grid_steps {
            let tx = ix as f32 / (grid_steps.saturating_sub(1)).max(1) as f32;
            let x = min_x + (max_x - min_x) * tx;
            positions.push((x, y));
        }
    }
    let num_states = positions.len();

    // Precompute face bboxes per sample.
    let per_sample_faces = sample_faces_per_time(registry, manifest, num_samples, params.sample_rate);

    // Static cost configuration shared across all samples.
    let cost_config = CaptionCostConfig {
        caption_w: params.caption_w,
        caption_h: params.caption_h,
        canvas_w: params.canvas_w,
        canvas_h: params.canvas_h,
        safe_top: params.safe_top,
        safe_bottom: params.safe_bottom,
        safe_left: params.safe_left,
        safe_right: params.safe_right,
        solver: &manifest.solver,
    };

    let mut state_costs: Vec<Vec<f32>> = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let faces = &per_sample_faces[i];
        let mut costs = Vec::with_capacity(num_states);
        for &(x, y) in &positions {
            costs.push(state_cost(x, y, faces, &cost_config));
        }
        state_costs.push(costs);
    }

    // Viterbi DP.
    // dp[i][j] = minimum cost to reach sample i in state j.
    let mut dp: Vec<Vec<f32>> = Vec::with_capacity(num_samples);
    let mut prev: Vec<Vec<Option<usize>>> = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let mut sample_dp = Vec::with_capacity(num_states);
        let mut sample_prev = Vec::with_capacity(num_states);

        for j in 0..num_states {
            let cost = state_costs[i][j];

            if i == 0 {
                sample_dp.push(cost);
                sample_prev.push(None);
                continue;
            }

            let mut best_cost = f32::INFINITY;
            let mut best_prev: Option<usize> = None;
            for prev_j in 0..num_states {
                let (prev_x, prev_y) = positions[prev_j];
                let (x, y) = positions[j];
                let dx = x - prev_x;
                let dy = y - prev_y;
                let velocity = (dx * dx + dy * dy).sqrt();
                let motion_cost = velocity * manifest.solver.smoothness_weight;
                let total = dp[i - 1][prev_j] + cost + motion_cost;
                if total < best_cost {
                    best_cost = total;
                    best_prev = Some(prev_j);
                }
            }
            sample_dp.push(best_cost);
            sample_prev.push(best_prev);
        }

        dp.push(sample_dp);
        prev.push(sample_prev);
    }

    // Backtrack to find the optimal state sequence.
    let mut best_sequence: Vec<usize> = Vec::with_capacity(num_samples);
    if num_samples > 0 {
        let last_idx = (0..num_states)
            .min_by(|a, b| dp[num_samples - 1][*a].total_cmp(&dp[num_samples - 1][*b]))
            .unwrap_or(0);
        best_sequence.push(last_idx);
        for i in (1..num_samples).rev() {
            let p = prev[i][best_sequence[best_sequence.len() - 1]];
            if let Some(p_idx) = p {
                best_sequence.push(p_idx);
            } else {
                best_sequence.push(0);
            }
        }
        best_sequence.reverse();
    }

    // Build trajectory from the best sequence.
    let mut samples = Vec::with_capacity(num_samples);
    for i in 0..num_samples {
        let state_idx = best_sequence.get(i).copied().unwrap_or(0);
        let (x, y) = positions[state_idx];
        samples.push(TrajectorySample {
            time: manifest.start_time + i as f64 / params.sample_rate,
            x,
            y,
            anchor: None,
        });
    }

    // Apply smoothing.
    samples = smooth_trajectory(samples, manifest.solver.smoothing_window);

    CaptionTrajectory::new(samples)
}

fn state_cost(x: f32, y: f32, faces: &[BoundingBox], config: &CaptionCostConfig<'_>) -> f32 {
    let half_w = config.caption_w / 2.0;
    let half_h = config.caption_h / 2.0;
    let caption_bbox = BoundingBox::new(
        (x - half_w).max(0.0),
        (y - half_h).max(0.0),
        config.caption_w.min(config.canvas_w),
        config.caption_h.min(config.canvas_h),
    );

    let mut cost = 0.0f32;

    // Overlap and clearance penalties with faces.
    let mut nearest_face_distance = f32::INFINITY;
    for face in faces {
        let overlap_area = intersection_area(&caption_bbox, face);
        if overlap_area > 0.0 {
            cost += overlap_area * config.solver.overlap_penalty;
        }
        let clearance_cost = expanded_overlap_area(&caption_bbox, face, config.solver.clearance);
        cost += clearance_cost * config.solver.overlap_penalty * 0.1;

        // Track distance to nearest face center for negative-space preference.
        let (fx, fy) = face.center();
        let dx = x - fx;
        let dy = y - fy;
        let distance = (dx * dx + dy * dy).sqrt();
        if distance < nearest_face_distance {
            nearest_face_distance = distance;
        }
    }

    // Safe zone penalty.
    if caption_bbox.y < config.safe_top {
        cost += (config.safe_top - caption_bbox.y) * config.solver.safe_zone_penalty;
    }
    if caption_bbox.y + caption_bbox.height > config.canvas_h - config.safe_bottom {
        cost += (caption_bbox.y + caption_bbox.height - (config.canvas_h - config.safe_bottom))
            * config.solver.safe_zone_penalty;
    }
    if caption_bbox.x < config.safe_left {
        cost += (config.safe_left - caption_bbox.x) * config.solver.safe_zone_penalty;
    }
    if caption_bbox.x + caption_bbox.width > config.canvas_w - config.safe_right {
        cost += (caption_bbox.x + caption_bbox.width - (config.canvas_w - config.safe_right))
            * config.solver.safe_zone_penalty;
    }

    // Negative-space preference: prefer positions far from faces.
    if !faces.is_empty() && nearest_face_distance.is_finite() {
        // Inverse distance: closer to faces = higher cost. Clamp the maximum
        // contribution so a single nearby face cannot dominate the total cost.
        let negative_space_cost = config.solver.negative_space_weight / nearest_face_distance.max(1.0);
        cost += negative_space_cost.min(config.solver.negative_space_weight);
    }

    cost
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{EntityKind, EntityRegistry};

    #[test]
    fn trajectory_avoids_face_at_bottom() {
        let mut registry = EntityRegistry::new();
        let face = registry.create(EntityKind::Face, "mara".to_string());
        // Face at bottom center.
        registry.add_sample(face, 0.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);
        registry.add_sample(face, 2.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);

        let manifest = SemanticManifest {
            entity_refs: vec![face],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };

        let params = SolverParams {
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            safe_top: 60.0,
            safe_bottom: 110.0,
            safe_left: 60.0,
            safe_right: 60.0,
            sample_rate: 10.0,
        };
        let trajectory = solve_caption_trajectory(&registry, &manifest, &params);

        assert!(!trajectory.as_slice().is_empty());
        for sample in trajectory.as_slice() {
            // Caption should be placed at the top to avoid the face at the bottom.
            assert!(sample.y < 960.0, "caption should avoid bottom face");
        }
    }

    #[test]
    fn trajectory_sample_at_interpolates() {
        let trajectory = CaptionTrajectory::new(vec![
            TrajectorySample { time: 0.0, x: 0.0, y: 0.0, anchor: None },
            TrajectorySample { time: 1.0, x: 100.0, y: 200.0, anchor: None },
        ]);
        let (x, y) = trajectory.sample_at(0.5).unwrap();
        assert!((x - 50.0).abs() < f32::EPSILON);
        assert!((y - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn trajectory_sample_at_clamps_out_of_range() {
        let trajectory = CaptionTrajectory::new(vec![
            TrajectorySample { time: 0.0, x: 0.0, y: 0.0, anchor: None },
            TrajectorySample { time: 1.0, x: 100.0, y: 200.0, anchor: None },
        ]);
        let (x, y) = trajectory.sample_at(-1.0).unwrap();
        assert!((x - 0.0).abs() < f32::EPSILON);
        assert!((y - 0.0).abs() < f32::EPSILON);
        let (x, y) = trajectory.sample_at(2.0).unwrap();
        assert!((x - 100.0).abs() < f32::EPSILON);
        assert!((y - 200.0).abs() < f32::EPSILON);
    }

    #[test]
    fn trajectory_constructor_sorts_unsorted_samples() {
        let trajectory = CaptionTrajectory::new(vec![
            TrajectorySample { time: 2.0, x: 200.0, y: 200.0, anchor: None },
            TrajectorySample { time: 0.0, x: 0.0, y: 0.0, anchor: None },
            TrajectorySample { time: 1.0, x: 100.0, y: 100.0, anchor: None },
        ]);

        assert_eq!(trajectory.as_slice()[0].time, 0.0);
        assert_eq!(trajectory.as_slice()[1].time, 1.0);
        assert_eq!(trajectory.as_slice()[2].time, 2.0);

        let (x, y) = trajectory.sample_at(0.5).unwrap();
        assert!((x - 50.0).abs() < f32::EPSILON);
        assert!((y - 50.0).abs() < f32::EPSILON);
    }

    #[test]
    fn trajectory_constructor_handles_nan_time_gracefully() {
        // NaN times are not expected, but the constructor should not panic and
        // should still allow sampling to return a value.
        let trajectory = CaptionTrajectory::new(vec![
            TrajectorySample { time: 0.0, x: 0.0, y: 0.0, anchor: None },
            TrajectorySample { time: f64::NAN, x: 50.0, y: 50.0, anchor: None },
            TrajectorySample { time: 1.0, x: 100.0, y: 100.0, anchor: None },
        ]);

        // The constructor sorts; NaN comparisons return None, so the NaN
        // sample's position is implementation-defined but sampling must not
        // panic and should return a finite value.
        assert_eq!(trajectory.len(), 3, "all samples are preserved");
        let (x, y) = trajectory.sample_at(0.5).expect("sampling must succeed");
        assert!(x.is_finite());
        assert!(y.is_finite());

        // Non-NaN samples must remain ordered around the NaN sample.
        let non_nan: Vec<_> = trajectory
            .as_slice()
            .iter()
            .filter(|s| !s.time.is_nan())
            .collect();
        assert_eq!(non_nan.len(), 2);
        assert!(non_nan[0].time <= non_nan[1].time);
    }

    #[test]
    fn empty_trajectory_returns_none() {
        let trajectory = CaptionTrajectory::default();
        assert!(trajectory.is_empty());
        assert!(trajectory.sample_at(0.5).is_none());
    }

    #[test]
    fn solver_does_not_panic_with_nan_costs() {
        // A zero-area caption bbox can produce NaN/inf costs. The solver should
        // normalize them and return a trajectory rather than panicking.
        let mut registry = EntityRegistry::new();
        let face = registry.create(EntityKind::Face, "mara".to_string());
        registry.add_sample(face, 0.0, BoundingBox::new(0.0, 0.0, 0.0, 0.0), 1.0);
        registry.add_sample(face, 2.0, BoundingBox::new(0.0, 0.0, 0.0, 0.0), 1.0);

        let manifest = SemanticManifest {
            entity_refs: vec![face],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };

        let params = SolverParams {
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            safe_top: 60.0,
            safe_bottom: 110.0,
            safe_left: 60.0,
            safe_right: 60.0,
            sample_rate: 10.0,
        };
        let trajectory = solve_caption_trajectory(&registry, &manifest, &params);

        assert!(!trajectory.as_slice().is_empty());
        for sample in trajectory.as_slice() {
            assert!(!sample.x.is_nan());
            assert!(!sample.y.is_nan());
        }
    }

    #[test]
    fn trajectory_is_smooth() {
        let mut registry = EntityRegistry::new();
        let face = registry.create(EntityKind::Face, "mara".to_string());
        // Face moves from bottom to top over time.
        registry.add_sample(face, 0.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);
        registry.add_sample(face, 2.0, BoundingBox::new(400.0, 100.0, 200.0, 200.0), 1.0);

        let manifest = SemanticManifest {
            entity_refs: vec![face],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };

        let params = SolverParams {
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            safe_top: 60.0,
            safe_bottom: 110.0,
            safe_left: 60.0,
            safe_right: 60.0,
            sample_rate: 10.0,
        };
        let trajectory = solve_caption_trajectory(&registry, &manifest, &params);

        assert!(trajectory.as_slice().len() >= 3);
        let mut max_jump = 0.0f32;
        for window in trajectory.as_slice().windows(2) {
            let dy = (window[1].y - window[0].y).abs();
            if dy > max_jump {
                max_jump = dy;
            }
        }
        // With smoothing, jumps should be modest.
        assert!(max_jump < 500.0, "trajectory should be smooth, max jump: {}", max_jump);
    }

    #[test]
    fn continuous_solver_avoids_face_and_stays_smooth() {
        let mut registry = EntityRegistry::new();
        let face = registry.create(EntityKind::Face, "mara".to_string());
        // Face at bottom center.
        registry.add_sample(face, 0.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);
        registry.add_sample(face, 2.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);

        let manifest = SemanticManifest {
            entity_refs: vec![face],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };

        let params = SolverParams {
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            safe_top: 60.0,
            safe_bottom: 110.0,
            safe_left: 60.0,
            safe_right: 60.0,
            sample_rate: 10.0,
        };
        let trajectory = solve_caption_trajectory_continuous(&registry, &manifest, &params);

        assert!(!trajectory.as_slice().is_empty());
        for sample in trajectory.as_slice() {
            // Caption should be placed above the bottom face.
            assert!(sample.y < 1500.0, "caption should avoid bottom face, got y={}", sample.y);
        }

        // Trajectory should be smooth.
        let mut max_jump = 0.0f32;
        for window in trajectory.as_slice().windows(2) {
            let dy = (window[1].y - window[0].y).abs();
            if dy > max_jump {
                max_jump = dy;
            }
        }
        assert!(max_jump < 300.0, "continuous trajectory should be smooth, max jump: {}", max_jump);
    }

    #[test]
    fn trajectory_chooses_side_anchor_when_top_center_is_blocked() {
        // A face at the top-center blocks the centered top anchor, but
        // leaves room on the left (and right). The solver should dodge
        // horizontally instead of being forced to the bottom.
        let face_x = 300.0f32;
        let face_w = 200.0f32;
        let caption_w = 400.0f32;

        let mut registry = EntityRegistry::new();
        let face = registry.create(EntityKind::Face, "top_center".to_string());
        registry.add_sample(face, 0.0, BoundingBox::new(face_x, 60.0, face_w, 300.0), 1.0);
        registry.add_sample(face, 2.0, BoundingBox::new(face_x, 60.0, face_w, 300.0), 1.0);

        let manifest = SemanticManifest {
            entity_refs: vec![face],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };

        // Use a narrower caption so left/right variants do not overlap the face.
        let params = SolverParams {
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w,
            caption_h: 100.0,
            safe_top: 60.0,
            safe_bottom: 110.0,
            safe_left: 60.0,
            safe_right: 60.0,
            sample_rate: 10.0,
        };
        let trajectory = solve_caption_trajectory(&registry, &manifest, &params);

        assert!(!trajectory.as_slice().is_empty());
        for sample in trajectory.as_slice() {
            // Should stay at the top, not drop to the bottom.
            assert!(sample.y < 960.0, "caption should stay at top, got y={}", sample.y);
            // Should dodge to one side of the face (clear of the face x-range).
            // A caption centered over the face would overlap, so the solved x
            // must place the caption bbox outside the face's x-range.
            let caption_half_w = caption_w / 2.0;
            let caption_left = sample.x - caption_half_w;
            let caption_right = sample.x + caption_half_w;
            let face_right = face_x + face_w;
            let overlaps_face = caption_left < face_right && caption_right > face_x;
            assert!(
                !overlaps_face,
                "caption should dodge horizontally away from face, got x={}",
                sample.x
            );
        }
    }

    #[test]
    fn trajectory_chooses_right_when_top_bottom_and_left_are_blocked() {
        let mut registry = EntityRegistry::new();
        let face_top = registry.create(EntityKind::Face, "top".to_string());
        let face_bottom = registry.create(EntityKind::Face, "bottom".to_string());
        let face_left = registry.create(EntityKind::Face, "left".to_string());
        // Block top, bottom, and left, leaving right as the only open side.
        registry.add_sample(face_top, 0.0, BoundingBox::new(400.0, 60.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_top, 2.0, BoundingBox::new(400.0, 60.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_bottom, 0.0, BoundingBox::new(400.0, 1660.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_bottom, 2.0, BoundingBox::new(400.0, 1660.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_left, 0.0, BoundingBox::new(60.0, 800.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_left, 2.0, BoundingBox::new(60.0, 800.0, 200.0, 200.0), 1.0);

        let manifest = SemanticManifest {
            entity_refs: vec![face_top, face_bottom, face_left],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };

        let params = SolverParams {
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            safe_top: 60.0,
            safe_bottom: 110.0,
            safe_left: 60.0,
            safe_right: 60.0,
            sample_rate: 10.0,
        };
        let trajectory = solve_caption_trajectory(&registry, &manifest, &params);

        assert!(!trajectory.as_slice().is_empty());
        for sample in trajectory.as_slice() {
            assert!(sample.x > 540.0, "caption should use right anchor, got x={}", sample.x);
        }
    }

    #[test]
    fn trajectory_chooses_left_when_top_bottom_and_right_are_blocked() {
        let mut registry = EntityRegistry::new();
        let face_top = registry.create(EntityKind::Face, "top".to_string());
        let face_bottom = registry.create(EntityKind::Face, "bottom".to_string());
        let face_right = registry.create(EntityKind::Face, "right".to_string());
        // Block top, bottom, and right, leaving left as the only open side.
        registry.add_sample(face_top, 0.0, BoundingBox::new(400.0, 60.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_top, 2.0, BoundingBox::new(400.0, 60.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_bottom, 0.0, BoundingBox::new(400.0, 1660.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_bottom, 2.0, BoundingBox::new(400.0, 1660.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_right, 0.0, BoundingBox::new(820.0, 800.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_right, 2.0, BoundingBox::new(820.0, 800.0, 200.0, 200.0), 1.0);

        let manifest = SemanticManifest {
            entity_refs: vec![face_top, face_bottom, face_right],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };

        let params = SolverParams {
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            safe_top: 60.0,
            safe_bottom: 110.0,
            safe_left: 60.0,
            safe_right: 60.0,
            sample_rate: 10.0,
        };
        let trajectory = solve_caption_trajectory(&registry, &manifest, &params);

        assert!(!trajectory.as_slice().is_empty());
        for sample in trajectory.as_slice() {
            assert!(sample.x < 540.0, "caption should use left anchor, got x={}", sample.x);
        }
    }

    #[test]
    fn continuous_solver_optimizes_x_and_y() {
        let mut registry = EntityRegistry::new();
        let face = registry.create(EntityKind::Face, "mara".to_string());
        // Face at the bottom-right, leaving top-left as negative space.
        registry.add_sample(face, 0.0, BoundingBox::new(820.0, 1500.0, 200.0, 200.0), 1.0);
        registry.add_sample(face, 2.0, BoundingBox::new(820.0, 1500.0, 200.0, 200.0), 1.0);

        let manifest = SemanticManifest {
            entity_refs: vec![face],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };

        let params = SolverParams {
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            safe_top: 60.0,
            safe_bottom: 110.0,
            safe_left: 60.0,
            safe_right: 60.0,
            sample_rate: 10.0,
        };
        let trajectory = solve_caption_trajectory_continuous(&registry, &manifest, &params);

        assert!(!trajectory.as_slice().is_empty());
        for sample in trajectory.as_slice() {
            // Should avoid the bottom-right face.
            assert!(sample.y < 1500.0, "caption should avoid bottom face, got y={}", sample.y);
            assert!(sample.x < 820.0, "caption should avoid right face, got x={}", sample.x);
        }
    }

    #[test]
    fn continuous_solver_prefers_negative_space() {
        let mut registry = EntityRegistry::new();
        let face_top = registry.create(EntityKind::Face, "top".to_string());
        let face_bottom = registry.create(EntityKind::Face, "bottom".to_string());
        // Faces at top and bottom, leaving a gap in the middle.
        registry.add_sample(face_top, 0.0, BoundingBox::new(400.0, 60.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_top, 2.0, BoundingBox::new(400.0, 60.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_bottom, 0.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);
        registry.add_sample(face_bottom, 2.0, BoundingBox::new(400.0, 1500.0, 200.0, 200.0), 1.0);

        let manifest = SemanticManifest {
            entity_refs: vec![face_top, face_bottom],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };

        let params = SolverParams {
            canvas_w: 1080.0,
            canvas_h: 1920.0,
            caption_w: 800.0,
            caption_h: 100.0,
            safe_top: 60.0,
            safe_bottom: 110.0,
            safe_left: 60.0,
            safe_right: 60.0,
            sample_rate: 10.0,
        };
        let trajectory = solve_caption_trajectory_continuous(&registry, &manifest, &params);

        assert!(!trajectory.as_slice().is_empty());
        for sample in trajectory.as_slice() {
            // Should be in the middle gap, away from both faces.
            assert!(
                sample.y > 300.0 && sample.y < 1300.0,
                "caption should sit in negative space, got y={}",
                sample.y
            );
        }
    }
}
