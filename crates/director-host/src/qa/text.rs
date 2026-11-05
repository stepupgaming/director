//! Text checks on painted pixels. A plate document hides every glyph and keeps
//! the boxes behind them, so the pixels where a frame and its plate differ are
//! the glyphs on screen. From them come each text node's runs on screen, the
//! reading time of a run, its contrast against the plate and its cue against
//! the transcript.

use super::{normalize, QaIssue, QaWord};
use crate::error::{ErrorCode, HostError};
use director_core::render_manifest::{build_render_manifest, IssueSeverity};
use director_core::types::NodeId;
use director_core::Director;
use director_render::{Frame, FrameSession, RenderError};
use director_scene::{Channel, NodeBody, NodeRecord, NodeRef, SceneDocument};
use std::collections::HashMap;
use std::sync::OnceLock;

/// A pixel belongs to a glyph when a channel moves by more than this against the plate.
const GLYPH_DELTA: u8 = 8;
/// A text node shows once this many of its pixels are glyphs.
const GLYPH_PIXELS: usize = 16;
/// Reading time per word in seconds, and the floor for any text.
const SECONDS_PER_WORD: f64 = 0.3;
const MIN_READING_SECONDS: f64 = 1.0;
/// Lowest contrast ratio the glyph pixels must reach at the 75th percentile.
const MIN_CONTRAST: f64 = 3.0;
/// A cue counts as late this many seconds after its first spoken word.
const CUE_LATE: f64 = 0.2;
/// A spoken word matches a cue within this many seconds of it.
const CUE_SEARCH: f64 = 3.0;

/// One text node the checks follow.
pub(super) struct Target {
    id: NodeId,
    text: String,
    /// Visibility windows from the node up to its root on the composition
    /// clock: `(start, end)` with `None` for an open end.
    windows: Vec<(f64, Option<f64>)>,
    /// Clips that place the node's scene root: `(start, end)`.
    clips: Vec<(f64, f64)>,
}

impl Target {
    fn active(&self, time: f64) -> bool {
        let clipped = self.clips.is_empty() || self.clips.iter().any(|(start, end)| time >= *start && time < *end);
        clipped && self.windows.iter().all(|(start, end)| time >= *start && end.is_none_or(|end| time < end))
    }

    /// Times where the gate may open or close.
    fn boundaries(&self) -> impl Iterator<Item = f64> + '_ {
        let windows = self.windows.iter().flat_map(|(start, end)| [Some(*start), *end]).flatten();
        windows.chain(self.clips.iter().flat_map(|(start, end)| [*start, *end]))
    }

    fn words(&self) -> usize {
        self.text.split_whitespace().filter(|word| word.chars().any(char::is_alphanumeric)).count()
    }
}

