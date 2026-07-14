//! # Style Policy Module
//!
//! Production-rule schema and scoring for the Director compiler.
//!
//! ## Responsibilities
//! - Encode production rules for typography occupancy, motion budgets,
//!   transition frequency, and audio loudness.
//! - Provide a `score()` function the compiler can use to evaluate a
//!   `Director` scene against a policy.
//! - Emit machine-actionable `PolicyViolation`s so the compiler/repair
//!   pipeline can react.
//!
//! ## Key Types
//! - `StylePolicy`: Top-level container for all sub-policies.
//! - `TypographyPolicy`, `MotionPolicy`, `TransitionPolicy`, `AudioPolicy`:
//!   Rule sets for specific production dimensions.
//! - `PolicyViolation`: A single rule breach with severity and context.
//! - `PolicyScore`: Aggregate score and violation list.
//! - `StylePolicyScorer`: Evaluates a `Director` against a policy.

use crate::director::Director;
use crate::loudness::{analyze_stereo, LoudnessAnalysis};
use crate::node::caption::CaptionNode;
use crate::node::smart_caption::SmartCaptionNode;
use crate::node::text::TextNode;
use crate::render_manifest::IssueSeverity;
use crate::types::NodeId;
use serde::{Deserialize, Serialize};

/// A single production-rule violation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyViolation {
    /// Human-readable rule name, e.g. `typography.occupancy`.
    pub rule: String,
    /// Severity of the violation.
    pub severity: IssueSeverity,
    /// Affected node, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<NodeId>,
    /// Time in seconds where the violation was sampled, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<f64>,
    /// Human-readable description.
    pub description: String,
}

/// Aggregate score returned by `StylePolicyScorer::score`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PolicyScore {
    /// Normalized score in `[0.0, 1.0]`. Higher is better.
    pub total_score: f64,
    /// All violations found during scoring.
    pub violations: Vec<PolicyViolation>,
    /// Per-dimension sub-scores for diagnostics.
    pub dimension_scores: Vec<(String, f64)>,
}

/// Production rules for typography / on-screen text.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TypographyPolicy {
    /// Maximum fraction of the canvas that may be covered by text/captions.
    pub max_text_area_ratio: f64,
    /// Maximum number of display lines per caption cue.
    pub max_lines_per_caption: u32,
    /// Minimum allowed font size in pixels.
    pub min_font_size: f32,
    /// Maximum allowed font size in pixels.
    pub max_font_size: f32,
    /// Margin in pixels that text nodes must keep from the canvas edges.
    pub safe_zone_margin: f32,
    /// Weight of this dimension when computing the total score.
    #[serde(default = "TypographyPolicy::default_weight")]
    pub weight: f64,
}

impl TypographyPolicy {
    pub fn default_weight() -> f64 {
        1.0
    }
}

impl Default for TypographyPolicy {
    fn default() -> Self {
        Self {
            max_text_area_ratio: 0.35,
            max_lines_per_caption: 3,
            min_font_size: 12.0,
            max_font_size: 200.0,
            safe_zone_margin: 16.0,
            weight: Self::default_weight(),
        }
    }
}

/// Production rules for motion / animation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MotionPolicy {
    /// Maximum allowed centroid velocity in pixels per second.
    pub max_pixels_per_second: f32,
    /// Maximum allowed rotation velocity in degrees per second.
    pub max_rotation_velocity: f32,
    /// Maximum allowed scale velocity in scale units per second.
    pub max_scale_velocity: f32,
    /// Average per-sample centroid displacement (in pixels) required to avoid a
    /// "static stretch". Measured as the mean displacement across all nodes and
    /// sample pairs.
    pub avg_motion_displacement_threshold: f32,
    /// Weight of this dimension when computing the total score.
    #[serde(default = "MotionPolicy::default_weight")]
    pub weight: f64,
}

impl MotionPolicy {
    pub fn default_weight() -> f64 {
        1.0
    }
}

