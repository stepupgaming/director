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
    /// Path traced paths per pixel in each sample: the Hero level. 0
    /// rasterizes the frame.
    #[serde(default, skip_serializing_if = "is_rasterized")]
    pub paths: u32,
    /// Lights Director adds to the file's own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lights: Vec<ModelLight>,
    /// Gaussian splat clouds placed in the scene beside the file's meshes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub splats: Vec<ModelSplat>,
    /// Names of glTF nodes whose meshes show only the shadows cast on them,
    /// so the model's shadows fall on footage behind a transparent model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shadow_catchers: Vec<String>,
}

/// A Gaussian splat cloud from a 3DGS `.ply` file, placed in the model's
/// world space: scaled about its own origin, then rotated, then moved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelSplat {
    /// The `.ply` file.
    pub asset: AssetId,
    /// Metres along X, Y and Z.
    pub position: [f32; 3],
    /// Degrees about X, then Y, then Z.
    pub rotation: [f32; 3],
    /// Uniform scale.
    pub scale: f32,
    /// Multiplies every gaussian's opacity, 0 to 1.
    pub opacity: f32,
}

/// What a `ModelLight` emits from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ModelLightKind {
    /// Parallel light from a disc in the sky.
    Sun,
    /// A sphere that lights every direction.
    Point,
    /// A sphere that lights a cone.
    Spot,
    /// A one-sided rectangle, like a softbox.
    Rect,
}

/// A light in the model's world space: metres, Y up, with the framing camera
/// on +Z. A light with an emitter size (`radius`, `angle` or a rectangle)
/// averages renders lit from points across the emitter, which softens its
/// shadows the way a real source does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelLight {
    pub kind: ModelLightKind,
    /// sRGB colour, each channel 0 to 1.
    pub color: [f32; 3],
    /// Illuminance in lux for a sun, luminous power in lumens otherwise.
    pub intensity: f32,
    /// Where the light sits. `None` places it up and to the front right of
    /// the bounding sphere. A sun uses only its direction to `target`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 3]>,
    /// The point the light faces. `None` is the bounding sphere centre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<[f32; 3]>,
    /// Emitter radius in metres for a point or spot light.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub radius: f32,
    /// Angular diameter of a sun in degrees.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f32,
    /// Full cone angle of a spot light in degrees.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cone: f32,
    /// Width and height of a rect light in metres.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub width: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub height: f32,
    /// Metres beyond which a point, spot or rect light stops. 0 picks a
    /// range that covers the scene.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub range: f32,
    pub shadows: bool,
}

fn one() -> u32 {
    1
}

fn is_one(n: &u32) -> bool {
    *n == 1
}

fn is_rasterized(paths: &u32) -> bool {
    *paths == 0
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