fn text_of(record: &NodeRecord) -> Option<String> {
    let body = match &record.body {
        NodeBody::Text(body) => body,
        NodeBody::Html(html) => match html.content.as_deref().map(|content| &content.body) {
            Some(NodeBody::Text(body)) => body,
            _ => return None,
        },
        _ => return None,
    };
    let text: String = body.spans.iter().map(|span| span.text.as_str()).collect();
    text.chars().any(char::is_alphanumeric).then(|| text.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// The text nodes of `document` and a plate copy of it with their glyphs
/// hidden, or `None` without text.
pub(super) fn targets(document: &SceneDocument) -> Option<(Vec<Target>, SceneDocument)> {
    let key = |id: NodeRef| (id.index, id.generation);
    let records: HashMap<(u32, u32), &NodeRecord> = document.nodes.iter().map(|record| (key(record.id), record)).collect();
    let mut targets = Vec::new();
    for record in &document.nodes {
        let Some(text) = text_of(record) else { continue };
        let mut chain = vec![record];
        while let Some(parent) = chain.last().and_then(|node| node.parent).and_then(|parent| records.get(&key(parent))) {
            chain.push(*parent);
        }
        let root = chain.last().map(|node| node.id);
        let (mut windows, mut mapped, mut hidden) = (Vec::new(), false, false);
        for node in chain.iter().rev() {
            if let NodeBody::Html(html) = &node.body {
                if !mapped {
                    hidden |= html.hidden;
                    windows.push((html.start, html.end));
                }
                mapped |= html.time_map.is_some();
            }
        }
        if hidden {
            continue;
        }
        let clips = document
            .timeline
            .tracks
            .iter()
            .filter(|track| track.enabled)
            .flat_map(|track| &track.clips)
            .filter(|clip| Some(clip.scene_root) == root)
            .map(|clip| (clip.start_time, clip.start_time + clip.duration))
            .collect();
        targets.push(Target {
            id: NodeId::new(record.id.index, record.id.generation),
            text,
            windows,
            clips,
        });
    }
    let mut plate = document.clone();
    for record in &mut plate.nodes {
        if text_of(record).is_none() {
            continue;
        }
        match &mut record.body {
            NodeBody::Html(html) => {
                if let Some(content) = html.content.as_deref_mut() {
                    content.opacity = Channel::still(0.0);
                }
            }
            _ => record.opacity = Channel::still(0.0),
        }
    }
    (!targets.is_empty()).then_some((targets, plate))
}

/// One painted time: the frame, which targets show and the glyph contrast of
/// each one that does.
struct Probe {
    frame: Frame,
    shown: Vec<bool>,
    contrast: Vec<Option<f64>>,
}

struct Prober<'a> {
    director: &'a mut Director,
    frames: FrameSession,
    plates: FrameSession,
    targets: Vec<Target>,
    /// Canvas size the manifest bounds use.
    canvas: (f32, f32),
    /// Visibility per probed time.
    seen: HashMap<u64, Vec<bool>>,
    /// Plates painted.
    plated: usize,
}

fn render_error(error: RenderError) -> HostError {
    HostError::new(ErrorCode::Render, error.to_string())
}

impl Prober<'_> {
    /// Paint `time`, plus its plate when a target is active there, and measure
    /// the glyph pixels each active target owns.
    fn probe(&mut self, time: f64) -> Result<Probe, HostError> {
        let frame = self.frames.render(time).map_err(render_error)?;
        let count = self.targets.len();
        let (mut shown, mut contrasts) = (vec![false; count], vec![None; count]);
        if self.targets.iter().any(|target| target.active(time)) {
            let manifest = build_render_manifest(self.director, time);
            let plate = self.plates.render(time).map_err(render_error)?;
            self.plated += 1;
            let (width, height) = (frame.width as usize, frame.height as usize);
            let bounds: HashMap<NodeId, _> = manifest.nodes.iter().map(|node| (node.id, node.bounds)).collect();
            let (sx, sy) = (width as f32 / self.canvas.0.max(1.0), height as f32 / self.canvas.1.max(1.0));
            // Each glyph pixel goes to the smallest active text box holding it.
            let mut boxes: Vec<(usize, [usize; 4], f32)> = Vec::new();
            for (index, target) in self.targets.iter().enumerate() {
                let Some(b) = bounds.get(&target.id).filter(|_| target.active(time)) else { continue };
                if !(b.w > 0.0 && b.h > 0.0 && [b.x, b.y, b.w, b.h].iter().all(|v| v.is_finite())) {
                    continue;
                }
                let x0 = (b.x * sx).floor().clamp(0.0, width as f32) as usize;
                let y0 = (b.y * sy).floor().clamp(0.0, height as f32) as usize;
                let x1 = ((b.x + b.w) * sx).ceil().clamp(0.0, width as f32) as usize;
                let y1 = ((b.y + b.h) * sy).ceil().clamp(0.0, height as f32) as usize;
                if x1 > x0 && y1 > y0 {
                    boxes.push((index, [x0, y0, x1, y1], b.w * b.h));
                }
            }
            boxes.sort_by(|a, b| b.2.total_cmp(&a.2));
            let mut owner = vec![u32::MAX; width * height];
            for (index, [x0, y0, x1, y1], _) in &boxes {
                for y in *y0..*y1 {
                    owner[y * width + x0..y * width + x1].fill(*index as u32);
                }
            }
            for (index, [x0, y0, x1, y1], _) in &boxes {
                let mut glyphs = Vec::new();
                for y in *y0..*y1 {
                    for x in *x0..*x1 {
                        let at = (y * width + x) * 4;
                        let moved = owner[y * width + x] == *index as u32
                            && frame.rgba[at..at + 4].iter().zip(&plate.rgba[at..at + 4]).any(|(p, q)| p.abs_diff(*q) > GLYPH_DELTA);
                        if moved {
                            glyphs.push(at);
                        }
                    }
                }
                shown[*index] = glyphs.len() >= GLYPH_PIXELS;
                contrasts[*index] = contrast(&frame, &plate, &glyphs);
            }
        }
        self.seen.insert(time.to_bits(), shown.clone());
        Ok(Probe {
            frame,
            shown,
            contrast: contrasts,
        })
    }

    fn visible(&mut self, time: f64, index: usize) -> Result<bool, HostError> {
        match self.seen.get(&time.to_bits()) {
            Some(shown) => Ok(shown[index]),
            None => Ok(self.probe(time)?.shown[index]),
        }
    }

    /// The first time in `(before, after]` where target `index` takes its
    /// state at `after`, to frame precision. A gate boundary in between
    /// settles the edge with one paint; otherwise the edge is bisected.
    fn edge(&mut self, index: usize, mut before: f64, mut after: f64, frame: f64) -> Result<f64, HostError> {
        let goal = self.visible(after, index)?;
        let target = &self.targets[index];
        let flips = target.boundaries().filter(|b| *b > before && *b <= after);
        if goal {
            // The latest opening of the gate: the text shows from there unless it fades in.
            let opening = flips.filter(|b| target.active(*b) && !target.active(*b - 1e-6)).max_by(f64::total_cmp);
            if let Some(opening) = opening {
                if self.visible(opening, index)? {
                    return Ok(opening);
                }
                before = opening;
            }
        } else {
            // The earliest closing of the gate: the text is gone there and usually shows one frame earlier.
            let closing = flips.filter(|b| !target.active(*b) && target.active(*b - 1e-6)).min_by(f64::total_cmp);
            if let Some(closing) = closing {
                if closing - frame <= before || self.visible(closing - frame, index)? {
                    return Ok(closing);
                }
                after = closing - frame;
            }
        }
        while after - before > frame {
            let mid = 0.5 * (before + after);
            if self.visible(mid, index)? == goal {
                after = mid;
            } else {
                before = mid;
            }
        }
        Ok(after)
    }
}