impl Default for MotionPolicy {
    fn default() -> Self {
        Self {
            max_pixels_per_second: 800.0,
            max_rotation_velocity: 360.0,
            max_scale_velocity: 4.0,
            avg_motion_displacement_threshold: 0.5,
            weight: Self::default_weight(),
        }
    }
}

/// Production rules for scene transitions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransitionPolicy {
    /// Minimum seconds between consecutive transitions.
    pub min_seconds_between_transitions: f64,
    /// Maximum number of transitions per minute of timeline.
    pub max_transitions_per_minute: f64,
    /// Minimum allowed transition duration in seconds.
    pub min_transition_duration: f64,
    /// Maximum allowed transition duration in seconds.
    pub max_transition_duration: f64,
    /// Weight of this dimension when computing the total score.
    #[serde(default = "TransitionPolicy::default_weight")]
    pub weight: f64,
}

impl TransitionPolicy {
    pub fn default_weight() -> f64 {
        1.0
    }
}

impl Default for TransitionPolicy {
    fn default() -> Self {
        Self {
            min_seconds_between_transitions: 1.5,
            max_transitions_per_minute: 12.0,
            min_transition_duration: 0.15,
            max_transition_duration: 1.5,
            weight: Self::default_weight(),
        }
    }
}

/// Production rules for audio loudness.
///
/// Uses true IT-R BS.1770 integrated loudness (LUFS), true peak (dBTP), and
/// loudness range (LU) as measured by the `ebur128` library.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioPolicy {
    /// Target integrated loudness in LUFS.
    pub target_loudness_db: f32,
    /// Allowed deviation from target loudness before a warning.
    pub loudness_tolerance_db: f32,
    /// Maximum allowed true peak in dBTP.
    pub max_true_peak_db: f32,
    /// Minimum acceptable loudness range (LU) for dynamic content.
    /// Currently unused; reserved for future true loudness-range analysis.
    pub min_loudness_range: f32,
    /// Weight of this dimension when computing the total score.
    #[serde(default = "AudioPolicy::default_weight")]
    pub weight: f64,
}

impl AudioPolicy {
    pub fn default_weight() -> f64 {
        1.0
    }
}

impl Default for AudioPolicy {
    fn default() -> Self {
        Self {
            target_loudness_db: -14.0,
            loudness_tolerance_db: 2.0,
            max_true_peak_db: -1.0,
            min_loudness_range: 6.0,
            weight: Self::default_weight(),
        }
    }
}

/// Top-level production style policy.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StylePolicy {
    #[serde(default)]
    pub typography: TypographyPolicy,
    #[serde(default)]
    pub motion: MotionPolicy,
    #[serde(default)]
    pub transitions: TransitionPolicy,
    #[serde(default)]
    pub audio: AudioPolicy,
}

/// Evaluates a `Director` scene against a `StylePolicy`.
#[derive(Clone, Debug, Default)]
pub struct StylePolicyScorer;

impl StylePolicyScorer {
    /// Create a new scorer.
    pub fn new() -> Self {
        Self
    }

    /// Score the director over the given time range using `samples` evenly spaced samples.
    ///
    /// Returns a `PolicyScore` with per-dimension sub-scores and a list of violations.
    pub fn score(
        &self,
        director: &Director,
        policy: &StylePolicy,
        time_range: (f64, f64),
        samples: usize,
    ) -> PolicyScore {
        let mut violations = Vec::new();
        let mut dimension_scores: Vec<(String, f64)> = Vec::new();

        let typography_score = self.score_typography(director, policy, &mut violations);
        dimension_scores.push(("typography".to_string(), typography_score));

        let motion_score = self.score_motion(director, policy, time_range, samples, &mut violations);
        dimension_scores.push(("motion".to_string(), motion_score));

        let transition_score = self.score_transitions(director, policy, &mut violations);
        dimension_scores.push(("transitions".to_string(), transition_score));

        let audio_score = self.score_audio(director, &policy.audio, &mut violations);
        dimension_scores.push(("audio".to_string(), audio_score));

        let total_weight = policy.typography.weight
            + policy.motion.weight
            + policy.transitions.weight
            + policy.audio.weight;
        let total_score = if total_weight > 0.0 {
            (typography_score * policy.typography.weight
                + motion_score * policy.motion.weight
                + transition_score * policy.transitions.weight
                + audio_score * policy.audio.weight)
                / total_weight
        } else {
            1.0
        };

        PolicyScore {
            total_score,
            violations,
            dimension_scores,
        }
    }

