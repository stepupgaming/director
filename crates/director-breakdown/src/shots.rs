//! Shot boundaries: hard cuts, fades through black or white, dissolves,
//! flashes and motion-graphics beats inside a shot. Ported from rea-video
//! `shots.py` `detect_shots` (Apache-2.0, see NOTICE). The check that a
//! dissolve candidate is really camera or subject motion uses block matching
//! on the thumbnails in place of Farneback optical flow.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::track::FrameTrack;

/// How a shot begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    /// The first shot.
    Start,
    Cut,
    /// Through an almost uniform black or white frame.
    Fade,
    /// A cross-fade: frames blend the shots on either side.
    Dissolve,
    /// A cut that opens on a bright flash decaying into the shot.
    Flash,
}

/// Detection thresholds.
#[derive(Debug, Clone, Copy)]
pub struct ShotOptions {
    /// Shots shorter than this merge into a neighbour, in seconds.
    pub min_shot_s: f64,
    /// Score a hard cut must exceed.
    pub cut_threshold: f64,
    /// Look for dissolves.
    pub dissolve: bool,
}

impl Default for ShotOptions {
    fn default() -> Self {
        Self { min_shot_s: 0.35, cut_threshold: 0.30, dissolve: true }
    }
}

/// One shot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Shot {
    /// 1-based.
    pub index: usize,
    /// Seconds.
    pub start: f64,
    pub end: f64,
    pub duration: f64,
    pub start_frame: usize,
    /// Exclusive.
    pub end_frame: usize,
    pub transition_in: Transition,
    /// Strength of the boundary that opens the shot.
    pub boundary_score: f64,
    /// Times of motion-graphics or slide beats inside the shot: a held layout changing to another.
    pub beats: Vec<f64>,
    /// How long a flash takes to decay, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flash_s: Option<f64>,
}

fn round_to(x: f64, places: i32) -> f64 {
    let p = 10f64.powi(places);
    (x * p).round() / p
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

/// Median of the `radius` neighbours on each side, leaving out the centre.
fn local_median(x: &[f64], radius: usize) -> Vec<f64> {
    let n = x.len();
    let mut window = Vec::with_capacity(2 * radius + 1);
    (0..n)
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(radius), (i + radius + 1).min(n));
            window.clear();
            if hi - lo > 1 {
                window.extend_from_slice(&x[lo..i]);
                window.extend_from_slice(&x[i + 1..hi]);
            } else {
                window.extend_from_slice(&x[lo..hi]);
            }
            median(&mut window)
        })
        .collect()
}

/// Time of a frame index; past the last pts, one median frame step per frame.
fn frame_time(times: Option<&[f64]>, fps: f64, frame: usize) -> f64 {
    let Some(times) = times else { return frame as f64 / fps };
    if frame < times.len() {
        return times[frame];
    }
    let step = if times.len() > 1 {
        let mut steps: Vec<f64> = times.windows(2).map(|w| w[1] - w[0]).collect();
        median(&mut steps)
    } else {
        1.0 / fps
    };
    times[times.len() - 1] + step * (frame - times.len() + 1) as f64
}

