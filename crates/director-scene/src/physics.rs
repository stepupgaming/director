//! Baked rigid-body physics inside a `director-3d` model.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A rigid-body simulation run once over a span of the element clock and
/// played back from its stored poses, so any frame renders alone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ModelPhysics {
    /// Acceleration in metres per second squared.
    pub gravity: [f32; 3],
    /// Element seconds the simulation starts. Bodies hold their first pose
    /// before it.
    pub start: f32,
    /// Seconds simulated from `start`. Bodies hold their last pose after it.
    pub duration: f32,
    /// Simulation steps per second.
    pub rate: f32,
    /// Picks every random choice of the spawners.
    pub seed: u32,
    /// Bodies from glTF nodes or built-in shapes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bodies: Vec<PhysicsBody>,
    /// Emitters of copies of a body over time.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spawners: Vec<PhysicsSpawner>,
    /// The hash of every input of the bake, in hex, written on export.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bake: Option<String>,
}

impl Default for ModelPhysics {
    fn default() -> Self {
        Self {
            gravity: [0.0, -9.81, 0.0],
            start: 0.0,
            duration: 5.0,
            rate: 120.0,
            seed: 1,
            bodies: Vec::new(),
            spawners: Vec::new(),
            bake: None,
        }
    }
}

/// How a body moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BodyKind {
    /// Moved by gravity and contacts.
    #[default]
    Dynamic,
    /// Never moves.
    Static,
    /// Follows its glTF clip, or its velocity and spin without one, and
    /// pushes dynamic bodies.
    Kinematic,
}

/// The collision shape of a body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ColliderKind {
    /// The shape of a built-in body, a node's triangles on a static body,
    /// and a convex hull otherwise.
    #[default]
    Auto,
    /// The convex hull of the mesh.
    Hull,
    /// The mesh's bounding box in its own frame.
    Box,
    /// A ball around the bounding box.
    Sphere,
    /// A capsule along the mesh's own Y axis.
    Capsule,
    /// The triangles themselves, for static bodies.
    Trimesh,
}

/// A built-in body mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BodyShape {
    /// A box of `size`.
    Box,
    /// A ball `size[0]` across.
    Sphere,
    /// A cylinder along Y, `size[0]` across and `size[1]` tall: a coin, a
    /// can or a pillar.
    Cylinder,
}

/// One body, or every glTF node a pattern names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct PhysicsBody {
    /// A glTF node name, or a pattern where `*` matches any run of
    /// characters and `?` one. `None` with a `shape`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// A built-in mesh in place of a node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<BodyShape>,
    /// The node name a built-in body takes, so a shadow catcher can name it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A built-in body's size in metres.
    pub size: [f32; 3],
    /// A built-in body's position in metres, or a spawner's emitter centre.
    pub position: [f32; 3],
    /// A built-in body's or a spawner's rotation in degrees about X, then
    /// Y, then Z.
    pub rotation: [f32; 3],
    /// A built-in body's sRGB colour, each channel 0 to 1.
    pub color: [f32; 3],
    /// A built-in body's metallic factor.
    pub metallic: f32,
    /// A built-in body's perceptual roughness.
    pub roughness: f32,
    pub kind: BodyKind,
    pub collider: ColliderKind,
    /// Kilograms per cubic metre of the collider.
    pub density: f32,
    /// Kilograms, in place of the density.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mass: Option<f32>,
    /// Coulomb friction coefficient.
    pub friction: f32,
    /// Bounciness, 0 to 1.
    pub restitution: f32,
    /// Metres per second at the start.
    pub velocity: [f32; 3],
    /// Degrees per second about X, Y and Z at the start.
    pub spin: [f32; 3],
    /// The share of its speed a body loses each second, as air drag.
    pub linear_damping: f32,
    /// The share of its spin a body loses each second.
    pub angular_damping: f32,
    /// The share of its spin a body loses each second while it touches
    /// another, so a coin rolling on its rim slows and falls flat.
    pub rolling_resistance: f32,
}

impl Default for PhysicsBody {
    fn default() -> Self {
        Self {
            node: None,
            shape: None,
            name: None,
            size: [0.1; 3],
            position: [0.0; 3],
            rotation: [0.0; 3],
            color: [0.8; 3],
            metallic: 0.0,
            roughness: 0.5,
            kind: BodyKind::Dynamic,
            collider: ColliderKind::Auto,
            density: 1000.0,
            mass: None,
            friction: 0.6,
            restitution: 0.2,
            velocity: [0.0; 3],
            spin: [0.0; 3],
            linear_damping: 0.0,
            angular_damping: 0.0,
            rolling_resistance: 4.0,
        }
    }
}

/// Where a spawner places its copies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SpawnEmitter {
    /// At the body's position.
    #[default]
    Point,
    /// Uniformly inside a ball of `radius`.
    Sphere,
    /// Uniformly inside a box of size `extent`.
    Box,
}

/// Copies of a body born over time: "drop 300 coins".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct PhysicsSpawner {
    /// The body each copy repeats. Its `position` centres the emitter and
    /// its node, if any, is hidden as a template.
    #[serde(flatten)]
    pub body: PhysicsBody,
    /// Copies born.
    pub count: u32,
    /// Element seconds of the first birth.
    pub start: f32,
    /// Seconds over which the copies are born, evenly. 0 births all at once.
    pub duration: f32,
    pub emitter: SpawnEmitter,
    /// A sphere emitter's radius in metres.
    pub radius: f32,
    /// A box emitter's size in metres.
    pub extent: [f32; 3],
    /// Metres per second added to the velocity in a random direction, at
    /// most.
    pub velocity_jitter: f32,
    /// Degrees per second of spin about a random axis, at most.
    pub spin_jitter: f32,
    /// Degrees a copy's orientation may turn from the rotation about a
    /// random axis, 0 to 180.
    pub tilt: f32,
    /// The share of a copy's scale that varies at random, 0 to 1.
    pub scale_jitter: f32,
}

impl Default for PhysicsSpawner {
    fn default() -> Self {
        Self {
            body: PhysicsBody::default(),
            count: 10,
            start: 0.0,
            duration: 1.0,
            emitter: SpawnEmitter::Point,
            radius: 0.2,
            extent: [0.2; 3],
            velocity_jitter: 0.0,
            spin_jitter: 0.0,
            tilt: 180.0,
            scale_jitter: 0.0,
        }
    }
}