/// One stretch where a text node shows.
struct Run {
    start: f64,
    end: f64,
}

/// WCAG relative luminance of a straight-alpha pixel over black.
fn luminance(pixel: &[u8]) -> f64 {
    static LINEAR: OnceLock<[f64; 256]> = OnceLock::new();
    let table = LINEAR.get_or_init(|| {
        std::array::from_fn(|value| {
            let c = value as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        })
    });
    let alpha = pixel[3] as usize;
    let linear = |c: u8| table[(c as usize * alpha + 127) / 255];
    0.2126 * linear(pixel[0]) + 0.7152 * linear(pixel[1]) + 0.0722 * linear(pixel[2])
}

/// Contrast of the glyph pixels against the plate at the 75th percentile.
fn contrast(frame: &Frame, plate: &Frame, glyphs: &[usize]) -> Option<f64> {
    if glyphs.len() < GLYPH_PIXELS {
        return None;
    }
    let mut ratios: Vec<f64> = glyphs
        .iter()
        .map(|at| {
            let (a, b) = (luminance(&frame.rgba[*at..*at + 4]), luminance(&plate.rgba[*at..*at + 4]));
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        })
        .collect();
    ratios.sort_by(f64::total_cmp);
    Some(ratios[ratios.len() * 3 / 4])
}

/// Normalized words of a text.
fn tokens(text: &str) -> Vec<String> {
    text.split_whitespace().map(normalize).filter(|word| !word.is_empty()).collect()
}

/// The transcript says most of `text` while `run` shows.
fn voiced(text: &[String], run: &Run, words: &[QaWord]) -> bool {
    let spoken: Vec<String> = words.iter().filter(|word| word.start < run.end && word.end > run.start).map(|word| normalize(&word.text)).collect();
    let said = text.iter().filter(|token| spoken.contains(token)).count();
    !text.is_empty() && said * 2 >= text.len()
}