    fn score_typography(
        &self,
        director: &Director,
        policy: &StylePolicy,
        violations: &mut Vec<PolicyViolation>,
    ) -> f64 {
        let canvas_area = (director.width as f64) * (director.height as f64);
        if canvas_area <= 0.0 {
            return 1.0;
        }

        let mut text_area: f64 = 0.0;

        for (id, node_opt) in director.scene.nodes.iter().enumerate() {
            let Some(node) = node_opt else { continue };
            let is_text = node.element.as_any().is::<TextNode>()
                || node.element.as_any().is::<CaptionNode>()
                || node.element.as_any().is::<SmartCaptionNode>();

            if !is_text {
                continue;
            }

            let rect = node.layout_rect;
            let area = (rect.width() as f64) * (rect.height() as f64);
            text_area += area;

            // Safe-zone margin check.
            let margin = policy.typography.safe_zone_margin;
            if rect.left < margin
                || rect.top < margin
                || (rect.right > (director.width as f32 - margin))
                || (rect.bottom > (director.height as f32 - margin))
            {
                violations.push(PolicyViolation {
                    rule: "typography.safe_zone_margin".to_string(),
                    severity: IssueSeverity::Warning,
                    node_id: Some(id),
                    time: None,
                    description: format!(
                        "text node {} is within {:.0}px of a canvas edge",
                        id, margin
                    ),
                });
            }

            // Font size checks require downcasting to TextNode.
            if let Some(text) = node.element.as_any().downcast_ref::<TextNode>() {
                let size = text.default_font_size.current_value;
                if size < policy.typography.min_font_size {
                    violations.push(PolicyViolation {
                        rule: "typography.min_font_size".to_string(),
                        severity: IssueSeverity::Error,
                        node_id: Some(id),
                        time: None,
                        description: format!(
                            "font size {:.1}px is below minimum {:.1}px",
                            size, policy.typography.min_font_size
                        ),
                    });
                }
                if size > policy.typography.max_font_size {
                    violations.push(PolicyViolation {
                        rule: "typography.max_font_size".to_string(),
                        severity: IssueSeverity::Warning,
                        node_id: Some(id),
                        time: None,
                        description: format!(
                            "font size {:.1}px exceeds maximum {:.1}px",
                            size, policy.typography.max_font_size
                        ),
                    });
                }
            }

            if let Some(caption) = node.element.as_any().downcast_ref::<CaptionNode>() {
                let line_count = caption.lines.iter().map(|l| {
                    l.words
                        .iter()
                        .map(|w| w.display_line_idx)
                        .max()
                        .map(|m| m + 1)
                        .unwrap_or(1)
                }).sum::<usize>() as u32;
                if line_count > policy.typography.max_lines_per_caption {
                    violations.push(PolicyViolation {
                        rule: "typography.max_lines_per_caption".to_string(),
                        severity: IssueSeverity::Warning,
                        node_id: Some(id),
                        time: None,
                        description: format!(
                            "caption {} has {} lines, max is {}",
                            id, line_count, policy.typography.max_lines_per_caption
                        ),
                    });
                }
            }
        }

        let ratio = text_area / canvas_area;
        if ratio > policy.typography.max_text_area_ratio {
            violations.push(PolicyViolation {
                rule: "typography.max_text_area_ratio".to_string(),
                severity: IssueSeverity::Warning,
                node_id: None,
                time: None,
                description: format!(
                    "text covers {:.2}% of canvas, max allowed {:.2}%",
                    ratio * 100.0,
                    policy.typography.max_text_area_ratio * 100.0
                ),
            });
        }

        // Score decays linearly from 1.0 at 0% coverage to 0.0 at 2x the max.
        let score = 1.0 - (ratio / (policy.typography.max_text_area_ratio * 2.0)).min(1.0);
        score.max(0.0)
    }

