//! Subtitle cues from timed words, written as SRT or WebVTT.

use crate::Word;

/// When [`cues`] starts a new cue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CueOptions {
    /// Most characters on a cue, spaces included. A longer single word keeps its own cue.
    pub max_chars: usize,
    /// Longest cue in seconds.
    pub max_duration: f64,
    /// A pause of at least this many seconds between words ends the cue.
    pub max_gap: f64,
    /// A word ending in `.`, `?` or `!` ends the cue.
    pub break_on_sentence: bool,
}

impl Default for CueOptions {
    /// 42 characters, the broadcast line length, at most 5 s, ending at a 0.8 s pause or a
    /// sentence end.
    fn default() -> Self {
        Self { max_chars: 42, max_duration: 5.0, max_gap: 0.8, break_on_sentence: true }
    }
}

/// One subtitle: text shown from `start` to `end` seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Groups words into cues in order.
pub fn cues(words: &[Word], options: &CueOptions) -> Vec<Cue> {
    let mut out: Vec<Cue> = Vec::new();
    let mut open = false;
    for (i, word) in words.iter().enumerate() {
        if open {
            let cue = out.last_mut().unwrap();
            let fits = cue.text.chars().count() + 1 + word.text.chars().count() <= options.max_chars
                && word.end - cue.start <= options.max_duration
                && word.start - words[i - 1].end < options.max_gap;
            if fits {
                cue.text.push(' ');
                cue.text.push_str(&word.text);
                cue.end = word.end;
            } else {
                open = false;
            }
        }
        if !open {
            out.push(Cue { start: word.start, end: word.end, text: word.text.clone() });
            open = true;
        }
        if options.break_on_sentence && word.text.ends_with(['.', '?', '!']) {
            open = false;
        }
    }
    out
}

fn timestamp(seconds: f64, separator: char) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02}{separator}{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// SubRip text: numbered cues with `HH:MM:SS,mmm` times.
pub fn srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, timestamp(cue.start, ','), timestamp(cue.end, ','), cue.text));
    }
    out
}

/// WebVTT text with `HH:MM:SS.mmm` times and `&`, `<` and `>` escaped.
pub fn vtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for cue in cues {
        let text = cue.text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        out.push_str(&format!("{} --> {}\n{text}\n\n", timestamp(cue.start, '.'), timestamp(cue.end, '.')));
    }
    out
}
