//! # Entity Registry Module
//!
//! Persistent UUID-based identity for semantic entities in a video project.
//!
//! ## Responsibilities
//! - **Entity Identity**: Every face, speaker, object, or caption phrase gets a stable UUID.
//! - **Temporal Tracks**: Each entity stores a sequence of bounding-box samples over time.
//! - **Sampling**: Query an entity's bounding box at any time with linear interpolation.
//! - **Node References**: Scene nodes can declare which entities they track.
//!
//! ## Key Types
//! - `EntityId`: A `uuid::Uuid` that survives project save/load without remapping.
//! - `EntityKind`: Classification for faces, speakers, objects, and caption phrases.
//! - `EntityTrack`: A single entity's label + temporal bounding-box samples.
//! - `EntityRegistry`: The container that owns all tracks and provides lookups.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Stable identifier for a semantic entity.
pub type EntityId = Uuid;

/// Classification of a semantic entity.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    /// A detected face in the footage.
    Face,
    /// A speaking person (may span multiple face tracks over time).
    Speaker,
    /// A generic tracked object.
    Object,
    /// A phrase in a caption/transcript.
    CaptionPhrase,
}

impl EntityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityKind::Face => "face",
            EntityKind::Speaker => "speaker",
            EntityKind::Object => "object",
            EntityKind::CaptionPhrase => "caption_phrase",
        }
    }
}

impl std::str::FromStr for EntityKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "face" => Ok(EntityKind::Face),
            "speaker" => Ok(EntityKind::Speaker),
            "object" => Ok(EntityKind::Object),
            "caption_phrase" | "captionphrase" => Ok(EntityKind::CaptionPhrase),
            _ => Err(format!("unknown entity kind: {}", s)),
        }
    }
}

/// Axis-aligned bounding box in frame coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl BoundingBox {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    /// Center point of the bounding box.
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    /// Linearly interpolate to another bounding box.
    pub fn lerp(&self, other: &BoundingBox, t: f32) -> BoundingBox {
        BoundingBox {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
            width: self.width + (other.width - self.width) * t,
            height: self.height + (other.height - self.height) * t,
        }
    }
}

/// A single temporal sample for an entity track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityTrackSample {
    pub time: f64,
    pub bbox: BoundingBox,
    #[serde(default)]
    pub confidence: f32,
}

/// A semantic entity with a stable identity and temporal bounding-box track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityTrack {
    pub id: EntityId,
    pub kind: EntityKind,
    pub label: String,
    pub samples: Vec<EntityTrackSample>,
}

impl EntityTrack {
    pub fn new(id: EntityId, kind: EntityKind, label: String) -> Self {
        Self {
            id,
            kind,
            label,
            samples: Vec::new(),
        }
    }

    /// Add a new sample. Samples are kept sorted by time.
    pub fn add_sample(&mut self, time: f64, bbox: BoundingBox, confidence: f32) {
        let sample = EntityTrackSample {
            time,
            bbox,
            confidence,
        };
        // Insert sorted by time to keep sample_at efficient.
        // NaN times are treated as equal to everything and appended; they
        // should not exist in normal usage.
        let idx = self
            .samples
            .binary_search_by(|s| s.time.partial_cmp(&time).unwrap_or(std::cmp::Ordering::Equal))
            .ok();
        match idx {
            Some(idx) => self.samples[idx] = sample,
            None => self.samples.push(sample),
        }
    }

    /// Sample the bounding box at a given time using linear interpolation.
    /// Returns `None` if there are no samples.
    pub fn sample_at(&self, time: f64) -> Option<EntityTrackSample> {
        if self.samples.is_empty() {
            return None;
        }

        // Before first sample
        if time <= self.samples.first().unwrap().time {
            return Some(self.samples.first().unwrap().clone());
        }

        // After last sample
        if time >= self.samples.last().unwrap().time {
            return Some(self.samples.last().unwrap().clone());
        }

        // Find surrounding samples
        let idx = self.samples.partition_point(|s| s.time < time);
        let prev = &self.samples[idx - 1];
        let next = &self.samples[idx];

        let t = if next.time > prev.time {
            ((time - prev.time) / (next.time - prev.time)) as f32
        } else {
            0.0
        };

        let bbox = prev.bbox.lerp(&next.bbox, t);
        let confidence = prev.confidence + (next.confidence - prev.confidence) * t;

        Some(EntityTrackSample {
            time,
            bbox,
            confidence,
        })
    }