    fn score_motion(
        &self,
        director: &Director,
        policy: &StylePolicy,
        time_range: (f64, f64),
        samples: usize,
        violations: &mut Vec<PolicyViolation>,
    ) -> f64 {
        if samples < 2 || time_range.1 <= time_range.0 {
            return 1.0;
        }

        // Drive the actual Director update loop so we capture audio-reactive
        // bindings, path animation, and parent transforms. We sample transform
        // values directly because `Director::update` does not run layout.
        let mut sampled_director = director.clone();
        let dt = (time_range.1 - time_range.0) / (samples.saturating_sub(1)).max(1) as f64;
        let mut total_displacement: f64 = 0.0;
        let mut violation_count = 0usize;
        let mut evaluated_pairs = 0usize;

        // Previous sampled state: (node_id, x, y, rotation, scale_x, scale_y).
        let mut prev_state: Vec<(usize, f64, f64, f64, f64, f64)> = Vec::new();

        for i in 0..samples {
            let t = time_range.0 + i as f64 * dt;

            // Activate every node so that `Director::update` processes transforms
            // even when the node is not currently on an active timeline root.
            // Policy scoring evaluates the intrinsic motion of the scene graph.
            for node in sampled_director.scene.nodes.iter_mut().flatten() {
                node.last_visit_time = t;
                node.local_time = t;
            }
            sampled_director.update(t);

            let mut current_state: Vec<(usize, f64, f64, f64, f64, f64)> = Vec::new();
            for (id, node_opt) in sampled_director.scene.nodes.iter().enumerate() {
                let Some(node) = node_opt else { continue };
                current_state.push((
                    id,
                    node.transform.translate_x.current_value as f64,
                    node.transform.translate_y.current_value as f64,
                    node.transform.rotation.current_value as f64,
                    node.transform.scale_x.current_value as f64,
                    node.transform.scale_y.current_value as f64,
                ));
            }

            if !prev_state.is_empty() {
                let delta_t = dt;
                for (id, cx, cy, crot, csx, csy) in &current_state {
                    let prev = prev_state.iter().find(|(pid, _, _, _, _, _)| *pid == *id);
                    let Some((_, px, py, prot, psx, psy)) = prev else { continue };

                    let displacement = ((cx - px).powi(2) + (cy - py).powi(2)).sqrt();
                    total_displacement += displacement;
                    evaluated_pairs += 1;

                    if delta_t > 0.0 {
                        let velocity = displacement / delta_t;
                        if velocity > policy.motion.max_pixels_per_second as f64 {
                            violation_count += 1;
                            violations.push(PolicyViolation {
                                rule: "motion.max_pixels_per_second".to_string(),
                                severity: IssueSeverity::Warning,
                                node_id: Some(*id),
                                time: Some(t),
                                description: format!(
                                    "node {} moves at {:.0} px/s, max {:.0}",
                                    id, velocity, policy.motion.max_pixels_per_second
                                ),
                            });
                        }

                        let rot_vel = (crot - prot).abs() / delta_t;
                        if rot_vel > policy.motion.max_rotation_velocity as f64 {
                            violation_count += 1;
                            violations.push(PolicyViolation {
                                rule: "motion.max_rotation_velocity".to_string(),
                                severity: IssueSeverity::Warning,
                                node_id: Some(*id),
                                time: Some(t),
                                description: format!(
                                    "node {} rotates at {:.0} deg/s, max {:.0}",
                                    id, rot_vel, policy.motion.max_rotation_velocity
                                ),
                            });
                        }

                        let scale_vel = ((csx - psx).abs().max((csy - psy).abs())) / delta_t;
                        if scale_vel > policy.motion.max_scale_velocity as f64 {
                            violation_count += 1;
                            violations.push(PolicyViolation {
                                rule: "motion.max_scale_velocity".to_string(),
                                severity: IssueSeverity::Warning,
                                node_id: Some(*id),
                                time: Some(t),
                                description: format!(
                                    "node {} scales at {:.2}/s, max {:.2}",
                                    id, scale_vel, policy.motion.max_scale_velocity
                                ),
                            });
                        }
                    }
                }
            }

            prev_state = current_state;
        }

        // Only flag static stretches if we actually evaluated motion samples.
        if evaluated_pairs > 0 {
            let avg_displacement = total_displacement / evaluated_pairs as f64;
            if avg_displacement < policy.motion.avg_motion_displacement_threshold as f64 {
                violations.push(PolicyViolation {
                    rule: "motion.avg_motion_displacement_threshold".to_string(),
                    severity: IssueSeverity::Info,
                    node_id: None,
                    time: None,
                    description: format!(
                        "average motion displacement {:.4}px is below threshold {:.4}px",
                        avg_displacement, policy.motion.avg_motion_displacement_threshold
                    ),
                });
            }
        }

        // Score penalizes violations but never goes below 0.0.
        let score = 1.0 - (violation_count as f64 * 0.1).min(1.0);
        score.max(0.0)
    }

