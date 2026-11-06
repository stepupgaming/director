//! Frame-to-frame feature tracks over a whole shot.

use rayon::prelude::*;

use crate::features::corners;
use crate::image::{Gray, Pyramid};
use crate::klt;

/// Pyramid levels, enough for about 50 pixels of motion between frames.
const LEVELS: usize = 4;
/// Tracks kept alive at once.
const MAX_TRACKS: usize = 500;
/// Minimum spacing of tracked corners in pixels.
const MIN_DISTANCE: f32 = 10.0;
/// A track survives only if tracking back lands this close to where it started.
const ROUND_TRIP: f32 = 0.5;
/// Pixels kept clear of the border.
const MARGIN: usize = 8;
/// The affine match against the first appearance may move a chained step
/// at most this many pixels.
const CORRECTION: f32 = 1.5;
/// The affine match must leave a mean absolute difference below this.
const MISMATCH: f32 = 10.0;

/// What a live track carries between frames: its first appearance and the
/// affine warp that last matched it.
struct Live {
    track: usize,
    template: Vec<f32>,
    warp: klt::Warp,
}

/// A feature followed through consecutive frames from `start`.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub start: usize,
    pub points: Vec<[f32; 2]>,
}

impl Track {
    /// The frame after the last one this track holds.
    pub fn end(&self) -> usize {
        self.start + self.points.len()
    }

    /// The track's position at `frame`, if it holds one.
    pub fn at(&self, frame: usize) -> Option<[f32; 2]> {
        frame.checked_sub(self.start).and_then(|i| self.points.get(i)).copied()
    }
}

/// Follows corners through `frames`. Each step tracks every live feature
/// forward and back and drops those whose round trip misses by more than
/// half a pixel, then corrects the step with an affine match against the
/// track's first appearance, which keeps long tracks from drifting. New
/// corners are seeded where the live ones thinned out. Tracks shorter than
/// 3 frames are dropped.
pub fn track(frames: &[Gray]) -> Vec<Track> {
    let Some(first) = frames.first() else { return Vec::new() };
    let mut tracks: Vec<Track> = Vec::new();
    let mut live: Vec<Live> = Vec::new();
    let mut prev = Pyramid::new(first, LEVELS);
    seed(&prev, 0, &mut tracks, &mut live);
    for (k, frame) in frames.iter().enumerate().skip(1) {
        let next = Pyramid::new(frame, LEVELS);
        let moved: Vec<Option<klt::Warp>> = live
            .par_iter()
            .map(|l| {
                let p = *tracks[l.track].points.last().expect("a live track has a point");
                let q = klt::track(&prev, &next, p, p)?;
                let back = klt::track(&next, &prev, q, p)?;
                if (back[0] - p[0]).powi(2) + (back[1] - p[1]).powi(2) >= ROUND_TRIP * ROUND_TRIP {
                    return None;
                }
                let guess = [l.warp[0], l.warp[1], l.warp[2], l.warp[3], q[0], q[1]];
                let (warp, mismatch) = klt::align(&l.template, &next.levels[0], guess)?;
                let shift = (warp[4] - q[0]).powi(2) + (warp[5] - q[1]).powi(2);
                (shift < CORRECTION * CORRECTION && mismatch < MISMATCH && inside(&next, [warp[4], warp[5]])).then_some(warp)
            })
            .collect();
        let mut kept = Vec::with_capacity(live.len());
        for (mut l, warp) in live.into_iter().zip(moved) {
            if let Some(warp) = warp {
                tracks[l.track].points.push([warp[4], warp[5]]);
                l.warp = warp;
                kept.push(l);
            }
        }
        live = kept;
        seed(&next, k, &mut tracks, &mut live);
        prev = next;
    }
    tracks.retain(|t| t.points.len() >= 3);
    tracks
}

fn inside(pyramid: &Pyramid, p: [f32; 2]) -> bool {
    let image = &pyramid.levels[0].image;
    let m = MARGIN as f32;
    p[0] >= m && p[1] >= m && p[0] <= (image.width - 1 - MARGIN) as f32 && p[1] <= (image.height - 1 - MARGIN) as f32
}

/// Starts new tracks at frame `k` away from the live ones.
fn seed(pyramid: &Pyramid, k: usize, tracks: &mut Vec<Track>, live: &mut Vec<Live>) {
    if live.len() >= MAX_TRACKS * 9 / 10 {
        return;
    }
    let level = &pyramid.levels[0];
    let existing: Vec<[f32; 2]> = live.iter().map(|l| *tracks[l.track].points.last().expect("a point")).collect();
    for p in corners(&level.image, &level.gx, &level.gy, &existing, MAX_TRACKS - live.len(), MIN_DISTANCE, MARGIN) {
        live.push(Live { track: tracks.len(), template: klt::template(level, p), warp: [0.0, 0.0, 0.0, 0.0, p[0], p[1]] });
        tracks.push(Track { start: k, points: vec![p] });
    }
}
