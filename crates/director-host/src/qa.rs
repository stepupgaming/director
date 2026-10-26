//! QA report: one pass over sampled layout, the render manifest detectors and
//! the audio mix, with a 0-100 score and a suggested fix per issue.

use crate::error::{ErrorCode, HostError};
use crate::session::Session;
use director_core::loudness::analyze_stereo;
use director_core::node::caption::scaled_safe_zone;
use director_core::render_manifest::{build_render_manifest, IssueSeverity, RenderManifest, VerificationConfig, VerificationIssueType};
use director_core::scene_export::node_ref;
use director_core::timeline::{audio_samples_per_video_frame, duration_to_frame_count};
use director_core::types::{NodeId, SafeZone};
use director_render::Frame;
use director_scene::NodeRef;
use serde::Serialize;
use std::collections::BTreeMap;

/// 10 minutes of stereo at 48 kHz. Longer mixes skip loudness analysis.
pub const MAX_QA_AUDIO_SAMPLES: u64 = 48_000 * 2 * 600;
/// A text clipping or safe-zone finding counts once it holds this many seconds,
/// so entrances and exits through the canvas edge stay quiet.
const HELD_SECONDS: f64 = 1.0;
/// Allowed distance between integrated loudness and the target, in LU.
const LOUDNESS_TOLERANCE: f64 = 2.0;
/// Highest true peak before a warning, in dBTP.
const TRUE_PEAK_CEILING: f64 = -1.0;
/// Seconds between painted frames compared for motion.
const MOTION_STEP: f64 = 0.5;
/// A run without visible change counts as a static stretch from this many seconds.
const STATIC_SECONDS: f64 = 2.0;

#[derive(Clone, Debug)]
pub struct QaOptions {
    /// Seconds between layout samples.
    pub every: f64,
    /// Platform safe-zone preset for text, such as `tiktok`.
    pub safe_zone: Option<String>,
    /// Integrated loudness target of the mix in LUFS.
    pub loudness_target: f64,
}