/// Detects the shots of a track.
pub fn detect_shots(track: &FrameTrack, options: &ShotOptions) -> Vec<Shot> {
    let n = track.n();
    if n == 0 {
        return Vec::new();
    }
    let fps = track.fps;
    let cut_threshold = options.cut_threshold;
    let mut score: Vec<f64> =
        track.hist_dist.iter().zip(&track.pix_diff).map(|(h, p)| 0.6 * h + 0.4 * (p * 4.0).clamp(0.0, 1.0)).collect();
    score[0] = 0.0;
    let radius = 4.max((fps * 0.5).round() as usize);
    let base = local_median(&score, radius);
    let mut boundaries: BTreeMap<usize, (Transition, f64)> = BTreeMap::new();

    // 1) Hard cuts: a spike well above local activity that changes most of the frame.
    let chg = &track.changed;
    let chg_base = local_median(chg, radius);
    for i in 1..n {
        let s = score[i];
        let strong = s > cut_threshold && s > base[i] * 2.5 + 0.06 && chg[i] > 0.25;
        let relative = chg[i] > 0.45 && chg[i] > chg_base[i] * 3.0 + 0.2 && s > base[i] * 4.0 + 0.03;
        // A one-frame swap between two digitally frozen layouts (slides, cards on one ground).
        let still_swap = s > cut_threshold
            && track.hist_dist[i] > 0.25
            && chg[i] > 0.18
            && chg_base[i] < 0.01
            && s > base[i] * 5.0 + 0.1
            && i >= 3
            && track.pix_diff[i - 3..i].iter().copied().fold(f64::MIN, f64::max) < 1e-3;
        if strong || relative || still_swap {
            boundaries.insert(i, (Transition::Cut, s.max(chg[i])));
        }
    }

    // 2) Fades through black or white: luma dips to an extreme over an almost uniform frame and comes back.
    let lo_l = 10f64.min(median(&mut track.luma.clone()) * 0.35);
    let dark: Vec<bool> = track.luma.iter().map(|&l| l < lo_l || l > 248.0).collect();
    let mut i = 0;
    while i < n {
        if !dark[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < n && dark[j] {
            j += 1;
        }
        let flat = track.thumb_std[i..j].iter().copied().fold(f64::MAX, f64::min) < 4.0;
        if i > 0 && j < n && (j - i) as f64 <= fps * 2.5 && flat && !boundaries.keys().any(|&b| j.abs_diff(b) < radius) {
            boundaries.insert(j, (Transition::Fade, 0.5));
        }
        i = j;
    }

    // 3) Dissolves: frame i is a linear blend of frames i - k and i + k, and the change is a burst
    //    that motion does not explain.
    if options.dissolve && n as f64 > 2.0 * fps {
        let mut ks: Vec<usize> = [(2, 0.12), (3, 0.2), (5, 0.45)].iter().map(|&(lo, f)| lo.max((fps * f).round() as usize)).collect();
        ks.sort_unstable();
        ks.dedup();
        for k in ks {
            let mut cand: Vec<(usize, f64)> = Vec::new();
            for i in k..n.saturating_sub(k) {
                if let Some(resid) = dissolve_residual(track, &score, i, k, cut_threshold) {
                    if !steady_morph(&track.thumbs, i, k)
                        && !motion_explains(&track.thumbs[i - k], &track.thumbs[i + k], track.width, track.height)
                    {
                        cand.push((i, resid));
                    }
                }
            }
            cand.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (i, resid) in cand {
                if boundaries.keys().all(|&b| i.abs_diff(b) > 2 * k) {
                    boundaries.insert(i, (Transition::Dissolve, round_to(1.0 - resid, 3)));
                }
            }
        }
    }

    let beat_marks = mg_beats(track, &boundaries, fps);

    // Assemble shots; a boundary that would leave a shot shorter than the minimum is skipped.
    let min_len = 1.max((options.min_shot_s * fps).round() as usize);
    let times = track.times.as_deref();
    let mut spans: Vec<(usize, usize, Transition, f64)> = Vec::new();
    let (mut start, mut trans, mut bscore) = (0, Transition::Start, 0.0);
    for b in boundaries.keys().copied().chain(std::iter::once(n)) {
        if b - start < min_len && b != n {
            continue;
        }
        if b == n && !spans.is_empty() && b - start < min_len {
            spans.last_mut().unwrap().1 = n;
            break;
        }
        spans.push((start, b, trans, bscore));
        if b < n {
            (trans, bscore) = boundaries[&b];
        }
        start = b;
    }
    spans
        .into_iter()
        .enumerate()
        .map(|(idx, (s, e, mut transition, boundary_score))| {
            let beats = beat_marks
                .iter()
                .filter(|&&b| s + min_len <= b && b + min_len <= e)
                .map(|&b| round_to(frame_time(times, fps, b), 3))
                .collect();
            let mut flash_s = None;
            if transition == Transition::Cut {
                let f = flash(&track.luma, s, e, fps);
                if f > 0.0 {
                    flash_s = Some(round_to(f, 2));
                    transition = Transition::Flash;
                }
            }
            let (t0, t1) = (frame_time(times, fps, s), frame_time(times, fps, e));
            Shot {
                index: idx + 1,
                start: round_to(t0, 3),
                end: round_to(t1, 3),
                duration: round_to(t1 - t0, 3),
                start_frame: s,
                end_frame: e,
                transition_in: transition,
                boundary_score: round_to(boundary_score, 3),
                beats,
                flash_s,
            }
        })
        .collect()
}

/// The blend residual of frame `i` against frames `i - k` and `i + k` when it qualifies as a dissolve frame.
fn dissolve_residual(track: &FrameTrack, score: &[f64], i: usize, k: usize, cut_threshold: f64) -> Option<f64> {
    let (a, b, m) = (&track.thumbs[i - k], &track.thumbs[i + k], &track.thumbs[i]);
    let thr = 6f64.max(0.35 * track.thumb_std[i - k].min(track.thumb_std[i + k]));
    let far = a.iter().zip(b).filter(|(&x, &y)| (y as f64 - x as f64).abs() > thr).count() as f64 / a.len() as f64;
    if far < 0.15 || score[i + 1 - k..i + k + 1].iter().copied().fold(f64::MIN, f64::max) >= cut_threshold {
        return None;
    }
    let (mut dd, mut md, mut ad) = (0f64, 0f64, 0f64);
    for ((&x, &y), &z) in a.iter().zip(b).zip(m) {
        let d = y as f64 - x as f64;
        dd += d * d;
        md += (z as f64 - x as f64) * d;
        ad += d.abs();
    }
    if dd <= 0.0 {
        return None;
    }
    let alpha = md / dd;
    if !(0.2..=0.8).contains(&alpha) {
        return None;
    }
    let len = a.len() as f64;
    let blend: f64 = a.iter().zip(b).zip(m).map(|((&x, &y), &z)| (z as f64 - (x as f64 + alpha * (y as f64 - x as f64))).abs()).sum();
    let resid = (blend / len) / (ad / len + 1e-6);
    (resid < 0.20).then_some(resid)
}

/// A cut that opens on a bright flash decaying into the new shot: seconds until the brightness settles, or 0.
fn flash(luma: &[f64], start: usize, end: usize, fps: f64) -> f64 {
    if start < 1 || end - start < 3 {
        return 0.0;
    }
    let before = median(&mut luma[start.saturating_sub(3)..start].to_vec());
    let later = &luma[start + 1..end];
    let take = 1.max((end - start - 1).min(fps as usize));
    let settle = median(&mut later[later.len() - take..].to_vec());
    let jump = luma[start] - before.max(settle);
    if jump < 40.0 {
        return 0.0;
    }
    let mut k = start;
    while k + 1 < end && luma[k + 1] - settle > 0.2 * jump {
        k += 1;
    }
    // One bright frame is a bright shot; a long ramp is a fade.
    if k == start || (k - start) as f64 > fps * 1.5 {
        return 0.0;
    }
    (k + 1 - start) as f64 / fps
}

fn changed_fraction(a: &[u8], b: &[u8], thr: f64) -> f64 {
    a.iter().zip(b).filter(|(&x, &y)| (x as f64 - y as f64).abs() > thr).count() as f64 / a.len().max(1) as f64
}

/// Holds of frozen frames at least 0.4 s long; consecutive holds whose pictures differ over a large
/// area are separate beats of a rendered (motion graphics or slide) video. Returns the frame that
/// ends the first hold of each such pair.
fn mg_beats(track: &FrameTrack, boundaries: &BTreeMap<usize, (Transition, f64)>, fps: f64) -> Vec<usize> {
    let n = track.n();
    let frozen: Vec<bool> = track.changed.iter().map(|&c| c < 0.004).collect();
    let min_run = 3.max((0.4 * fps).round() as usize);
    let mut runs = Vec::new();
    let mut i = 0;
    while i < n {
        if !frozen[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < n && frozen[j] {
            j += 1;
        }
        if j - i >= min_run {
            runs.push((i, j));
        }
        i = j;
    }
    runs.windows(2)
        .filter_map(|w| {
            let ((a0, a1), (b0, b1)) = (w[0], w[1]);
            if boundaries.keys().any(|&k| a1 - 1 <= k && k <= b0 + 1) {
                return None;
            }
            let (ia, ib) = ((a0 + a1) / 2, (b0 + b1) / 2);
            let thr = 6f64.max(0.35 * track.thumb_std[ia].min(track.thumb_std[ib]));
            (changed_fraction(&track.thumbs[ia], &track.thumbs[ib], thr) > 0.12).then_some(a1)
        })
        .collect()
}

/// True when the picture changes as fast just before and just after the window [i - k, i + k] as inside it.
fn steady_morph(thumbs: &[Vec<u8>], i: usize, k: usize) -> bool {
    let n = thumbs.len() as isize;
    let rate = |a: isize, b: isize| -> Option<f64> {
        let (a, b) = (a.max(0), b.min(n - 1));
        if b - a < k as isize {
            return None;
        }
        let (ta, tb) = (&thumbs[a as usize], &thumbs[b as usize]);
        let mean = ta.iter().zip(tb).map(|(&x, &y)| (y as f64 - x as f64).abs()).sum::<f64>() / ta.len() as f64;
        Some(mean * (2 * k) as f64 / (b - a) as f64)
    };
    let (i, k2) = (i as isize, k as isize);
    let Some(mid) = rate(i - k2, i + k2).filter(|&m| m > 0.0) else { return false };
    let around = [rate(i - 3 * k2, i - k2), rate(i + k2, i + 3 * k2)];
    let around: Vec<f64> = around.into_iter().flatten().collect();
    !around.is_empty() && mid < 1.3 * around.iter().copied().fold(f64::MIN, f64::max)
}

/// True when `b` is mostly a moved copy of `a` (pan, zoom, subject motion), so the change between them
/// is movement and no new content fades in. Each 8 px block of `a` finds its best match in `b` within a
/// search radius of an eighth of the width; motion explains the change when the matched residual falls
/// below 0.45 of the plain difference, both measured inside a 4 px border.
pub fn motion_explains(a: &[u8], b: &[u8], w: usize, h: usize) -> bool {
    const BLOCK: usize = 8;
    const MARGIN: usize = 4;
    const FLAT_RANGE: u8 = 6;
    if w <= 2 * MARGIN + BLOCK || h <= 2 * MARGIN + BLOCK || a.len() < w * h || b.len() < w * h {
        return false;
    }
    let reach = (w / 8).max(2) as isize;
    let at = |img: &[u8], x: isize, y: isize| img[y.clamp(0, h as isize - 1) as usize * w + x.clamp(0, w as isize - 1) as usize] as i32;
    let (mut before, mut after, mut count) = (0u64, 0u64, 0u64);
    for by in (0..h).step_by(BLOCK) {
        for bx in (0..w).step_by(BLOCK) {
            let (bw, bh) = (BLOCK.min(w - bx), BLOCK.min(h - by));
            let mut best = (u64::MAX, 0isize, 0isize);
            // Optical flow has no gradient to follow in a flat block and leaves it in place.
            let (mut lo, mut hi) = (u8::MAX, 0u8);
            for y in by..by + bh {
                for &v in &a[y * w + bx..y * w + bx + bw] {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
            }
            let reach = if hi - lo <= FLAT_RANGE { 0 } else { reach };
            for dy in -reach..=reach {
                for dx in -reach..=reach {
                    let mut sad = 0u64;
                    for y in by..by + bh {
                        for x in bx..bx + bw {
                            sad += (a[y * w + x] as i32 - at(b, x as isize + dx, y as isize + dy)).unsigned_abs() as u64;
                        }
                        if sad >= best.0 {
                            break;
                        }
                    }
                    if sad < best.0 {
                        best = (sad, dx, dy);
                    }
                }
            }
            for y in by..by + bh {
                for x in bx..bx + bw {
                    if x < MARGIN || y < MARGIN || x >= w - MARGIN || y >= h - MARGIN {
                        continue;
                    }
                    let av = a[y * w + x] as i32;
                    before += (av - b[y * w + x] as i32).unsigned_abs() as u64;
                    after += (av - at(b, x as isize + best.1, y as isize + best.2)).unsigned_abs() as u64;
                    count += 1;
                }
            }
        }
    }
    count > 0 && (after as f64) < 0.45 * before as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_median_skips_the_centre() {
        let x = [0.0, 0.0, 9.0, 0.0, 0.0];
        assert_eq!(local_median(&x, 2)[2], 0.0);
        assert_eq!(local_median(&[1.0, 3.0], 1), vec![3.0, 1.0]);
    }

    #[test]
    fn shifted_texture_is_motion_and_new_content_is_not() {
        let (w, h) = (96, 54);
        let tex = |x: usize, y: usize| ((x * 37 + y * 91 + (x * y) % 13 * 17) % 251) as u8;
        let a: Vec<u8> = (0..w * h).map(|i| tex(i % w + 20, i / w + 20)).collect();
        let shifted: Vec<u8> = (0..w * h).map(|i| tex(i % w + 23, i / w + 22)).collect();
        assert!(motion_explains(&a, &shifted, w, h));
        let other: Vec<u8> = (0..w * h).map(|i| ((i * 7919 + 13) % 256) as u8).collect();
        assert!(!motion_explains(&a, &other, w, h));
    }
}