/// The spoken word that a cue's first words match, closest to the cue start.
fn cue_word<'a>(text: &[String], start: f64, words: &'a [QaWord]) -> Option<&'a QaWord> {
    let first = text.first()?;
    words
        .iter()
        .enumerate()
        .filter(|(at, word)| {
            let next = words.get(at + 1).map(|word| normalize(&word.text));
            normalize(&word.text) == *first && (word.start - start).abs() <= CUE_SEARCH && text.get(1).is_none_or(|second| next.is_none_or(|next| next == *second))
        })
        .map(|(_, word)| word)
        .min_by(|a, b| (a.start - start).abs().total_cmp(&(b.start - start).abs()))
}

/// Times the text pass paints: `steps`, plus the middle of every gate span
/// that is shorter than a step and holds no step, so short text shows up.
pub(super) fn scan_times(targets: &[Target], steps: &[f64]) -> Vec<f64> {
    let step = steps.windows(2).map(|pair| pair[1] - pair[0]).fold(f64::INFINITY, f64::min);
    let last = steps.last().copied().unwrap_or(0.0);
    let mut times = steps.to_vec();
    for target in targets {
        let start = target.windows.iter().map(|window| window.0).fold(f64::NEG_INFINITY, f64::max);
        let end = target.windows.iter().filter_map(|window| window.1).fold(f64::INFINITY, f64::min);
        let spans: Vec<(f64, f64)> = if target.clips.is_empty() {
            vec![(start, end)]
        } else {
            target.clips.iter().map(|(from, to)| (from.max(start), to.min(end))).collect()
        };
        for (from, to) in spans {
            let middle = 0.5 * (from + to);
            if to > from && to - from < step && (0.0..=last).contains(&middle) && !steps.iter().any(|time| (from..to).contains(time)) {
                times.push(middle);
            }
        }
    }
    times.sort_by(f64::total_cmp);
    times.dedup();
    times
}