impl Default for QaOptions {
    fn default() -> Self {
        Self {
            every: 0.25,
            safe_zone: None,
            loudness_target: -14.0,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct QaReport {
    pub revision: u64,
    /// 100 minus 20 per error and 5 per warning, floored at 0.
    pub score: u32,
    /// True when no issue is an error.
    pub pass: bool,
    pub errors: usize,
    pub warnings: usize,
    /// Layout sample count.
    pub samples: usize,
    /// Errors first, then warnings, then info, each by start time.
    pub issues: Vec<QaIssue>,
    /// Loudness of the mix, present when the timeline has audio tracks.
    pub audio: Option<QaAudio>,
}

/// One finding, merged across the samples where it held.
#[derive(Clone, Debug, Serialize)]
pub struct QaIssue {
    pub kind: &'static str,
    pub severity: IssueSeverity,
    pub node: Option<NodeRef>,
    /// Authored name of the node, such as an HTML `id`.
    pub name: Option<String>,
    pub start: f64,
    pub end: f64,
    /// Samples where the finding held.
    pub count: usize,
    pub detail: String,
    pub fix: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct QaAudio {
    pub tracks: usize,
    /// Non-finite levels are null.
    pub integrated_lufs: Option<f64>,
    pub true_peak_db: Option<f64>,
    pub loudness_range_lu: Option<f64>,
    /// Mix samples at or above full scale.
    pub clipped_samples: usize,
}

/// Sample times from 0 to just before `duration`, at most 240, widening the
/// step on long timelines.
pub fn sample_times(duration: f64, every: f64) -> Vec<f64> {
    if every <= 0.0 || !every.is_finite() || !duration.is_finite() || duration <= 0.0 {
        return vec![0.0];
    }
    let mut step = every;
    while (duration / step).ceil() as usize + 2 > 240 && step < duration {
        step *= 2.0;
    }
    let mut times = Vec::new();
    let mut t = 0.0;
    while t < duration && times.len() < 240 {
        times.push(t);
        t += step;
    }
    let end = duration * 0.999;
    let already = times.iter().any(|sample| (sample - end).abs() <= 1e-6);
    if !already {
        if times.len() < 240 {
            times.push(end);
        } else if let Some(last) = times.last_mut() {
            if *last < end {
                *last = end;
            }
        }
    }
    if times.is_empty() {
        times.push(0.0);
    }
    times
}

/// Findings keyed by kind and node, merged across samples.
#[derive(Default)]
struct Issues(BTreeMap<(&'static str, Option<NodeId>), QaIssue>);

impl Issues {
    fn hit(&mut self, kind: &'static str, severity: IssueSeverity, node: Option<NodeId>, time: f64, detail: String, fix: Option<String>) {
        self.0
            .entry((kind, node))
            .and_modify(|issue| {
                issue.end = time;
                issue.count += 1;
            })
            .or_insert(QaIssue {
                kind,
                severity,
                node: node.map(node_ref),
                name: None,
                start: time,
                end: time,
                count: 1,
                detail,
                fix,
            });
    }
}

const TEXT_KINDS: [&str; 3] = ["text", "caption", "smart_caption"];

/// Kind, parent and whether the node touched the canvas at any sample.
type Seen = BTreeMap<NodeId, (&'static str, Option<NodeId>, bool)>;

fn layout(manifest: &RenderManifest, time: f64, zone: Option<(&str, &SafeZone)>, seen: &mut Seen, issues: &mut Issues) {
    let (width, height) = (manifest.width as f32, manifest.height as f32);
    for node in &manifest.nodes {
        let b = &node.bounds;
        if b.w <= 0.0 || b.h <= 0.0 || ![b.x, b.y, b.w, b.h].iter().all(|v| v.is_finite()) {
            continue;
        }
        let intersects = b.x < width && b.y < height && b.x + b.w > 0.0 && b.y + b.h > 0.0;
        let inside = b.x >= 0.0 && b.y >= 0.0 && b.x + b.w <= width && b.y + b.h <= height;
        seen.entry(node.id).or_insert((node.kind, node.parent_id, false)).2 |= intersects;
        if !TEXT_KINDS.contains(&node.kind) || !intersects {
            continue;
        }
        let bounds = format!("x={} y={} w={} h={}", b.x, b.y, b.w, b.h);
        if !inside {
            let fix = "Move or shrink the text so it fits inside the canvas".to_string();
            issues.hit("clipped", IssueSeverity::Warning, Some(node.id), time, format!("{} crosses the canvas edge at {bounds}", node.kind), Some(fix));
        }
        if let Some((preset, zone)) = zone {
            let outside = b.x < zone.left || b.y < zone.top || b.x + b.w > width - zone.right || b.y + b.h > height - zone.bottom;
            if outside {
                let fix = format!("Move the text inside the {preset} safe zone");
                issues.hit("safe_zone", IssueSeverity::Warning, Some(node.id), time, format!("{} enters the {preset} inset at {bounds}", node.kind), Some(fix));
            }
        }
    }
}

fn finite(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

impl Session {
    /// Check the current revision for publishing problems: missing fonts,
    /// nodes never on the canvas, clipped text, text in the safe-zone inset,
    /// captions over faces, reframed subjects out of frame, static stretches
    /// and the loudness, peak and clipping of the audio mix.
    pub fn qa(&mut self, options: &QaOptions) -> Result<QaReport, HostError> {
        self.qa_with_progress(options, &mut |_, _| {})
    }

    /// [`Session::qa`] that calls `progress(done, total)` as the layout
    /// samples, the detector grid, the audio and the static pass finish.
    pub fn qa_with_progress(&mut self, options: &QaOptions, progress: &mut dyn FnMut(usize, usize)) -> Result<QaReport, HostError> {
        let invalid = |message: String| HostError::new(ErrorCode::InvalidArgument, message);
        if !(options.every.is_finite() && options.every > 0.0) {
            return Err(invalid("every must be finite and positive".into()));
        }
        if !options.loudness_target.is_finite() {
            return Err(invalid("loudness_target must be finite".into()));
        }
        let height = self.document().height as f32;
        let zone = match options.safe_zone.as_deref() {
            None => None,
            Some(preset) => Some((preset, scaled_safe_zone(preset, height).ok_or_else(|| invalid(format!("unknown safe zone '{preset}'")))?)),
        };
        let names: BTreeMap<(u32, u32), String> = self.names().iter().map(|(name, id)| ((id.index, id.generation), name.clone())).collect();
        let duration = self.duration();
        let end = duration.max(0.0);
        let mut issues = Issues::default();
        let mut report: Vec<QaIssue> = self
            .missing_fonts()
            .iter()
            .map(|font| QaIssue {
                kind: "missing_font",
                severity: IssueSeverity::Error,
                node: None,
                name: None,
                start: 0.0,
                end,
                count: 1,
                detail: format!("required font {font} is not installed"),
                fix: Some("Install the font or bundle it with the project".into()),
            })
            .collect();

        let times = sample_times(duration, options.every);
        let grid = sample_times(duration, 1.0);
        let total = times.len() + grid.len() + 2;
        let mut done = 0;
        progress(done, total);
        let step = if times.len() > 1 { times[1] - times[0] } else { end };
        let mut seen = Seen::new();
        let director = self.inspector()?;
        for &time in &times {
            let manifest = build_render_manifest(director, time);
            layout(&manifest, time, zone.as_ref().map(|(preset, zone)| (*preset, zone)), &mut seen, &mut issues);
            done += 1;
            progress(done, total);
        }
        let held = HELD_SECONDS.min(end);
        issues.0.retain(|(kind, _), issue| !matches!(*kind, "clipped" | "safe_zone") || issue.count as f64 * step >= held);
        let never: Vec<NodeId> = seen.iter().filter(|(_, (_, _, touched))| !touched).map(|(id, _)| *id).collect();
        for id in &never {
            let (kind, parent, _) = seen[id];
            if parent.is_some_and(|parent| never.contains(&parent)) {
                continue;
            }
            let fix = Some("Move the node onto the canvas or remove it".to_string());
            issues.hit("off_canvas", IssueSeverity::Error, Some(*id), 0.0, format!("{kind} stays outside the canvas at every sample"), fix);
            if let Some(issue) = issues.0.get_mut(&("off_canvas", Some(*id))) {
                (issue.end, issue.count) = (times.last().copied().unwrap_or(0.0), times.len());
            }
        }

        // Face and camera detectors on a one-second grid. Text placement is covered
        // above and motion is measured on painted frames below, so the manifest's
        // safe-zone and layout-motion passes stay out (a zero window skips the latter).
        let config = VerificationConfig {
            static_stretch_window: 0.0,
            ..VerificationConfig::default()
        };
        for time in grid {
            done += 1;
            progress(done, total);
            let manifest = build_render_manifest(director, time);
            for issue in manifest.verify(director, &config) {
                let node = issue.node_ids.first().copied();
                match issue.issue_type {
                    VerificationIssueType::SafeZoneViolation | VerificationIssueType::StaticStretch => {}
                    VerificationIssueType::CaptionOverFace => {
                        issues.hit("caption_over_face", IssueSeverity::Warning, node, time, issue.description, issue.suggested_fix)
                    }
                    VerificationIssueType::SubjectOutOfFrame => {
                        issues.hit("subject_out_of_frame", IssueSeverity::Warning, node, time, issue.description, issue.suggested_fix)
                    }
                }
            }
        }

        let tracks = director.audio_mixer.tracks.iter().flatten().count();
        let audio = if tracks == 0 {
            None
        } else {
            let (rate, fps) = (director.audio_mixer.sample_rate, director.fps);
            let total = duration_to_frame_count(director.timeline_duration(), fps).saturating_mul(audio_samples_per_video_frame(rate, fps, 1) as u64 * 2);
            if total > MAX_QA_AUDIO_SAMPLES {
                report.push(QaIssue {
                    kind: "audio_unmeasured",
                    severity: IssueSeverity::Info,
                    node: None,
                    name: None,
                    start: 0.0,
                    end,
                    count: 1,
                    detail: format!("the mix has {total} samples, above the {MAX_QA_AUDIO_SAMPLES} analysis limit"),
                    fix: None,
                });
                None
            } else {
                let wav = self.audio_wav()?;
                let samples: Vec<f32> = wav[44..].as_chunks::<4>().0.iter().map(|bytes| f32::from_le_bytes(*bytes)).collect();
                Some(audio_issues(&samples, rate, tracks, options.loudness_target, end, &mut report))
            }
        };

        done += 1;
        progress(done, total);
        for (start, stop) in self.static_stretches(duration)? {
            report.push(QaIssue {
                kind: "static",
                severity: IssueSeverity::Info,
                node: None,
                name: None,
                start,
                end: stop,
                count: 1,
                detail: format!("no visible change from {start:.2}s to {stop:.2}s"),
                fix: Some("Add motion, a cut or B-roll".into()),
            });
        }

        progress(total, total);
        report.extend(issues.0.into_values());
        for issue in &mut report {
            issue.name = issue.node.and_then(|id| names.get(&(id.index, id.generation)).cloned());
        }
        report.sort_by(|a, b| rank(a.severity).cmp(&rank(b.severity)).then(a.start.total_cmp(&b.start)));
        let errors = report.iter().filter(|issue| issue.severity == IssueSeverity::Error).count();
        let warnings = report.iter().filter(|issue| issue.severity == IssueSeverity::Warning).count();
        Ok(QaReport {
            revision: self.revision(),
            score: 100u32.saturating_sub(20 * errors as u32 + 5 * warnings as u32),
            pass: errors == 0,
            errors,
            warnings,
            samples: times.len(),
            issues: report,
            audio,
        })
    }
}

impl Session {
    /// Time ranges of at least [`STATIC_SECONDS`] where painted frames
    /// [`MOTION_STEP`] apart show no visible change.
    fn static_stretches(&mut self, duration: f64) -> Result<Vec<(f64, f64)>, HostError> {
        let mut stretches = Vec::new();
        let mut previous: Option<(f64, Frame)> = None;
        let mut run: Option<(f64, f64)> = None;
        for time in sample_times(duration, MOTION_STEP) {
            let frame = self.render_frame(time)?;
            if let Some((before, last)) = &previous {
                if changed(last, &frame) {
                    stretches.extend(run.take());
                } else {
                    run.get_or_insert((*before, time)).1 = time;
                }
            }
            previous = Some((time, frame));
        }
        stretches.extend(run);
        stretches.retain(|(start, end)| end - start >= STATIC_SECONDS.min(duration * 0.99));
        Ok(stretches)
    }
}

/// True when more than 0.1% of the sampled pixels move a channel by more than 8.
fn changed(a: &Frame, b: &Frame) -> bool {
    if (a.width, a.height) != (b.width, b.height) {
        return true;
    }
    let (mut sampled, mut moved) = (0usize, 0usize);
    for y in (0..a.height as usize).step_by(4) {
        for x in (0..a.width as usize).step_by(4) {
            let at = (y * a.width as usize + x) * 4;
            sampled += 1;
            moved += usize::from(a.rgba[at..at + 4].iter().zip(&b.rgba[at..at + 4]).any(|(p, q)| p.abs_diff(*q) > 8));
        }
    }
    moved * 1000 > sampled
}

fn rank(severity: IssueSeverity) -> u8 {
    match severity {
        IssueSeverity::Error => 0,
        IssueSeverity::Warning => 1,
        IssueSeverity::Info => 2,
    }
}

/// Clipping, true peak, silence and loudness findings for a stereo mix.
fn audio_issues(samples: &[f32], rate: u32, tracks: usize, target: f64, end: f64, report: &mut Vec<QaIssue>) -> QaAudio {
    let mut push = |kind, severity, detail: String, fix: Option<String>| {
        report.push(QaIssue {
            kind,
            severity,
            node: None,
            name: None,
            start: 0.0,
            end,
            count: 1,
            detail,
            fix,
        })
    };
    if samples.iter().any(|sample| !sample.is_finite()) {
        push("invalid_audio", IssueSeverity::Error, "the mix holds a non-finite sample".into(), Some("Check the audio sources and effects".into()));
        return QaAudio {
            tracks,
            integrated_lufs: None,
            true_peak_db: None,
            loudness_range_lu: None,
            clipped_samples: 0,
        };
    }
    let clipped_samples = samples.iter().filter(|sample| sample.abs() >= 1.0).count();
    let analysis = analyze_stereo(samples, rate);
    let lufs = finite(analysis.integrated_lufs).filter(|_| analysis.is_valid());
    if clipped_samples > 0 {
        let fix = Some("Lower the track volumes or add a limiter".to_string());
        push("clipped_audio", IssueSeverity::Error, format!("{clipped_samples} mix samples reach full scale"), fix);
    } else if analysis.true_peak_db > TRUE_PEAK_CEILING {
        let fix = Some(format!("Lower the mix by {:.1} dB or add a limiter", analysis.true_peak_db - TRUE_PEAK_CEILING));
        push("true_peak", IssueSeverity::Warning, format!("true peak {:.1} dBTP is above {TRUE_PEAK_CEILING} dBTP", analysis.true_peak_db), fix);
    }
    match lufs {
        None => push("silent_audio", IssueSeverity::Warning, format!("{tracks} audio tracks mix to silence"), Some("Check track volumes, mutes and start times".into())),
        Some(lufs) if (lufs - target).abs() > LOUDNESS_TOLERANCE => {
            let delta = target - lufs;
            let direction = if delta > 0.0 { "Raise" } else { "Lower" };
            let fix = Some(format!("{direction} the mix by {:.1} dB", delta.abs()));
            push("loudness", IssueSeverity::Warning, format!("integrated loudness {lufs:.1} LUFS is off the {target} LUFS target"), fix);
        }
        Some(_) => {}
    }
    QaAudio {
        tracks,
        integrated_lufs: lufs,
        true_peak_db: finite(analysis.true_peak_db),
        loudness_range_lu: finite(analysis.loudness_range_lu),
        clipped_samples,
    }
}

#[cfg(test)]
mod tests {
    use super::{audio_issues, sample_times};

    #[test]
    fn samples_include_start_and_end() {
        let times = sample_times(1.0, 0.25);
        assert!(times.contains(&0.0));
        assert!(times.iter().any(|time| *time > 0.99));
        assert_eq!(sample_times(1.0, f64::NAN), vec![0.0]);
        assert!(sample_times(10_000.0, 0.01).len() <= 240);
    }

    #[test]
    fn audio_flags_clipping_loudness_and_silence() {
        let mut report = Vec::new();
        // A full-scale 1 kHz square wave on both channels.
        let loud: Vec<f32> = (0..96_000).map(|i| if (i / 2) % 48 < 24 { 1.0 } else { -1.0 }).collect();
        let audio = audio_issues(&loud, 48_000, 1, -14.0, 1.0, &mut report);
        assert!(audio.clipped_samples > 0);
        assert!(report.iter().any(|issue| issue.kind == "clipped_audio"));
        assert!(report.iter().any(|issue| issue.kind == "loudness" && issue.fix.as_deref().unwrap().starts_with("Lower")));

        report.clear();
        audio_issues(&vec![0.0; 96_000], 48_000, 2, -14.0, 1.0, &mut report);
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].kind, "silent_audio");
    }
}