    fn score_transitions(
        &self,
        director: &Director,
        policy: &StylePolicy,
        violations: &mut Vec<PolicyViolation>,
    ) -> f64 {
        let transitions = &director.transitions;
        if transitions.is_empty() {
            return 1.0;
        }

        let mut sorted = transitions.clone();
        sorted.sort_by(|a, b| a.start_time.partial_cmp(&b.start_time).unwrap_or(std::cmp::Ordering::Equal));

        let mut violation_count = 0usize;
        for (i, t) in sorted.iter().enumerate() {
            if t.duration < policy.transitions.min_transition_duration {
                violation_count += 1;
                violations.push(PolicyViolation {
                    rule: "transitions.min_transition_duration".to_string(),
                    severity: IssueSeverity::Warning,
                    node_id: None,
                    time: Some(t.start_time),
                    description: format!(
                        "transition {} duration {:.2}s is below minimum {:.2}s",
                        i, t.duration, policy.transitions.min_transition_duration
                    ),
                });
            }
            if t.duration > policy.transitions.max_transition_duration {
                violation_count += 1;
                violations.push(PolicyViolation {
                    rule: "transitions.max_transition_duration".to_string(),
                    severity: IssueSeverity::Warning,
                    node_id: None,
                    time: Some(t.start_time),
                    description: format!(
                        "transition {} duration {:.2}s exceeds maximum {:.2}s",
                        i, t.duration, policy.transitions.max_transition_duration
                    ),
                });
            }
        }

        for window in sorted.windows(2) {
            let gap = window[1].start_time - (window[0].start_time + window[0].duration);
            if gap < policy.transitions.min_seconds_between_transitions {
                violation_count += 1;
                violations.push(PolicyViolation {
                    rule: "transitions.min_seconds_between_transitions".to_string(),
                    severity: IssueSeverity::Warning,
                    node_id: None,
                    time: Some(window[1].start_time),
                    description: format!(
                        "transitions are only {:.2}s apart, minimum is {:.2}s",
                        gap, policy.transitions.min_seconds_between_transitions
                    ),
                });
            }
        }

        let duration_minutes = total_duration_minutes(director);
        if duration_minutes > 0.0 {
            let rate = sorted.len() as f64 / duration_minutes;
            if rate > policy.transitions.max_transitions_per_minute {
                violation_count += 1;
                violations.push(PolicyViolation {
                    rule: "transitions.max_transitions_per_minute".to_string(),
                    severity: IssueSeverity::Warning,
                    node_id: None,
                    time: None,
                    description: format!(
                        "{:.1} transitions/min exceeds maximum {:.1}",
                        rate, policy.transitions.max_transitions_per_minute
                    ),
                });
            }
        }

        let score = 1.0 - (violation_count as f64 * 0.15).min(1.0);
        score.max(0.0)
    }

