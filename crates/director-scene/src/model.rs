//! The `director-3d` model body.

use crate::AssetId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A glTF binary scene rendered by `director-3d` into the node box. Every
/// animation clip in the file plays on the node's clock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelBody {
    /// The `.glb` file.
    pub asset: AssetId,
    /// An equirectangular `.hdr` or `.exr` light probe. `None` lights the
    /// model with the neutral studio probe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<AssetId>,
    /// Probe radiance scale in cd/m².
    pub environment_intensity: f32,
    /// Camera exposure in EV100.
    pub exposure: f32,
    /// Paint the probe behind the model. Otherwise the box stays transparent
    /// where the model is absent.
    pub background: bool,
    /// Clips wrap at the longest clip's length. Otherwise they hold the end.
    pub looped: bool,
    /// The camera orbit before animation.
    pub view: ModelView,
    /// Jittered sub-pixel renders averaged into each frame. 1 is the Live
    /// level; more is the Export level.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub samples: u32,
}

fn one() -> u32 {
    1
}

fn is_one(n: &u32) -> bool {
    *n == 1
}

/// An orbit about the model's bounding sphere centre, applied on top of the
/// file's first camera or the framing camera, and the lens focus.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelView {
    /// Degrees about the world up axis.
    pub yaw: f32,
    /// Degrees about the camera's right axis; positive raises the camera.
    pub pitch: f32,
    /// Multiple of the camera's distance from the centre.
    pub distance: f32,
    /// Distance in metres from the camera to the plane in focus. 0 focuses
    /// on the bounding sphere centre.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub focus: f32,
    /// Lens aperture as an f-number on a Super 35 sensor. 0 is a pinhole
    /// with everything in focus.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub aperture: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

impl Default for ModelView {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: 0.0, distance: 1.0, focus: 0.0, aperture: 0.0 }
    }
}
