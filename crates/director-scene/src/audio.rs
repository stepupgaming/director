//! Audio tracks and their effect chains.

use crate::channel::Channel;
use crate::AssetId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The mixer: its rate and occupied track slots.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioMix {
    /// Mixer sample rate in Hz.
    pub sample_rate: u32,
    /// Occupied slots in slot order.
    pub tracks: Vec<AudioTrack>,
}

/// One mixer track.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioTrack {
    /// Mixer slot. Clips and spectrum nodes refer to this number.
    pub slot: usize,
    /// The audio file, or the decoded samples when no file exists.
    pub asset: AssetId,
    /// Gain. `1.0` is unity.
    pub volume: Channel,
    /// Pan. `0.5` is center.
    pub pan: Channel,
    /// Start on the global timeline in seconds.
    pub start_time: f64,
    /// Play length in seconds. Absent plays to the end of the samples.
    pub duration: Option<f64>,
    pub loop_audio: bool,
    /// Effects in processing order.
    pub effects: Vec<AudioEffect>,
    /// Keyframed effect parameters.
    pub param_animations: Vec<AudioParamAnimation>,
}

/// One audio effect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioEffect {
    /// Effect name: `reverb`, `delay`, `eq` or `compressor`.
    pub kind: String,
    /// EQ filter shape, such as `low_pass` or `peak`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Parameters by the names `set_parameter` accepts.
    pub params: BTreeMap<String, f32>,
}

/// A keyframed parameter on one effect of the chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioParamAnimation {
    /// Index into [`AudioTrack::effects`].
    pub effect: usize,
    /// Parameter name.
    pub param: String,
    pub channel: Channel,
}