/// Visibility runs, reading time, contrast and cue findings for `targets` of
/// `document`, painted on `sessions` (the document and its plate) at `times`
/// from [`scan_times`], with run edges refined to frame precision. Each
/// scanned frame goes to `scanned`, and each scanned time ticks `progress`.
#[allow(clippy::too_many_arguments)]
pub(super) fn text_issues(
    document: &SceneDocument,
    director: &mut Director,
    targets: Vec<Target>,
    sessions: (FrameSession, FrameSession),
    times: &[f64],
    words: &[QaWord],
    scanned: &mut dyn FnMut(f64, Frame),
    progress: &mut dyn FnMut(),
) -> Result<Vec<QaIssue>, HostError> {
    let frame = 1.0 / document.fps.max(1.0);
    let end = director.timeline_duration().max(0.0);
    let mut prober = Prober {
        director,
        frames: sessions.0,
        plates: sessions.1,
        targets,
        canvas: (document.width as f32, document.height as f32),
        seen: HashMap::new(),
        plated: 0,
    };
    let (mut shown, mut contrasts) = (Vec::with_capacity(times.len()), Vec::with_capacity(times.len()));
    for &time in times {
        let probe = prober.probe(time)?;
        shown.push(probe.shown);
        contrasts.push(probe.contrast);
        scanned(time, probe.frame);
        progress();
    }
    let plated = prober.plated;
    // Scanned sample ranges where each target shows.
    let mut spans = Vec::new();
    let columns: Vec<Vec<bool>> = (0..prober.targets.len()).map(|target| shown.iter().map(|row| row[target]).collect()).collect();
    for (target, column) in columns.iter().enumerate() {
        let mut open: Option<usize> = None;
        for at in 0..=times.len() {
            match (open, column.get(at).copied().unwrap_or(false)) {
                (None, true) => open = Some(at),
                (Some(first), false) => {
                    spans.push((target, first..at));
                    open = None;
                }
                _ => {}
            }
        }
    }

    let mut issues = Vec::new();
    for (index, samples) in &spans {
        let (first, at) = (samples.start, samples.end);
        // A run starts in (start_lo, start_hi] and ends in (end_lo, end_hi].
        let (start_lo, start_hi) = if first == 0 { (0.0, 0.0) } else { (times[first - 1], times[first]) };
        let (end_lo, end_hi) = if at == times.len() { (end, end) } else { (times[at - 1], times[at]) };
        let target = &prober.targets[*index];
        let text = tokens(&target.text);
        let needed = (SECONDS_PER_WORD * target.words() as f64).max(MIN_READING_SECONDS);
        let middle = 0.5 * (start_hi + end_lo);
        let nearest = |middle: f64| {
            samples
                .clone()
                .filter_map(|at| contrasts[at][*index].map(|ratio| (times[at], ratio)))
                .min_by(|a, b| (a.0 - middle).abs().total_cmp(&(b.0 - middle).abs()))
        };
        // Settle the run edges only when a check could fire on them.
        let short = end_lo - start_hi + 0.5 * frame < needed;
        let late = [start_lo, start_hi].iter().any(|start| cue_word(&text, *start, words).is_some_and(|word| start_hi - word.start > CUE_LATE));
        let faint = nearest(middle).is_some_and(|(_, ratio)| ratio < MIN_CONTRAST);
        if !(short || late || faint) {
            continue;
        }
        let start = if first == 0 { 0.0 } else { prober.edge(*index, start_lo, start_hi, frame)? };
        let stop = if at == times.len() { end } else { prober.edge(*index, end_lo, end_hi, frame)?.min(end) };
        let run = &Run { start, end: stop };
        let target = &prober.targets[*index];
        let quoted = quote(&target.text);
        let held = run.end - run.start;
        let mut push = |kind, detail: String, fix: String| {
            issues.push(QaIssue {
                kind,
                severity: IssueSeverity::Warning,
                node: Some(director_core::scene_export::node_ref(target.id)),
                name: None,
                start: run.start,
                end: run.end,
                count: 1,
                detail,
                fix: Some(fix),
            })
        };
        if held + 0.5 * frame < needed && !voiced(&text, run, words) {
            push(
                "reading_time",
                format!("{quoted} shows for {held:.2}s and needs {needed:.1}s to read"),
                format!("Hold the text for at least {needed:.1}s or cut it to fewer words"),
            );
        }
        if let Some(word) = cue_word(&text, run.start, words) {
            let late = run.start - word.start;
            if late > CUE_LATE {
                push(
                    "cue_late",
                    format!("{quoted} starts {late:.2}s after the voice says \"{}\" at {:.2}s", word.text, word.start),
                    format!("Start the text at {:.2}s", word.start),
                );
            }
        }
        // Contrast at the scanned time nearest the middle of the run.
        if let Some((time, ratio)) = nearest(run.start + 0.5 * held).filter(|(_, ratio)| *ratio < MIN_CONTRAST) {
            push(
                "contrast",
                format!("{quoted} has {ratio:.1}:1 contrast against the pixels behind it at {time:.2}s, under {MIN_CONTRAST}:1"),
                "Darken or lighten the backdrop, change the text color, or add a stroke or shadow".into(),
            );
        }
    }
    tracing::debug!(targets = prober.targets.len(), scanned = times.len(), plated, probes = prober.seen.len(), runs = spans.len(), "qa text pass");
    Ok(issues)
}

/// The text in quotes, cut to 40 characters.
fn quote(text: &str) -> String {
    let mut cut: String = text.chars().take(40).collect();
    if cut.len() < text.len() {
        cut.push_str("...");
    }
    format!("\"{cut}\"")
}

#[cfg(test)]
mod tests {
    use super::{cue_word, tokens, voiced, Run};
    use crate::qa::QaWord;

    fn word(text: &str, start: f64) -> QaWord {
        QaWord {
            text: text.into(),
            start,
            end: start + 0.3,
        }
    }

    #[test]
    fn cues_match_the_first_spoken_words_nearest_the_start() {
        let words = [word("Hello", 0.5), word("world", 0.8), word("hello", 4.0), word("there", 4.3)];
        assert_eq!(cue_word(&tokens("Hello, world!"), 1.0, &words).map(|w| w.start), Some(0.5));
        assert_eq!(cue_word(&tokens("hello there"), 1.0, &words).map(|w| w.start), Some(4.0));
        assert!(cue_word(&tokens("goodbye"), 1.0, &words).is_none());
        let run = Run { start: 0.4, end: 0.9 };
        assert!(voiced(&tokens("Hello world"), &run, &words));
        assert!(!voiced(&tokens("Hello world"), &Run { start: 2.0, end: 2.5 }, &words));
    }
}
