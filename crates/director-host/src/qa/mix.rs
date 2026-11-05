//! Mix checks on 10 ms mono levels: dead stops of a steady bed, the voice
//! against its bed under speech, and voiceless stretches louder than the voice.

use super::{QaIssue, QaWord};
use director_core::render_manifest::IssueSeverity;

/// Seconds per level step.
const STEP: f64 = 0.01;
/// Level of digital silence in dB.
const FLOOR: f64 = -120.0;
/// A bed counts as steady when its 100 ms level stays within this range over a second.
const STEADY_RANGE: f64 = 6.0;
/// Quietest bed a dead stop can cut, in dB.
const BED_FLOOR: f64 = -50.0;
/// Drop below the bed that counts as a stop, in dB.
const STOP_DROP: f64 = 12.0;
/// Steps the level stays down after a stop.
const STOP_HOLD: usize = 30;
/// Speech peaks must sit this far above the bed, in dB.
const VOICE_OVER_BED: f64 = 12.0;
/// Seconds of padding around each word that count as voice.
const VOICE_PAD: f64 = 0.5;
/// A voiceless stretch counts from this many seconds.
const VOICELESS_SECONDS: f64 = 1.0;
/// Allowed rise of a voiceless stretch over the voice median, in dB.
const VOICELESS_RISE: f64 = 3.0;

/// RMS level of the mono downmix per 10 ms step, in dB.
fn levels(stereo: &[f32], rate: u32) -> Vec<f64> {
    let frames = ((rate as f64 * STEP).round() as usize).max(1);
    stereo
        .chunks(frames * 2)
        .map(|chunk| {
            let power = chunk.chunks(2).map(|pair| (pair.iter().map(|s| *s as f64).sum::<f64>() / pair.len() as f64).powi(2)).sum::<f64>();
            let power = power / (chunk.len() / 2).max(1) as f64;
            if power > 0.0 { (10.0 * power.log10()).max(FLOOR) } else { FLOOR }
        })
        .collect()
}

/// Trailing power average over `width` steps, in dB.
fn trailing(levels: &[f64], width: usize) -> Vec<f64> {
    let power: Vec<f64> = levels.iter().map(|db| 10f64.powf(db / 10.0)).collect();
    let mut sum = 0.0;
    (0..levels.len())
        .map(|at| {
            sum += power[at];
            if at >= width {
                sum -= power[at - width];
            }
            let mean = sum.max(0.0) / (at + 1).min(width) as f64;
            if mean > 0.0 { (10.0 * mean.log10()).max(FLOOR) } else { FLOOR }
        })
        .collect()
}

fn percentile(values: &mut [f64], fraction: f64) -> f64 {
    values.sort_by(f64::total_cmp);
    values[((values.len() as f64 * fraction) as usize).min(values.len() - 1)]
}

/// Steps inside a word, padded by `pad` seconds.
fn voice_mask(words: &[QaWord], steps: usize, pad: f64) -> Vec<bool> {
    let mut mask = vec![false; steps];
    for word in words {
        let from = ((word.start - pad) / STEP).floor().max(0.0) as usize;
        let to = (((word.end + pad) / STEP).ceil().max(0.0) as usize).min(steps);
        if from < to {
            mask[from..to].fill(true);
        }
    }
    mask
}

/// Times where a steady bed drops by [`STOP_DROP`] within two steps and stays
/// down, with the bed and the level after.
fn dead_stops(levels: &[f64]) -> Vec<(f64, f64, f64)> {
    let smooth = trailing(levels, 10);
    let mut stops = Vec::new();
    let mut at = 110;
    while at + STOP_HOLD + 1 < levels.len() {
        let before = &smooth[at - 100..at];
        let bed = before.iter().copied().fold(f64::INFINITY, f64::min);
        let top = before.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let after = &levels[at + 1..at + 1 + STOP_HOLD];
        if top - bed <= STEADY_RANGE && bed > BED_FLOOR && levels[at - 1] > bed - STEADY_RANGE && after.iter().all(|db| *db < bed - STOP_DROP) {
            stops.push((at as f64 * STEP, bed, after.iter().copied().fold(FLOOR, f64::max)));
            at += 100;
        } else {
            at += 1;
        }
    }
    stops
}

