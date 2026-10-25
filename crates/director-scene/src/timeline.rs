//! Tracks, clips and scene transitions.

use crate::channel::Easing;
use crate::NodeRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Every track plus the transitions between scenes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Timeline {
    /// Tracks in paint order. A later track paints over an earlier one.
    pub tracks: Vec<Track>,
    /// Index of the track `add_scene` appends to.
    pub default_track: usize,
    /// Scene transitions.
    pub transitions: Vec<Transition>,
}

/// One track of clips.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Track {
    pub name: String,
    pub enabled: bool,
    pub opacity: f32,
    /// Clips ordered by start time.
    pub clips: Vec<Clip>,
}

/// A scene placed on a track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Clip {
    /// Root node of the scene content.
    pub scene_root: NodeRef,
    pub name: Option<String>,
    /// Start on the track in seconds.
    pub start_time: f64,
    /// Rendered duration in seconds.
    pub duration: f64,
    /// In-point inside the scene in seconds.
    pub source_start: f64,
    /// Out-point inside the scene in seconds. `0` plays to the end.
    pub source_end: f64,
    /// Playback speed. At or below zero, time holds at `source_start`.
    pub speed: f64,
    /// Scene time held for the whole clip.
    pub freeze_time: Option<f64>,
    pub z_index: i32,
    /// Audio track slots that belong to this clip.
    pub audio_tracks: Vec<usize>,
    /// Fade-in in seconds.
    pub fade_in: f64,
    /// Fade-out in seconds.
    pub fade_out: f64,
}

/// A transition between two scenes of the flat timeline.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Transition {
    /// Index of the outgoing scene in the flat timeline.
    pub from_scene: usize,
    /// Index of the incoming scene in the flat timeline.
    pub to_scene: usize,
    pub start_time: f64,
    pub duration: f64,
    /// Catalog name, such as `fade` or `wave`.
    pub kind: String,
    /// Catalog parameters by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, f32>,
    pub easing: Easing,
}
