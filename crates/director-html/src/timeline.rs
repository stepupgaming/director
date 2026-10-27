//! `director-timeline/1`: keyframe tracks over authored HTML elements. A
//! document carries them in `<script type="application/director-timeline+json">`
//! and each track compiles to a native animation on the element whose `id`
//! matches its target.

use crate::CompileError;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const TIMELINE_FORMAT: &str = "director-timeline/1";
pub const TIMELINE_SCRIPT: &str = "application/director-timeline+json";

/// Motion over HTML visuals, one track per element property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timeline {
    pub format: String,
    pub tracks: Vec<TimelineTrack>,
}

/// Keyframes for one animatable property of the element with HTML id `target`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimelineTrack {
    pub target: String,
    pub property: String,
    pub keyframes: Vec<TimelineKeyframe>,
}

/// A value at `time` seconds on the target's animation clock. `ease` shapes
/// the segment arriving at this keyframe and defaults to `linear`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimelineKeyframe {
    pub time: f64,
    pub value: TimelineValue,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ease: Option<String>,
}

/// CSS text, or a number: unitless for `opacity`, px for lengths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TimelineValue {
    Number(f64),
    Text(String),
}

impl Timeline {
    /// Check the format, one track per target and property, and finite,
    /// nonnegative, increasing keyframe times.
    pub fn validate(&self) -> Result<(), String> {
        if self.format != TIMELINE_FORMAT {
            return Err(format!("unsupported timeline format '{}'", self.format));
        }
        let mut seen = std::collections::BTreeSet::new();
        for track in &self.tracks {
            if !seen.insert((&track.target, &track.property)) {
                return Err(format!(
                    "timeline animates '{}' on '{}' twice; merge the keyframes into one track",
                    track.property, track.target
                ));
            }
            if track.keyframes.is_empty() {
                return Err(format!("timeline track '{}' has no keyframes", track.target));
            }
            let mut previous = None;
            for key in &track.keyframes {
                if !key.time.is_finite() || key.time < 0.0 {
                    return Err(format!("timeline time {} must be finite and nonnegative", key.time));
                }
                if previous.is_some_and(|p| key.time <= p) {
                    return Err(format!(
                        "timeline keyframes for '{}' on '{}' must increase in time",
                        track.property, track.target
                    ));
                }
                previous = Some(key.time);
            }
        }
        Ok(())
    }
}

/// The structural path of `el` below the composition root: `/` for the
/// root, else `/i/j` with zero-based element child indices.
pub(crate) fn element_path(el: scraper::ElementRef<'_>, root: ego_tree::NodeId) -> String {
    let mut indices = Vec::new();
    let mut node = el;
    while node.id() != root {
        let Some(parent) = node.parent().and_then(scraper::ElementRef::wrap) else {
            break;
        };
        let index = parent
            .children()
            .filter_map(scraper::ElementRef::wrap)
            .position(|c| c.id() == node.id())
            .unwrap_or(0);
        indices.push(index);
        node = parent;
    }
    if indices.is_empty() {
        return "/".into();
    }
    indices.iter().rev().map(|i| format!("/{i}")).collect()
}

/// Every timeline track in the document, grouped by target id or path.
pub(crate) fn collect(dom: &Html) -> Result<BTreeMap<String, Vec<TimelineTrack>>, CompileError> {
    let mut tracks: BTreeMap<String, Vec<TimelineTrack>> = BTreeMap::new();
    let mut timeline = Timeline {
        format: TIMELINE_FORMAT.into(),
        tracks: Vec::new(),
    };
    for script in dom.select(&Selector::parse("script").unwrap()) {
        if script.value().attr("type") != Some(TIMELINE_SCRIPT) {
            continue;
        }
        let text: String = script.text().collect();
        let part: Timeline = serde_json::from_str(&text)
            .map_err(|e| CompileError::Input(format!("timeline JSON: {e}")))?;
        if part.format != TIMELINE_FORMAT {
            return Err(CompileError::Input(format!(
                "unsupported timeline format '{}'",
                part.format
            )));
        }
        timeline.tracks.extend(part.tracks);
    }
    timeline.validate().map_err(CompileError::Input)?;
    for track in timeline.tracks {
        tracks.entry(track.target.clone()).or_default().push(track);
    }
    Ok(tracks)
}
