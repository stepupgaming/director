//! The composition's beat grid and beat-valued times.
//!
//! The grid comes from the root's `data-bpm`, `data-beat-offset` and
//! `data-beats-per-bar`, or from the music track: an `audio` element with
//! `data-timeline-role="music"`, or else one whose id names music, bgm or a
//! soundtrack. It resolves on first use, so pages that never name a beat
//! never analyse their music.

use crate::assets::AssetStore;
use director_core::beats::{BeatGrid, BeatOptions};
use scraper::ElementRef;
use std::cell::{OnceCell, RefCell};
use std::sync::Arc;

pub(crate) struct Beats {
    source: Source,
    grid: OnceCell<Result<BeatGrid, String>>,
    /// The music decoded for analysis, kept for the audio element that plays it.
    decoded: RefCell<Option<(String, Vec<f32>)>>,
}

#[derive(Clone)]
enum Source {
    None,
    Fixed(BeatGrid),
    Music {
        src: String,
        /// Composition seconds minus music file seconds.
        shift: f64,
        options: BeatOptions,
        assets: Arc<AssetStore>,
    },
}

/// One `+` or `-` term of a beat time.
enum Term {
    Beats(f64),
    Bars(f64),
    Drop,
}

const SYNTAX: &str = "write beat times as terms like 8bar, 2b or drop joined by + or -";

/// The terms of `text`, or `None` when it names no beat unit.
fn terms(text: &str) -> Result<Option<Vec<(f64, Term)>>, String> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    let mut sign = 1.0;
    let mut named = false;
    let mut first = true;
    loop {
        if let Some(r) = rest.strip_prefix('-').filter(|_| first) {
            sign = -1.0;
            rest = r.trim_start();
        }
        let number_end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
        let (number, after) = rest.split_at(number_end);
        let after = after.trim_start();
        let unit_end = after.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(after.len());
        let (unit, after) = after.split_at(unit_end);
        let value = number.parse::<f64>();
        let term = match (unit, value) {
            ("drop", _) if number.is_empty() => Some(Term::Drop),
            ("b" | "beat" | "beats", Ok(n)) => Some(Term::Beats(n)),
            ("bar" | "bars", Ok(n)) => Some(Term::Bars(n)),
            _ => None,
        };
        named |= term.is_some();
        out.push((sign, term));
        rest = after.trim_start();
        first = false;
        match rest.chars().next() {
            None => break,
            Some('+') => sign = 1.0,
            Some('-') => sign = -1.0,
            Some(_) if named => return Err(format!("'{text}': {SYNTAX}")),
            Some(_) => return Ok(None),
        }
        rest = rest[1..].trim_start();
    }
    if !named {
        return Ok(None);
    }
    out.into_iter()
        .map(|(sign, term)| term.map(|t| (sign, t)).ok_or_else(|| format!("'{text}': {SYNTAX}")))
        .collect::<Result<_, _>>()
        .map(Some)
}

impl Beats {
    /// The grid source `root` declares.
    pub(crate) fn new(root: ElementRef<'_>, assets: &Arc<AssetStore>) -> Result<Self, String> {
        let attr = |name: &str| -> Result<Option<f64>, String> {
            root.value()
                .attr(name)
                .map(|v| v.trim().parse::<f64>().map_err(|_| format!("{name} must be numeric (got '{v}')")))
                .transpose()
        };
        let bpm = attr("data-bpm")?;
        let offset = attr("data-beat-offset")?;
        let beats_per_bar = match attr("data-beats-per-bar")? {
            None => 4,
            Some(n) if n.fract() == 0.0 && (1.0..=32.0).contains(&n) => n as u32,
            Some(n) => return Err(format!("data-beats-per-bar must be a whole number 1..32 (got {n})")),
        };
        let music = root.descendants().filter_map(ElementRef::wrap).filter(|el| el.value().name() == "audio");
        let music: Vec<_> = music.collect();
        let track = music
            .iter()
            .find(|el| el.value().attr("data-timeline-role") == Some("music"))
            .or_else(|| {
                music.iter().find(|el| {
                    el.value().attr("id").is_some_and(|id| {
                        let id = id.to_ascii_lowercase();
                        ["music", "bgm", "soundtrack"].iter().any(|word| id.contains(word))
                    })
                })
            });
        let source = match (bpm, offset, track) {
            (None, Some(_), _) => return Err("data-beat-offset needs data-bpm".into()),
            (Some(bpm), Some(offset), _) => Source::Fixed(BeatGrid::fixed(bpm, offset, beats_per_bar)?),
            (bpm, None, Some(track)) => {
                let seconds = |name: &str| -> Result<f64, String> {
                    track
                        .value()
                        .attr(name)
                        .map_or(Ok(0.0), |v| v.trim().parse().map_err(|_| format!("the music track's {name} must be seconds")))
                };
                Source::Music {
                    src: track.value().attr("src").ok_or("the music track has no src")?.to_string(),
                    shift: seconds("data-start")? - seconds("data-media-start")?,
                    options: BeatOptions { beats_per_bar, bpm, ..Default::default() },
                    assets: assets.clone(),
                }
            }
            (Some(bpm), None, None) => Source::Fixed(BeatGrid::fixed(bpm, 0.0, beats_per_bar)?),
            (None, None, None) => Source::None,
        };
        Ok(Self {
            source,
            grid: OnceCell::new(),
            decoded: RefCell::new(None),
        })
    }

