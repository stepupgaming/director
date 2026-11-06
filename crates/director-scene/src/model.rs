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
}

/// An orbit about the model's bounding sphere centre, applied on top of the
/// file's first camera or the framing camera.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelView {
    /// Degrees about the world up axis.
    pub yaw: f32,
    /// Degrees about the camera's right axis; positive raises the camera.
    pub pitch: f32,
    /// Multiple of the camera's distance from the centre.
    pub distance: f32,
}

impl Default for ModelView {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: 0.0, distance: 1.0 }
    }
}