    fn score_audio(
        &self,
        director: &Director,
        policy: &AudioPolicy,
        violations: &mut Vec<PolicyViolation>,
    ) -> f64 {
        let mut violation_count = 0usize;
        let mut has_tracks = false;

        for (idx, track_opt) in director.audio_mixer.tracks.iter().enumerate() {
            let Some(track) = track_opt else { continue };
            has_tracks = true;
            let samples = &track.samples;
            if samples.is_empty() {
                continue;
            }

            let analysis = analyze_stereo(samples, director.audio_mixer.sample_rate);
            if !analysis.is_valid() {
                continue;
            }

            let integrated_lufs = analysis.integrated_lufs;
            let true_peak_db = analysis.true_peak_db;
            let loudness_range_lu = analysis.loudness_range_lu;

            if integrated_lufs.is_finite()
                && (integrated_lufs - policy.target_loudness_db as f64).abs()
                    > policy.loudness_tolerance_db as f64
            {
                violation_count += 1;
                violations.push(PolicyViolation {
                    rule: "audio.target_loudness".to_string(),
                    severity: IssueSeverity::Warning,
                    node_id: None,
                    time: None,
                    description: format!(
                        "track {} integrated loudness {:.1} LUFS deviates from target {:.1} LUFS",
                        idx, integrated_lufs, policy.target_loudness_db
                    ),
                });
            }

            if true_peak_db.is_finite() && true_peak_db > policy.max_true_peak_db as f64 {
                violation_count += 1;
                violations.push(PolicyViolation {
                    rule: "audio.max_true_peak".to_string(),
                    severity: IssueSeverity::Error,
                    node_id: None,
                    time: None,
                    description: format!(
                        "track {} true peak {:.1} dBTP exceeds maximum {:.1} dBTP",
                        idx, true_peak_db, policy.max_true_peak_db
                    ),
                });
            }

            if policy.min_loudness_range > 0.0
                && loudness_range_lu.is_finite()
                && loudness_range_lu < policy.min_loudness_range as f64
            {
                violation_count += 1;
                violations.push(PolicyViolation {
                    rule: "audio.min_loudness_range".to_string(),
                    severity: IssueSeverity::Warning,
                    node_id: None,
                    time: None,
                    description: format!(
                        "track {} loudness range {:.1} LU is below minimum {:.1} LU",
                        idx, loudness_range_lu, policy.min_loudness_range
                    ),
                });
            }
        }

        if !has_tracks {
            return 1.0;
        }

        let score = 1.0 - (violation_count as f64 * 0.2).min(1.0);
        score.max(0.0)
    }
}

