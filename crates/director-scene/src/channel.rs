//! Keyframed values, easing and color.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One animated value. `T` is `f32` unless a field says otherwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Channel<T = f32> {
    /// Samples in time order. A still value is one keyframe at `t = 0`.
    pub keyframes: Vec<Keyframe<T>>,
}

/// A sample on a [`Channel`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Keyframe<T = f32> {
    /// Time in seconds from the start of the channel.
    pub time: f64,
    /// Value at `time`.
    pub value: T,
    /// Easing into this sample. `linear` for the first sample.
    pub easing: Easing,
}

/// Easing into a keyframe. Matches `director-core`'s `EasingType`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    BounceOut,
    BounceIn,
    BounceInOut,
    ElasticOut,
    ElasticIn,
    ElasticInOut,
    BackOut,
    BackIn,
    BackInOut,
    /// CSS `cubic-bezier(x1, y1, x2, y2)`.
    CubicBezier { x1: f64, y1: f64, x2: f64, y2: f64 },
    /// The value holds until the end of the segment.
    Hold,
    /// The expression the keyframes were baked from at 60 fps.
    Expression { expr: String },
}

/// sRGB color with straight alpha, each component in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rgba {
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
    /// Alpha. `0.0` is clear.
    pub a: f32,
}

impl<T> Channel<T> {
    /// A channel that holds `value` at time 0 with linear easing.
    pub fn still(value: T) -> Self {
        Self {
            keyframes: vec![Keyframe {
                time: 0.0,
                value,
                easing: Easing::Linear,
            }],
        }
    }
}