    /// A copy for another script run, keeping the grid analysed so far.
    pub(crate) fn fork(&self) -> Self {
        let grid = OnceCell::new();
        if let Some(g) = self.grid.get() {
            let _ = grid.set(g.clone());
        }
        Self { source: self.source.clone(), grid, decoded: RefCell::new(None) }
    }

    /// The grid, analysing the music on first use.
    pub(crate) fn grid(&self) -> Result<&BeatGrid, String> {
        self.grid
            .get_or_init(|| match &self.source {
                Source::None => Err("beat times need data-bpm on the composition root or a music track (an audio element with data-timeline-role=\"music\")".into()),
                Source::Fixed(grid) => Ok(grid.clone()),
                Source::Music { src, shift, options, assets } => {
                    let bytes = assets.read(src).map_err(|e| e.to_string())?;
                    let samples = director_core::audio::load_audio_bytes(&bytes, 48000)
                        .map_err(|e| format!("music '{src}': {e}"))?;
                    let mut grid = director_core::beats::detect(&samples, 2, 48000, options)
                        .map_err(|e| format!("music '{src}': {e}"))?;
                    for beat in &mut grid.beats {
                        *beat += shift;
                    }
                    *self.decoded.borrow_mut() = Some((src.clone(), samples));
                    Ok(grid)
                }
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// The grid when something already resolved it.
    pub(crate) fn resolved(&self) -> Option<&BeatGrid> {
        self.grid.get().and_then(|grid| grid.as_ref().ok())
    }

    /// The decoded samples of `src` when it is the analysed music.
    pub(crate) fn take_decoded(&self, src: &str) -> Option<Vec<f32>> {
        let mut decoded = self.decoded.borrow_mut();
        decoded.as_ref().is_some_and(|(s, _)| s == src).then(|| decoded.take().unwrap().1)
    }

    /// The beat position `text` names, or `None` when it names no beat unit.
    pub(crate) fn position(&self, text: &str) -> Result<Option<f64>, String> {
        let Some(terms) = terms(text)? else { return Ok(None) };
        let grid = self.grid()?;
        let mut total = 0.0;
        for (sign, term) in terms {
            total += sign
                * match term {
                    Term::Beats(n) => n,
                    Term::Bars(n) => n * grid.beats_per_bar as f64,
                    Term::Drop => grid.drop.ok_or("the music has no drop: no bar raises its energy enough over the bars before it")?,
                };
        }
        Ok(Some(total))
    }

    /// Composition seconds at the beat time `text`.
    pub(crate) fn seconds(&self, text: &str) -> Result<Option<f64>, String> {
        self.position(text)?.map(|p| self.grid().map(|g| g.time(p))).transpose()
    }

    /// The end of a beat-valued span of `text` beats and bars from `start` seconds.
    pub(crate) fn span_end(&self, start: f64, text: &str) -> Result<Option<f64>, String> {
        if terms(text)?.is_some_and(|terms| terms.iter().any(|(_, t)| matches!(t, Term::Drop))) {
            return Err(format!("'{text}': a duration counts beats and bars; drop names a moment"));
        }
        let Some(span) = self.position(text)? else { return Ok(None) };
        let grid = self.grid()?;
        Ok(Some(grid.time(grid.position(start) + span)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed() -> Beats {
        Beats {
            source: Source::Fixed(BeatGrid { drop: Some(16.0), ..BeatGrid::fixed(120.0, 0.25, 4).unwrap() }),
            grid: OnceCell::new(),
            decoded: RefCell::new(None),
        }
    }

    #[test]
    fn beat_times_sum_beats_bars_and_the_drop() {
        let beats = fixed();
        assert_eq!(beats.seconds("8b").unwrap(), Some(4.25));
        assert_eq!(beats.seconds("2bar + 1.5 beats").unwrap(), Some(5.0));
        assert_eq!(beats.seconds("drop - 1bar").unwrap(), Some(6.25));
        assert_eq!(beats.seconds("-1b").unwrap(), Some(-0.25));
        assert_eq!(beats.span_end(1.0, "2b").unwrap(), Some(2.0));
        for seconds in ["1.5", "0", "2s", "300ms"] {
            assert_eq!(beats.seconds(seconds).unwrap(), None, "{seconds}");
        }
        for bad in ["8bar + 2s", "8 bar*2", "drop2", "8bars beats"] {
            assert!(beats.seconds(bad).is_err(), "{bad}");
        }
        assert!(beats.span_end(0.0, "drop").is_err());
    }
}