/// Ranges where speech peaks sit less than [`VOICE_OVER_BED`] above the bed,
/// with the smallest spread: 6 s windows, 3 s apart, at least 70% voice.
fn voice_over_bed(levels: &[f64], words: &[QaWord]) -> Vec<(f64, f64, f64)> {
    let voice = voice_mask(words, levels.len(), VOICE_PAD);
    let window = 600.min(levels.len());
    if window < 100 {
        return Vec::new();
    }
    let mut ranges: Vec<(f64, f64, f64)> = Vec::new();
    for from in (0..=levels.len() - window).step_by(300) {
        let to = from + window;
        if voice[from..to].iter().filter(|on| **on).count() * 10 < window * 7 {
            continue;
        }
        let mut sorted = levels[from..to].to_vec();
        let spread = percentile(&mut sorted, 0.9) - percentile(&mut sorted, 0.03);
        if spread >= VOICE_OVER_BED {
            continue;
        }
        let (start, end) = (from as f64 * STEP, to as f64 * STEP);
        match ranges.last_mut() {
            Some(last) if start <= last.1 => (last.1, last.2) = (end, last.2.min(spread)),
            _ => ranges.push((start, end, spread)),
        }
    }
    ranges
}

/// Voiceless stretches of [`VOICELESS_SECONDS`] or more whose 400 ms level
/// passes the voice median by [`VOICELESS_RISE`], with the level and the median.
fn voiceless_loud(levels: &[f64], words: &[QaWord]) -> Vec<(f64, f64, f64, f64)> {
    let momentary = trailing(levels, 40);
    let speech = voice_mask(words, levels.len(), 0.0);
    let mut spoken: Vec<f64> = momentary.iter().zip(&speech).filter(|(_, on)| **on).map(|(db, _)| *db).collect();
    if spoken.is_empty() {
        return Vec::new();
    }
    let median = percentile(&mut spoken, 0.5);
    let voice = voice_mask(words, levels.len(), VOICE_PAD);
    let mut stretches = Vec::new();
    let mut from = None;
    for at in 0..=levels.len() {
        let quiet = at < levels.len() && !voice[at];
        match (from, quiet) {
            (None, true) => from = Some(at),
            (Some(start), false) => {
                from = None;
                if ((at - start) as f64) * STEP < VOICELESS_SECONDS {
                    continue;
                }
                let level = percentile(&mut momentary[start..at].to_vec(), 0.9);
                if level > median + VOICELESS_RISE {
                    stretches.push((start as f64 * STEP, at as f64 * STEP, level, median));
                }
            }
            _ => {}
        }
    }
    stretches
}

/// Dead stops, and with a transcript the voice against its bed and loud
/// voiceless stretches, for a finite stereo mix.
pub(super) fn mix_issues(stereo: &[f32], rate: u32, words: &[QaWord], report: &mut Vec<QaIssue>) {
    let levels = levels(stereo, rate);
    let mut push = |kind, start, end, detail: String, fix: String| {
        report.push(QaIssue {
            kind,
            severity: IssueSeverity::Warning,
            node: None,
            name: None,
            start,
            end,
            count: 1,
            detail,
            fix: Some(fix),
        })
    };
    for (time, bed, after) in dead_stops(&levels) {
        let detail = format!("a steady {bed:.0} dB bed stops dead at {time:.2}s and falls to {after:.0} dB within 20 ms");
        push("dead_stop", time, time, detail, "Fade the audio out over at least 0.3s".into());
    }
    if words.is_empty() {
        return;
    }
    for (start, end, spread) in voice_over_bed(&levels, words) {
        let detail = format!("speech peaks sit {spread:.1} dB above the bed from {start:.2}s to {end:.2}s, under {VOICE_OVER_BED} dB");
        push("voice_bed", start, end, detail, format!("Lower or duck the music under the voice by {:.1} dB", VOICE_OVER_BED - spread));
    }
    for (start, end, level, median) in voiceless_loud(&levels, words) {
        let detail = format!("the mix reaches {level:.1} dB with no voice from {start:.2}s to {end:.2}s, above the {median:.1} dB voice median");
        push("voiceless_loud", start, end, detail, format!("Lower the audio between the lines by {:.1} dB", level - median));
    }
}