/// Compute total timeline duration in minutes, including both legacy timeline
/// and modern track clips.
pub(crate) fn total_duration_minutes(director: &Director) -> f64 {
    let mut max_end = 0.0f64;
    for item in &director.timeline {
        max_end = max_end.max(item.start_time + item.duration);
    }
    for track in &director.tracks {
        for clip in &track.clips {
            max_end = max_end.max(clip.start_time + clip.duration);
        }
    }
    max_end / 60.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::EasingType;
    use crate::director::Director;
    use crate::node::text::TextNode;
    use crate::systems::transitions::{Transition, TransitionType};
    use crate::video_wrapper::RenderMode;
    use crate::DefaultAssetLoader;
    use skia_safe::textlayout::FontCollection;
    use std::sync::{Arc, Mutex};

    fn empty_director() -> Director {
        Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        )
    }

    fn empty_font_collection() -> Arc<Mutex<FontCollection>> {
        Arc::new(Mutex::new(FontCollection::new()))
    }

    #[test]
    fn default_policy_scores_perfectly_for_empty_scene() {
        let director = empty_director();
        let policy = StylePolicy::default();
        let scorer = StylePolicyScorer::new();
        let score = scorer.score(&director, &policy, (0.0, 1.0), 2);
        assert_eq!(score.total_score, 1.0);
        assert!(score.violations.is_empty());
    }

    #[test]
    fn director_score_style_policy_uses_default_policy() {
        let mut director = empty_director();
        let mut text = TextNode::new(vec![], empty_font_collection());
        text.default_font_size.current_value = 250.0;
        let id = director.scene.add_node(Box::new(text));
        director.scene.nodes[id].as_mut().unwrap().layout_rect = skia_safe::Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        director.style_policy.typography.max_font_size = 200.0;

        let score = director.score_style_policy((0.0, 1.0), 2);
        assert!(score.violations.iter().any(|v| v.rule == "typography.max_font_size"));
    }

    #[test]
    fn detects_oversized_text() {
        let mut director = empty_director();
        let mut text = TextNode::new(vec![], empty_font_collection());
        text.default_font_size.current_value = 250.0;
        let id = director.scene.add_node(Box::new(text));
        director.scene.nodes[id].as_mut().unwrap().layout_rect = skia_safe::Rect::from_xywh(0.0, 0.0, 100.0, 100.0);

        let mut policy = StylePolicy::default();
        policy.typography.max_font_size = 200.0;
        let scorer = StylePolicyScorer::new();
        let score = scorer.score(&director, &policy, (0.0, 1.0), 2);

        assert!(score.violations.iter().any(|v| v.rule == "typography.max_font_size"));
    }

    #[test]
    fn detects_text_occupancy() {
        let mut director = empty_director();
        let mut text = TextNode::new(vec![], empty_font_collection());
        text.default_font_size.current_value = 24.0;
        let id = director.scene.add_node(Box::new(text));
        // Cover 50% of the canvas with text.
        director.scene.nodes[id].as_mut().unwrap().layout_rect = skia_safe::Rect::from_xywh(0.0, 0.0, 1080.0, 960.0);

        let mut policy = StylePolicy::default();
        policy.typography.max_text_area_ratio = 0.35;
        let scorer = StylePolicyScorer::new();
        let score = scorer.score(&director, &policy, (0.0, 1.0), 2);

        assert!(score.violations.iter().any(|v| v.rule == "typography.max_text_area_ratio"));
        assert!(score.total_score < 1.0);
    }

    #[test]
    fn detects_too_many_transitions() {
        let mut director = empty_director();
        director.timeline.push(crate::director::TimelineItem {
            scene_root: 0,
            name: None,
            start_time: 0.0,
            duration: 60.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        for i in 0..20 {
            director.transitions.push(Transition {
                from_scene_idx: i,
                to_scene_idx: i + 1,
                start_time: i as f64 * 2.0,
                duration: 0.5,
                kind: TransitionType::Fade,
                easing: EasingType::Linear,
            });
        }

        let policy = StylePolicy::default();
        let scorer = StylePolicyScorer::new();
        let score = scorer.score(&director, &policy, (0.0, 1.0), 2);

        assert!(score.total_score < 1.0);
        assert!(score.violations.iter().any(|v| v.rule == "transitions.max_transitions_per_minute"));
    }

    #[test]
    fn detects_audio_peak_violation() {
        let mut director = empty_director();
        let samples = vec![1.2f32; 48000 * 2];
        director.add_global_audio(samples);

        let policy = StylePolicy::default();
        let scorer = StylePolicyScorer::new();
        let score = scorer.score(&director, &policy, (0.0, 1.0), 2);

        assert!(score.violations.iter().any(|v| v.rule == "audio.max_true_peak"));
    }

    #[test]
    fn detects_excessive_motion() {
        let mut director = empty_director();
        let mut text = TextNode::new(vec![], empty_font_collection());
        text.default_font_size.current_value = 24.0;
        let id = director.scene.add_node(Box::new(text));
        // Animate translate_x from 0 to 2000 over 1 second.
        director.scene.nodes[id].as_mut().unwrap().transform.translate_x.add_segment(0.0, 2000.0, 1.0, EasingType::Linear);

        let mut policy = StylePolicy::default();
        policy.motion.max_pixels_per_second = 100.0;
        let scorer = StylePolicyScorer::new();
        let score = scorer.score(&director, &policy, (0.0, 1.0), 3);

        assert!(score.violations.iter().any(|v| v.rule == "motion.max_pixels_per_second"));
    }
}
