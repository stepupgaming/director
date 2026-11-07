use director_asr::chunk::{stitch, windows};
use director_asr::{Window, WindowOptions, SAMPLE_RATE};

use crate::word;

const RATE: usize = SAMPLE_RATE as usize;

/// Deterministic "speech": noise bursts of 0.3 s with 0.15 s gaps, and a 0.7 s pause every
/// 7 s, over `seconds`.
fn speech(seconds: usize) -> Vec<f32> {
    let mut state = 0x2545_f491u32;
    let mut out = vec![0f32; seconds * RATE];
    let mut t = 0usize;
    let mut n = 0;
    while t < out.len() {
        let burst = (RATE * 3 / 10).min(out.len() - t);
        for s in &mut out[t..t + burst] {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *s = (state as f32 / u32::MAX as f32 - 0.5) * 0.6;
        }
        n += 1;
        t += burst + if n % 14 == 0 { RATE * 7 / 10 } else { RATE * 15 / 100 };
    }
    out
}

#[test]
fn short_audio_is_one_window() {
    let samples = speech(30);
    assert_eq!(windows(&samples, &WindowOptions::default()), vec![Window { start: 0, end: samples.len(), keep_start: 0, keep_end: samples.len() }]);
}

#[test]
fn long_audio_splits_in_pauses_with_overlap() {
    let samples = speech(200);
    let options = WindowOptions::default();
    let ws = windows(&samples, &options);
    assert!(ws.len() >= 4, "{} windows", ws.len());
    assert_eq!(ws[0].keep_start, 0);
    assert_eq!(ws.last().unwrap().keep_end, samples.len());
    let half = (options.overlap * RATE as f64) as usize / 2;
    for pair in ws.windows(2) {
        assert_eq!(pair[0].keep_end, pair[1].keep_start, "keep ranges tile the recording");
        let cut = pair[0].keep_end;
        assert_eq!(pair[0].end, cut + half);
        assert_eq!(pair[1].start, cut - half);
        let around = &samples[cut - RATE / 20..cut + RATE / 20];
        assert!(around.iter().all(|s| *s == 0.0), "cut at {:.2} s lands in speech", cut as f64 / RATE as f64);
    }
    for w in &ws {
        assert!(w.end - w.start <= (options.max_len * RATE as f64) as usize, "{w:?} is too long");
    }
}

#[test]
fn stitching_keeps_every_word_once_in_order() {
    let samples = speech(200);
    let ws = windows(&samples, &WindowOptions::default());
    let total = samples.len() as f64 / RATE as f64;
    let all: Vec<_> = (0..)
        .map(|i| i as f64 * 0.45)
        .take_while(|t| t + 0.3 <= total)
        .enumerate()
        .map(|(i, t)| word(&format!("w{i}"), t, t + 0.3))
        .collect();
    // Each window hears the words that lie wholly inside it, as a model would.
    let parts = ws
        .iter()
        .map(|w| {
            let (lo, hi) = (w.start as f64 / RATE as f64, w.end as f64 / RATE as f64);
            (*w, all.iter().filter(|x| x.start >= lo && x.end <= hi).cloned().collect())
        })
        .collect();
    assert_eq!(stitch(parts), all);
}

#[test]
fn window_offset_is_its_start_in_seconds() {
    let w = Window { start: 48_000, end: 96_000, keep_start: 64_000, keep_end: 96_000 };
    assert_eq!(w.offset(), 3.0);
}