#[cfg(test)]
mod tests {
    use super::{dead_stops, levels, voice_over_bed, voiceless_loud};
    use crate::qa::QaWord;

    const RATE: u32 = 48_000;

    /// Stereo sine at `amplitude` for `seconds`.
    fn tone(seconds: f64, amplitude: f32, hz: f32) -> Vec<f32> {
        let frames = (seconds * RATE as f64) as usize;
        (0..frames)
            .flat_map(|i| {
                let s = amplitude * (std::f32::consts::TAU * hz * i as f32 / RATE as f32).sin();
                [s, s]
            })
            .collect()
    }

    /// A syllable-like voice: 200 ms bursts at `amplitude` with 100 ms gaps,
    /// over a bed tone at `bed`, and one word per burst.
    fn voice(seconds: f64, amplitude: f32, bed: f32) -> (Vec<f32>, Vec<QaWord>) {
        let mut mix = tone(seconds, bed, 110.0);
        let mut words = Vec::new();
        let mut start = 0.0;
        while start + 0.2 <= seconds {
            let from = (start * RATE as f64) as usize * 2;
            for (at, s) in tone(0.2, amplitude, 220.0).into_iter().enumerate() {
                mix[from + at] += s;
            }
            words.push(QaWord {
                text: "la".into(),
                start,
                end: start + 0.2,
            });
            start += 0.3;
        }
        (mix, words)
    }

    #[test]
    fn a_bed_cut_dead_is_a_stop_and_a_fade_is_not() {
        let mut cut = tone(2.0, 0.3, 220.0);
        cut.extend(vec![0.0; RATE as usize * 2]);
        let stops = dead_stops(&levels(&cut, RATE));
        assert_eq!(stops.len(), 1, "{stops:?}");
        assert!((stops[0].0 - 2.0).abs() < 0.03, "{stops:?}");

        let mut faded = tone(2.0, 0.3, 220.0);
        let fade = RATE as usize / 2;
        for (i, s) in tone(0.5, 0.3, 220.0).into_iter().enumerate() {
            faded.push(s * (1.0 - (i / 2) as f32 / fade as f32));
        }
        faded.extend(vec![0.0; RATE as usize * 2]);
        assert!(dead_stops(&levels(&faded, RATE)).is_empty());
    }

    #[test]
    fn music_under_speech_needs_room_and_voiceless_stretches_stay_under_the_voice() {
        let (loud, words) = voice(8.0, 0.3, 0.2);
        let crowded = voice_over_bed(&levels(&loud, RATE), &words);
        assert!(!crowded.is_empty() && crowded[0].2 < 12.0, "{crowded:?}");
        let (ducked, words) = voice(8.0, 0.3, 0.01);
        assert!(voice_over_bed(&levels(&ducked, RATE), &words).is_empty());

        // Voice for 4 s, then 3 s of music louder than the voice.
        let (mut mix, words) = voice(4.0, 0.2, 0.0);
        mix.extend(tone(3.0, 0.5, 110.0));
        let swells = voiceless_loud(&levels(&mix, RATE), &words);
        assert_eq!(swells.len(), 1, "{swells:?}");
        let (mut mix, words) = voice(4.0, 0.2, 0.0);
        mix.extend(tone(3.0, 0.05, 110.0));
        assert!(voiceless_loud(&levels(&mix, RATE), &words).is_empty());
    }
}
