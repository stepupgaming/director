//! Semantic entities: faces, speakers, objects and caption phrases over time.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stable UUID text, such as `6f1c…`.
pub type EntityId = String;

/// One entity with its bounding-box track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Entity {
    pub id: EntityId,
    pub kind: EntityKind,
    pub label: String,
    /// Samples in time order.
    pub samples: Vec<EntitySample>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Face,
    Speaker,
    Object,
    CaptionPhrase,
}

/// A bounding box at one time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EntitySample {
    pub time: f64,
    pub bbox: BoundingBox,
    pub confidence: f32,
}

/// Axis-aligned box in frame pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