    /// Time range covered by this track, if any.
    pub fn time_range(&self) -> Option<(f64, f64)> {
        if self.samples.is_empty() {
            None
        } else {
            Some((self.samples.first().unwrap().time, self.samples.last().unwrap().time))
        }
    }
}

/// Container for all semantic entity tracks in a project.
#[derive(Clone, Debug, Default)]
pub struct EntityRegistry {
    tracks: HashMap<EntityId, EntityTrack>,
    by_kind: HashMap<EntityKind, Vec<EntityId>>,
}

impl EntityRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new entity track and return its stable ID.
    pub fn create(&mut self, kind: EntityKind, label: String) -> EntityId {
        let id = Uuid::new_v4();
        let track = EntityTrack::new(id, kind.clone(), label);
        self.add_track(track)
    }

    /// Add an existing track to the registry. Returns the track's ID.
    /// If a track with the same ID already exists, it is replaced and the
    /// kind index is updated.
    pub fn add_track(&mut self, track: EntityTrack) -> EntityId {
        let id = track.id;
        // Remove any stale kind index entry before re-inserting.
        if let Some(existing) = self.tracks.get(&id) {
            if let Some(ids) = self.by_kind.get_mut(&existing.kind) {
                ids.retain(|&x| x != id);
            }
        }
        self.by_kind.entry(track.kind.clone()).or_default().push(id);
        self.tracks.insert(id, track);
        id
    }

    /// Get a reference to a track by ID.
    pub fn get(&self, id: EntityId) -> Option<&EntityTrack> {
        self.tracks.get(&id)
    }

    /// Get a mutable reference to a track by ID.
    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut EntityTrack> {
        self.tracks.get_mut(&id)
    }

    /// Remove a track from the registry.
    pub fn remove(&mut self, id: EntityId) -> Option<EntityTrack> {
        if let Some(track) = self.tracks.remove(&id) {
            if let Some(ids) = self.by_kind.get_mut(&track.kind) {
                ids.retain(|&x| x != id);
            }
            Some(track)
        } else {
            None
        }
    }

    /// Find all entity IDs of a given kind.
    pub fn find_by_kind(&self, kind: &EntityKind) -> &[EntityId] {
        self.by_kind.get(kind).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Iterate over all tracks.
    pub fn iter(&self) -> impl Iterator<Item = &EntityTrack> {
        self.tracks.values()
    }

    /// True if a track with the given ID exists.
    pub fn contains(&self, id: EntityId) -> bool {
        self.tracks.contains_key(&id)
    }

    /// Number of tracks in the registry.
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// True if the registry has no tracks.
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Sample a track's bounding box at a given time.
    pub fn sample_at(&self, id: EntityId, time: f64) -> Option<EntityTrackSample> {
        self.tracks.get(&id).and_then(|t| t.sample_at(time))
    }

    /// Convenience: add a sample to a track by ID.
    pub fn add_sample(&mut self, id: EntityId, time: f64, bbox: BoundingBox, confidence: f32) {
        if let Some(track) = self.tracks.get_mut(&id) {
            track.add_sample(time, bbox, confidence);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_lookup() {
        let mut registry = EntityRegistry::new();
        let id = registry.create(EntityKind::Face, "mara".to_string());
        assert!(!registry.is_empty());
        let track = registry.get(id).unwrap();
        assert_eq!(track.kind, EntityKind::Face);
        assert_eq!(track.label, "mara");
    }

    #[test]
    fn find_by_kind_filters() {
        let mut registry = EntityRegistry::new();
        let face = registry.create(EntityKind::Face, "mara".to_string());
        let _speaker = registry.create(EntityKind::Speaker, "mara".to_string());
        let faces = registry.find_by_kind(&EntityKind::Face);
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0], face);
    }

    #[test]
    fn sample_interpolates() {
        let mut registry = EntityRegistry::new();
        let id = registry.create(EntityKind::Object, "phone".to_string());
        registry.add_sample(id, 0.0, BoundingBox::new(0.0, 0.0, 10.0, 10.0), 1.0);
        registry.add_sample(id, 1.0, BoundingBox::new(10.0, 0.0, 10.0, 10.0), 1.0);

        let s = registry.sample_at(id, 0.5).unwrap();
        assert!((s.bbox.x - 5.0).abs() < 0.01);
    }
}
