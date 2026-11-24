//! Baked rigid-body physics: one deterministic Rapier simulation per set of
//! inputs, run on the CPU at a fixed step and stored as a pose track per
//! moving body. Frames look the pose up at their own element time, so they
//! render alone, in any order, and at any sub-frame time.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use bevy::camera::visibility::Visibility;
use bevy::ecs::world::World;
use bevy::math::{EulerRot, Mat4, Quat, Vec2, Vec3};
use bevy::prelude::*;
use director_scene::{BodyKind, BodyShape, ColliderKind, ModelPhysics, PhysicsBody, SpawnEmitter};
use rapier3d::prelude::{ColliderBuilder, PhysicsWorld, RigidBodyBuilder, RigidBodyHandle, QueryFilter, SharedShape, TriMeshFlags};

/// Bumped whenever the bake's output changes for the same inputs.
const BAKE_VERSION: u32 = 1;
/// The most bodies one simulation holds.
pub const MAX_BODIES: usize = 20_000;
/// The most simulation steps one bake runs.
const MAX_STEPS: f32 = 200_000.0;

/// A model's physics: its inputs and their bake.
#[derive(Clone, Debug)]
pub struct Physics {
    pub spec: ModelPhysics,
    pub bake: Arc<Bake>,
}

impl Physics {
    /// Checks `spec` against the model `bytes` and bakes it, or reuses the
    /// bake of the same inputs. `duration` and `looped` are the model's clip
    /// length and looping, which kinematic bodies follow.
    pub fn new(bytes: &[u8], model_hash: u64, spec: ModelPhysics, duration: f64, looped: bool) -> Result<Self, String> {
        let spec = ModelPhysics { bake: None, ..spec };
        check(&spec)?;
        let hash = inputs_hash(model_hash, &spec, duration, looped);
        static CACHE: OnceLock<Mutex<HashMap<u64, Arc<Bake>>>> = OnceLock::new();
        let cache = CACHE.get_or_init(Default::default);
        if let Some(bake) = cache.lock().expect("bake cache").get(&hash) {
            return Ok(Self { spec, bake: bake.clone() });
        }
        let bake = Arc::new(bake(bytes, &spec, duration, looped, hash)?);
        let mut cache = cache.lock().expect("bake cache");
        if cache.len() >= 16 {
            cache.clear();
        }
        cache.insert(hash, bake.clone());
        Ok(Self { spec, bake })
    }

    /// The bake's hash, the key of every input.
    pub fn hash(&self) -> u64 {
        self.bake.hash
    }
}

/// The stored poses of every moving body.
#[derive(Debug)]
pub struct Bake {
    /// Hash of every input, the cache key.
    pub hash: u64,
    /// Element seconds of step 0.
    pub start: f64,
    /// Steps per second.
    pub rate: f64,
    /// Steps simulated.
    pub steps: u32,
    /// One track per moving body.
    pub tracks: Vec<Track>,
    /// Built-in bodies that never move.
    pub fixed: Vec<usize>,
    /// What the bake measured.
    pub stats: BakeStats,
}

/// Where a track's body comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// A glTF node, by name.
    Node(String),
    /// The built-in body at this index of `ModelPhysics::bodies`.
    Shape(usize),
    /// A copy from the spawner at this index of `ModelPhysics::spawners`.
    Copy(usize),
}

/// The poses of one moving body from its birth.
#[derive(Clone, Debug)]
pub struct Track {
    pub source: Source,
    /// The step of the first pose: 0 for bodies present from the start.
    pub birth: u32,
    /// A copy's scale against its template.
    pub scale: f32,
    /// Position and rotation (x, y, z, w) at each step from `birth`. A body
    /// that ends at rest keeps one pose for its rest.
    pub poses: Vec<[f32; 7]>,
}

/// One mesh of a spawner's template, in its rigid frame.
type Part = (Handle<Mesh>, Handle<StandardMaterial>, Transform);

/// What a bake measured.
#[derive(Clone, Copy, Debug, Default)]
pub struct BakeStats {
    /// Wall-clock milliseconds of the simulation.
    pub millis: f64,
    /// Bodies simulated, of every kind.
    pub bodies: usize,
    /// Dynamic bodies asleep at the end.
    pub sleeping: usize,
    /// The deepest contact penetration between any two colliders at the end,
    /// in metres.
    pub penetration: f32,
    /// Contact pairs at the end deeper than a fiftieth of the typical moving
    /// body, such as two thin coins crossed mid-flight.
    pub deep: usize,
    /// Bytes of the stored poses.
    pub bytes: usize,
}

impl Bake {
    /// The pose of track `i` at element seconds `t`, or `None` before its
    /// birth. Positions blend linearly and rotations by slerp between steps.
    pub fn pose(&self, i: usize, t: f64) -> Option<(Vec3, Quat)> {
        let track = &self.tracks[i];
        let s = ((t - self.start) * self.rate).max(0.0);
        if track.birth > 0 && s < track.birth as f64 {
            return None;
        }
        let local = s - track.birth as f64;
        let at = local.floor();
        let last = track.poses.len() - 1;
        let unpack = |p: &[f32; 7]| (Vec3::new(p[0], p[1], p[2]), Quat::from_xyzw(p[3], p[4], p[5], p[6]));
        if at as usize >= last {
            return Some(unpack(&track.poses[last]));
        }
        let (a, b) = (unpack(&track.poses[at as usize]), unpack(&track.poses[at as usize + 1]));
        let f = (local - at) as f32;
        Some((a.0.lerp(b.0, f), a.1.slerp(b.1, f)))
    }

    /// Element seconds over which some body moves: from the first step to
    /// the last step that changes a pose.
    pub fn span(&self) -> Option<(f64, f64)> {
        let end = self
            .tracks
            .iter()
            .filter(|t| t.poses.len() > 1 || t.birth > 0)
            .map(|t| t.birth as f64 + (t.poses.len() - 1) as f64)
            .fold(None, |m: Option<f64>, e| Some(m.map_or(e, |m| m.max(e))))?;
        Some((self.start, self.start + end / self.rate))
    }

    /// Whether any body moves between element seconds `t0` and `t1`.
    pub fn moves_between(&self, t0: f64, t1: f64) -> bool {
        let (lo, hi) = (t0.min(t1), t0.max(t1));
        t0 != t1 && self.span().is_some_and(|(a, b)| hi > a && lo < b)
    }

    /// The bits of element seconds `t` for frame keys: one value before the
    /// bodies move, one after they rest, and the time between.
    pub fn time_bits(&self, t: f64) -> u64 {
        match self.span() {
            Some((a, _)) if t <= a => 0,
            Some((_, b)) if t >= b => 1,
            Some(_) => t.to_bits(),
            None => 0,
        }
    }
}

/// Refuses physics no simulation can run.
pub fn check(spec: &ModelPhysics) -> Result<(), String> {
    let finite = spec.gravity.iter().all(|g| g.is_finite()) && spec.start.is_finite();
    if !finite {
        return Err("physics gravity and start must be finite numbers".into());
    }
    if !(spec.duration > 0.0 && spec.duration <= 120.0) {
        return Err(format!("physics duration {} must be seconds above 0 and at most 120", spec.duration));
    }
    if !(10.0..=1000.0).contains(&spec.rate) {
        return Err(format!("physics rate {} must be from 10 to 1000 steps per second", spec.rate));
    }
    if spec.duration * spec.rate > MAX_STEPS {
        return Err(format!("physics runs {} steps, above {MAX_STEPS}", spec.duration * spec.rate));
    }
    if spec.bodies.is_empty() && spec.spawners.is_empty() {
        return Err("physics needs a <rigid> or <spawner> child".into());
    }
    for body in &spec.bodies {
        check_body(body, "rigid")?;
    }
    let copies: u64 = spec.spawners.iter().map(|s| s.count as u64).sum();
    if copies as usize + spec.bodies.len() > MAX_BODIES {
        return Err(format!("physics holds {copies} spawned bodies, above {MAX_BODIES}"));
    }
    let end = spec.start + spec.duration;
    for spawner in &spec.spawners {
        check_body(&spawner.body, "spawner")?;
        if spawner.body.kind != BodyKind::Dynamic {
            return Err("spawner bodies are dynamic".into());
        }
        if spawner.count == 0 {
            return Err("spawner count must be at least 1".into());
        }
        let numbers = [spawner.duration, spawner.radius, spawner.velocity_jitter, spawner.spin_jitter];
        if !spawner.start.is_finite() || numbers.iter().chain(&spawner.extent).any(|n| !(n.is_finite() && *n >= 0.0)) {
            return Err("spawner duration, radius, extent and jitters must be finite numbers from 0".into());
        }
        if !(0.0..=180.0).contains(&spawner.tilt) || !(0.0..=1.0).contains(&spawner.scale_jitter) {
            return Err("spawner tilt must be degrees from 0 to 180 and scale-jitter from 0 to 1".into());
        }
        if spawner.start < spec.start || spawner.start >= end {
            return Err(format!(
                "spawner start {} must fall within the simulation, from {} to before {end}",
                spawner.start, spec.start
            ));
        }
    }
    Ok(())
}

fn check_body(body: &PhysicsBody, what: &str) -> Result<(), String> {
    match (&body.node, body.shape) {
        (Some(_), Some(_)) => return Err(format!("{what} takes a node or a shape, not both")),
        (None, None) => return Err(format!("{what} needs a node name or a shape")),
        _ => {}
    }
    if body.collider == ColliderKind::Trimesh && body.kind != BodyKind::Static {
        return Err(format!("{what} collider trimesh needs a static body; use hull, box, sphere or capsule"));
    }
    if body.shape.is_some() && !matches!(body.collider, ColliderKind::Auto) {
        return Err(format!("{what} with a shape collides as that shape, so it takes no collider"));
    }
    if body.shape.is_some() && body.size.iter().any(|s| !(s.is_finite() && *s > 0.0)) {
        return Err(format!("{what} size must be metres above 0"));
    }
    if !(body.density.is_finite() && body.density > 0.0) || body.mass.is_some_and(|m| !(m.is_finite() && m > 0.0)) {
        return Err(format!("{what} density and mass must be above 0"));
    }
    if [body.linear_damping, body.angular_damping, body.rolling_resistance].iter().any(|d| !(d.is_finite() && *d >= 0.0)) {
        return Err(format!("{what} linear-damping, angular-damping and rolling-resistance must be numbers from 0"));
    }
    if !(body.friction.is_finite() && body.friction >= 0.0) || !(0.0..=1.0).contains(&body.restitution) {
        return Err(format!("{what} friction must be from 0 and restitution from 0 to 1"));
    }
    let vectors = body.position.iter().chain(&body.rotation).chain(&body.velocity).chain(&body.spin);
    if vectors.into_iter().any(|n| !n.is_finite()) {
        return Err(format!("{what} position, rotation, velocity and spin must be finite numbers"));
    }
    if body.color.iter().any(|c| !(0.0..=1.0).contains(c)) || !(0.0..=1.0).contains(&body.metallic) || !(0.0..=1.0).contains(&body.roughness) {
        return Err(format!("{what} color channels, metallic and roughness must be from 0 to 1"));
    }
    if body.name.is_some() && body.shape.is_none() {
        return Err(format!("{what} name names a built-in shape; a node body already has its node's name"));
    }
    Ok(())
}

fn inputs_hash(model: u64, spec: &ModelPhysics, duration: f64, looped: bool) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    BAKE_VERSION.hash(&mut hasher);
    model.hash(&mut hasher);
    serde_json::to_string(spec).expect("physics serializes").hash(&mut hasher);
    duration.to_bits().hash(&mut hasher);
    looped.hash(&mut hasher);
    hasher.finish()
}

/// Whether `name` matches `pattern`, where `*` matches any run of
/// characters and `?` any one.
pub fn matches(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ni;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

// ---------------------------------------------------------------- glTF --

/// A glTF node read on the CPU.
struct GltfNode {
    name: Option<String>,
    parent: Option<usize>,
    rest: Transform,
    /// Triangle vertices in the node's own space, and their indices.
    points: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
}

/// One animated property of a node.
struct Channel {
    node: usize,
    times: Vec<f32>,
    values: ChannelValues,
    step: bool,
}

enum ChannelValues {
    Translation(Vec<Vec3>),
    Rotation(Vec<Quat>),
    Scale(Vec<Vec3>),
}

struct GltfScene {
    nodes: Vec<GltfNode>,
    channels: Vec<Channel>,
}

fn read_gltf(bytes: &[u8]) -> Result<GltfScene, String> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| format!("glTF: {e}"))?;
    let blob = gltf.blob.as_deref();
    let buffer = |b: gltf::Buffer<'_>| match b.source() {
        gltf::buffer::Source::Bin => blob,
        gltf::buffer::Source::Uri(_) => None,
    };
    let document = &gltf.document;
    let mut nodes: Vec<GltfNode> = document
        .nodes()
        .map(|node| {
            let (t, r, s) = node.transform().decomposed();
            let mut points = Vec::new();
            let mut triangles = Vec::new();
            for primitive in node.mesh().into_iter().flat_map(|m| m.primitives()) {
                if primitive.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let reader = primitive.reader(buffer);
                let Some(positions) = reader.read_positions() else { continue };
                let base = points.len() as u32;
                points.extend(positions.map(Vec3::from));
                let count = points.len() as u32 - base;
                let indices: Vec<u32> = match reader.read_indices() {
                    Some(i) => i.into_u32().collect(),
                    None => (0..count).collect(),
                };
                triangles.extend(indices.as_chunks::<3>().0.iter().map(|t| [base + t[0], base + t[1], base + t[2]]));
            }
            GltfNode {
                name: node.name().map(str::to_string),
                parent: None,
                rest: Transform {
                    translation: Vec3::from(t),
                    rotation: Quat::from_array(r),
                    scale: Vec3::from(s),
                },
                points,
                triangles,
            }
        })
        .collect();
    for node in document.nodes() {
        for child in node.children() {
            nodes[child.index()].parent = Some(node.index());
        }
    }
    let mut channels = Vec::new();
    for animation in document.animations() {
        for channel in animation.channels() {
            let reader = channel.reader(buffer);
            let Some(times) = reader.read_inputs() else { continue };
            let times: Vec<f32> = times.collect();
            let interpolation = channel.sampler().interpolation();
            let cubic = interpolation == gltf::animation::Interpolation::CubicSpline;
            // A cubic spline stores in-tangent, value and out-tangent; the
            // bake follows its values linearly.
            let pick = |i: usize| !cubic || i % 3 == 1;
            let values = match reader.read_outputs() {
                Some(gltf::animation::util::ReadOutputs::Translations(v)) => {
                    ChannelValues::Translation(v.enumerate().filter(|(i, _)| pick(*i)).map(|(_, p)| Vec3::from(p)).collect())
                }
                Some(gltf::animation::util::ReadOutputs::Rotations(v)) => ChannelValues::Rotation(
                    v.into_f32().enumerate().filter(|(i, _)| pick(*i)).map(|(_, q)| Quat::from_array(q).normalize()).collect(),
                ),
                Some(gltf::animation::util::ReadOutputs::Scales(v)) => {
                    ChannelValues::Scale(v.enumerate().filter(|(i, _)| pick(*i)).map(|(_, s)| Vec3::from(s)).collect())
                }
                _ => continue,
            };
            channels.push(Channel {
                node: channel.target().node().index(),
                times,
                values,
                step: interpolation == gltf::animation::Interpolation::Step,
            });
        }
    }
    Ok(GltfScene { nodes, channels })
}

impl GltfScene {
    /// Whether node `i` or any node above it is animated.
    fn animated(&self, i: usize) -> bool {
        let mut at = Some(i);
        while let Some(n) = at {
            if self.channels.iter().any(|c| c.node == n) {
                return true;
            }
            at = self.nodes[n].parent;
        }
        false
    }

    /// Node `i`'s local transform at clip seconds `t`.
    fn local(&self, i: usize, t: f32) -> Transform {
        let mut out = self.nodes[i].rest;
        for c in self.channels.iter().filter(|c| c.node == i) {
            let n = c.times.len();
            let k = c.times.partition_point(|&x| x <= t);
            let (a, b, f) = if k == 0 {
                (0, 0, 0.0)
            } else if k >= n {
                (n - 1, n - 1, 0.0)
            } else {
                let (t0, t1) = (c.times[k - 1], c.times[k]);
                let f = if c.step || t1 <= t0 { 0.0 } else { (t - t0) / (t1 - t0) };
                (k - 1, k, f)
            };
            match &c.values {
                ChannelValues::Translation(v) => out.translation = v[a].lerp(v[b], f),
                ChannelValues::Rotation(v) => out.rotation = v[a].slerp(v[b], f),
                ChannelValues::Scale(v) => out.scale = v[a].lerp(v[b], f),
            }
        }
        out
    }

    /// Node `i`'s matrix in the scene's space at clip seconds `t`.
    fn world(&self, i: usize, t: f32) -> Mat4 {
        let mut m = self.local(i, t).to_matrix();
        let mut at = self.nodes[i].parent;
        while let Some(p) = at {
            m = self.local(p, t).to_matrix() * m;
            at = self.nodes[p].parent;
        }
        m
    }

    /// Nodes at or below `i`.
    fn subtree(&self, i: usize) -> Vec<usize> {
        let mut out = vec![i];
        let mut k = 0;
        while k < out.len() {
            let n = out[k];
            out.extend((0..self.nodes.len()).filter(|&c| self.nodes[c].parent == Some(n)));
            k += 1;
        }
        out
    }

    /// The triangles at or below node `i` in a frame placed at `frame` in
    /// the scene's space at rest.
    fn geometry(&self, i: usize, frame: Mat4) -> (Vec<Vec3>, Vec<[u32; 3]>) {
        let inverse = frame.inverse();
        let (mut points, mut triangles) = (Vec::new(), Vec::new());
        for n in self.subtree(i) {
            let to = inverse * self.world(n, 0.0).mul_scalar(1.0);
            let base = points.len() as u32;
            let node = &self.nodes[n];
            points.extend(node.points.iter().map(|p| to.transform_point3(*p)));
            triangles.extend(node.triangles.iter().map(|t| t.map(|k| k + base)));
        }
        (points, triangles)
    }
}

/// The rigid part (translation and rotation) of a matrix and its scale.
fn rigid(m: Mat4) -> (Vec3, Quat, Vec3) {
    let (scale, rotation, translation) = m.to_scale_rotation_translation();
    (translation, rotation.normalize(), scale)
}

fn euler(degrees: [f32; 3]) -> Quat {
    let [x, y, z] = degrees.map(f32::to_radians);
    Quat::from_euler(EulerRot::ZYX, z, y, x)
}

// ---------------------------------------------------------------- bake --

/// A random number in [0, 1) from a PCG hash of the seed and three indices.
fn random(seed: u32, a: u32, b: u32, c: u32) -> f32 {
    let mut h = seed ^ 0x9e37_79b9;
    for v in [a, b, c] {
        h = h.wrapping_add(v).wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
        let w = ((h >> ((h >> 28) + 4)) ^ h).wrapping_mul(277_803_737);
        h = (w >> 22) ^ w;
    }
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// A uniform point in the unit ball.
fn in_ball(r: impl Fn(u32) -> f32, at: u32) -> Vec3 {
    let z = r(at) * 2.0 - 1.0;
    let a = r(at + 1) * std::f32::consts::TAU;
    let s = (1.0 - z * z).max(0.0).sqrt();
    Vec3::new(s * a.cos(), z, s * a.sin()) * r(at + 2).cbrt()
}

/// A collision shape of `collider` kind around `points`, with the offset of
/// its centre in the body's frame.
fn node_shape(
    points: &[Vec3],
    triangles: &[[u32; 3]],
    collider: ColliderKind,
    kind: BodyKind,
    name: &str,
) -> Result<(SharedShape, Vec3), String> {
    if points.is_empty() {
        return Err(format!("physics node '{name}' has no mesh to collide with"));
    }
    let (lo, hi) = points.iter().fold((Vec3::MAX, Vec3::MIN), |(l, h), p| (l.min(*p), h.max(*p)));
    let (centre, half) = ((lo + hi) / 2.0, ((hi - lo) / 2.0).max(Vec3::splat(5e-4)));
    let size = (hi - lo).max_element();
    // A mesh whose every vertex sits on a corner of its bounds is a box, and
    // collides as one: exact faces stack steadier than a hull's.
    let boxy = points.iter().all(|p| {
        let corner = Vec3::select(p.cmplt(centre), lo, hi);
        p.distance(corner) <= size * 1e-4
    });
    // A mesh whose vertices sit on two rings about its own Y axis, or at
    // their centres, is a coin, a can or a pillar, and collides as an exact
    // cylinder.
    let radius = half.x;
    let round = (half.x - half.z).abs() <= size * 1e-3
        && points.iter().all(|p| {
            let r = Vec2::new(p.x - centre.x, p.z - centre.z).length();
            let cap = (p.y - lo.y).abs() <= size * 1e-4 || (p.y - hi.y).abs() <= size * 1e-4;
            cap && (r <= size * 1e-4 || (r - radius).abs() <= size * 2e-3)
        });
    if kind != BodyKind::Static && collider == ColliderKind::Auto && round && (hi - lo).y > size * 1e-3 {
        return Ok((SharedShape::cylinder(half.y, radius), centre));
    }
    let collider = match collider {
        ColliderKind::Auto if kind == BodyKind::Static => ColliderKind::Trimesh,
        ColliderKind::Auto if boxy && (hi - lo).min_element() > size * 1e-3 => ColliderKind::Box,
        ColliderKind::Auto => ColliderKind::Hull,
        other => other,
    };
    if collider == ColliderKind::Hull && (hi - lo).min_element() <= size * 1e-3 {
        return Err(format!("physics node '{name}' is flat, so it has no convex hull; give it collider box"));
    }
    Ok(match collider {
        ColliderKind::Box => (SharedShape::cuboid(half.x, half.y, half.z), centre),
        ColliderKind::Sphere => {
            let radius = points.iter().map(|p| p.distance(centre)).fold(0.0, f32::max).max(1e-3);
            (SharedShape::ball(radius), centre)
        }
        ColliderKind::Capsule => {
            let radius = half.x.max(half.z);
            (SharedShape::capsule_y((half.y - radius).max(0.0), radius), centre)
        }
        ColliderKind::Trimesh => {
            // Internal edges of a flat floor would bump bodies sliding over
            // them.
            let shape = SharedShape::trimesh_with_flags(points.to_vec(), triangles.to_vec(), TriMeshFlags::FIX_INTERNAL_EDGES)
                .map_err(|e| format!("physics node '{name}' triangles: {e:?}"))?;
            (shape, Vec3::ZERO)
        }
        ColliderKind::Hull | ColliderKind::Auto => {
            let shape = SharedShape::convex_hull(points).ok_or_else(|| {
                format!("physics node '{name}' is flat, so it has no convex hull; give it collider box")
            })?;
            (shape, Vec3::ZERO)
        }
    })
}

/// The collision shape of a built-in body of `size` scaled by `scale`.
fn builtin_shape(shape: BodyShape, size: [f32; 3], scale: f32) -> SharedShape {
    let s = Vec3::from(size) * scale;
    match shape {
        BodyShape::Box => SharedShape::cuboid(s.x / 2.0, s.y / 2.0, s.z / 2.0),
        BodyShape::Sphere => SharedShape::ball(s.x / 2.0),
        BodyShape::Cylinder => SharedShape::cylinder(s.y / 2.0, s.x / 2.0),
    }
}

/// One body ready for the simulation.
struct Planned {
    /// Index into `tracks`, for bodies that move.
    track: Option<usize>,
    birth: u32,
    builder: RigidBodyBuilder,
    shape: SharedShape,
    offset: Vec3,
    props: PhysicsBody,
    /// A clip-driven kinematic node and its frame in the scene at rest
    /// against its node matrix.
    clip: Option<(usize, Mat4)>,
    /// A copy's pose at birth, before it rises clear of other bodies.
    born: Option<(Vec3, Quat)>,
}

fn bake(bytes: &[u8], spec: &ModelPhysics, duration: f64, looped: bool, hash: u64) -> Result<Bake, String> {
    let scene = read_gltf(bytes)?;
    let rate = spec.rate as f64;
    let steps = (spec.duration as f64 * rate).ceil() as u32;
    let mut tracks: Vec<Track> = Vec::new();
    let mut fixed = Vec::new();
    let mut planned: Vec<Planned> = Vec::new();
    let mut taken: HashMap<usize, usize> = HashMap::new();
    let named = |i: usize| scene.nodes[i].name.as_deref();
    let velocity = |b: &PhysicsBody| (Vec3::from(b.velocity), Vec3::from(b.spin.map(f32::to_radians)));

    for (index, body) in spec.bodies.iter().enumerate() {
        if let Some(shape) = body.shape {
            let rotation = euler(body.rotation);
            let position = Vec3::from(body.position);
            let builder = match body.kind {
                BodyKind::Static => RigidBodyBuilder::fixed(),
                BodyKind::Kinematic => RigidBodyBuilder::kinematic_velocity_based(),
                BodyKind::Dynamic => RigidBodyBuilder::dynamic().ccd_enabled(true),
            };
            let (linear, angular) = velocity(body);
            let track = (body.kind != BodyKind::Static).then(|| {
                tracks.push(Track { source: Source::Shape(index), birth: 0, scale: 1.0, poses: Vec::new() });
                tracks.len() - 1
            });
            if track.is_none() {
                fixed.push(index);
            }
            planned.push(Planned {
                track,
                birth: 0,
                builder: builder.pose(pose(position, rotation)).linvel(linear).angvel(angular),
                shape: builtin_shape(shape, body.size, 1.0),
                offset: Vec3::ZERO,
                props: body.clone(),
                clip: None,
                born: None,
            });
            continue;
        }
        let pattern = body.node.as_deref().expect("checked");
        let mut hits: Vec<usize> = (0..scene.nodes.len()).filter(|&i| named(i).is_some_and(|n| matches(pattern, n))).collect();
        if hits.is_empty() {
            return Err(format!("rigid node '{pattern}' names no node in the model"));
        }
        // A match under another match of the same pattern is part of it.
        let all = hits.clone();
        hits.retain(|&i| {
            let mut at = scene.nodes[i].parent;
            while let Some(p) = at {
                if all.contains(&p) {
                    return false;
                }
                at = scene.nodes[p].parent;
            }
            true
        });
        for i in hits {
            let name = named(i).expect("named").to_string();
            if scene.nodes.iter().filter(|n| n.name.as_deref() == Some(&name)).count() > 1 {
                return Err(format!("rigid node '{name}' names two nodes in the model; give each body a unique name"));
            }
            if let Some(&other) = taken.get(&i) {
                if other != index {
                    return Err(format!("node '{name}' is matched by two <rigid> bodies"));
                }
            }
            taken.insert(i, index);
            let animated = scene.animated(i);
            if body.kind == BodyKind::Dynamic && animated {
                return Err(format!("rigid node '{name}' is animated, so it cannot be dynamic; make it kinematic"));
            }
            let world = scene.world(i, 0.0);
            let (t, r, _) = rigid(world);
            let frame = Mat4::from_rotation_translation(r, t);
            let (points, triangles) = scene.geometry(i, frame);
            let (shape, offset) = node_shape(&points, &triangles, body.collider, body.kind, &name)?;
            let clip = (body.kind == BodyKind::Kinematic && animated).then(|| (i, world.inverse() * frame));
            let builder = match body.kind {
                BodyKind::Static => RigidBodyBuilder::fixed(),
                BodyKind::Kinematic if clip.is_some() => RigidBodyBuilder::kinematic_position_based(),
                BodyKind::Kinematic => RigidBodyBuilder::kinematic_velocity_based(),
                BodyKind::Dynamic => RigidBodyBuilder::dynamic().ccd_enabled(true),
            };
            let (linear, angular) = velocity(body);
            let track = (body.kind == BodyKind::Dynamic || body.kind == BodyKind::Kinematic && clip.is_none()).then(|| {
                tracks.push(Track { source: Source::Node(name.clone()), birth: 0, scale: 1.0, poses: Vec::new() });
                tracks.len() - 1
            });
            planned.push(Planned {
                track,
                birth: 0,
                builder: builder.pose(pose(t, r)).linvel(linear).angvel(angular),
                shape,
                offset,
                props: body.clone(),
                clip,
                born: None,
            });
        }
    }

    for (index, spawner) in spec.spawners.iter().enumerate() {
        let body = &spawner.body;
        let template = match (&body.node, body.shape) {
            (Some(name), _) => {
                let hits: Vec<usize> = (0..scene.nodes.len()).filter(|&i| named(i) == Some(name.as_str())).collect();
                match hits[..] {
                    [i] => {
                        if taken.contains_key(&i) {
                            return Err(format!("spawner node '{name}' is also a <rigid> body"));
                        }
                        let (t, r, _) = rigid(scene.world(i, 0.0));
                        let (points, triangles) = scene.geometry(i, Mat4::from_rotation_translation(r, t));
                        node_shape(&points, &triangles, body.collider, BodyKind::Dynamic, name)?;
                        Some((points, triangles))
                    }
                    [] => return Err(format!("spawner node '{name}' names no node in the model")),
                    _ => return Err(format!("spawner node '{name}' names two nodes in the model")),
                }
            }
            _ => None,
        };
        let mut shapes: HashMap<u32, (SharedShape, Vec3)> = HashMap::new();
        let seed = spec.seed;
        for copy in 0..spawner.count {
            let r = |channel: u32| random(seed, index as u32, copy, channel);
            let born = spawner.start as f64 + spawner.duration as f64 * copy as f64 / spawner.count as f64;
            let birth = (((born - spec.start as f64) * rate).round() as i64).clamp(0, steps as i64) as u32;
            let offset = match spawner.emitter {
                SpawnEmitter::Point => Vec3::ZERO,
                SpawnEmitter::Sphere => in_ball(r, 0) * spawner.radius,
                SpawnEmitter::Box => (Vec3::new(r(0), r(1), r(2)) - 0.5) * Vec3::from(spawner.extent),
            };
            let tilt = if spawner.tilt >= 180.0 {
                // Shoemake's uniform rotation.
                let (u1, u2, u3) = (r(3), r(4) * std::f32::consts::TAU, r(5) * std::f32::consts::TAU);
                let (a, b) = ((1.0 - u1).sqrt(), u1.sqrt());
                Quat::from_xyzw(a * u2.sin(), a * u2.cos(), b * u3.sin(), b * u3.cos()).normalize()
            } else {
                let axis = in_ball(r, 3).try_normalize().unwrap_or(Vec3::Y);
                Quat::from_axis_angle(axis, (spawner.tilt * r(6)).to_radians())
            };
            let rotation = (tilt * euler(body.rotation)).normalize();
            let linear = Vec3::from(body.velocity) + in_ball(r, 7) * spawner.velocity_jitter;
            let angular = Vec3::from(body.spin.map(f32::to_radians)) + in_ball(r, 10) * spawner.spin_jitter.to_radians();
            let scale = 1.0 - spawner.scale_jitter * r(13);
            let (shape, centre) = match (&template, body.shape) {
                (Some((points, triangles)), _) => shapes
                    .entry(scale.to_bits())
                    .or_insert_with(|| {
                        let scaled: Vec<Vec3> = points.iter().map(|p| *p * scale).collect();
                        let name = body.node.as_deref().unwrap_or_default();
                        node_shape(&scaled, triangles, body.collider, BodyKind::Dynamic, name).expect("template checked")
                    })
                    .clone(),
                (None, Some(shape)) => (builtin_shape(shape, body.size, scale), Vec3::ZERO),
                (None, None) => unreachable!("checked"),
            };
            tracks.push(Track { source: Source::Copy(index), birth, scale, poses: Vec::new() });
            planned.push(Planned {
                track: Some(tracks.len() - 1),
                birth,
                builder: RigidBodyBuilder::dynamic().ccd_enabled(true).linvel(linear).angvel(angular),
                shape,
                offset: centre,
                props: body.clone(),
                clip: None,
                born: Some((Vec3::from(body.position) + offset, rotation)),
            });
        }
    }

    // Bodies enter in birth order, then in the order they were written, so
    // the simulation never depends on anything else.
    let mut order: Vec<usize> = (0..planned.len()).collect();
    order.sort_by_key(|&i| (planned[i].birth, i));
    let clip_time = |k: u32| crate::node::clip_seconds(spec.start as f64 + k as f64 / rate, duration, looped) as f32;
    let started = Instant::now();
    let mut world = PhysicsWorld { gravity: Vec3::from(spec.gravity), ..Default::default() };
    world.integration_parameters.dt = 1.0 / spec.rate;
    // Stacks and piles settle with more solver iterations, and contact
    // slop scales with the typical moving body, so coins and crates both
    // rest without sinking.
    world.integration_parameters.num_solver_iterations = 8;
    // Recycled contacts go stale by up to a twentieth of a length unit,
    // which lets stacks sink into each other; a bake pays for fresh ones.
    world.integration_parameters.contact_recycling = false;
    // Rapier's 30 Hz contacts let a stack of coins sink half a coin into
    // each other; 120 Hz holds them to a fraction of a millimetre, kept
    // below a quarter of the solver's substep rate.
    let hertz = (spec.rate * 2.0).min(120.0);
    world.integration_parameters.contact_softness.natural_frequency = hertz;
    world.integration_parameters.static_contact_softness.natural_frequency = 2.0 * hertz;
    let mut sizes: Vec<f32> = planned
        .iter()
        .filter(|p| p.props.kind == BodyKind::Dynamic)
        .map(|p| {
            let aabb = p.shape.compute_local_aabb();
            (aabb.maxs - aabb.mins).max_element()
        })
        .collect();
    sizes.sort_by(f32::total_cmp);
    if let Some(&median) = sizes.get(sizes.len() / 2) {
        world.integration_parameters.length_unit = median.clamp(0.01, 10.0);
    }
    let mut live: Vec<(usize, RigidBodyHandle)> = Vec::new();
    let mut next = 0;
    let record = |world: &PhysicsWorld, live: &[(usize, RigidBodyHandle)], tracks: &mut Vec<Track>, planned: &[Planned]| {
        for &(p, handle) in live {
            if let Some(t) = planned[p].track {
                let body = &world.bodies[handle];
                let (q, x) = (body.rotation(), body.translation());
                tracks[t].poses.push([x.x, x.y, x.z, q.x, q.y, q.z, q.w]);
            }
        }
    };
    let up = (-world.gravity).try_normalize().unwrap_or(Vec3::Y);
    for k in 0..=steps {
        let mut newborn: Vec<(SharedShape, rapier3d::math::Pose)> = Vec::new();
        while next < order.len() && planned[order[next]].birth <= k {
            let p = &planned[order[next]];
            let mut builder = p.builder.clone().linear_damping(p.props.linear_damping).angular_damping(p.props.angular_damping);
            // Thin bodies such as coins pass through each other in one
            // step at falling speed; predicting contacts half their
            // thickness ahead catches them.
            if p.props.kind == BodyKind::Dynamic {
                let aabb = p.shape.compute_local_aabb();
                builder = builder.soft_ccd_prediction(0.5 * (aabb.maxs - aabb.mins).min_element());
            }
            // A copy born inside another body rises against gravity until
            // it is clear, so births never start wedged together.
            if let Some((mut at, rotation)) = p.born {
                let aabb = p.shape.compute_local_aabb();
                let lift = (aabb.maxs - aabb.mins).max_element();
                for _ in 0..64 {
                    let here = pose(at + rotation * p.offset, rotation);
                    let blocked = world.intersect_shape(here, &*p.shape, QueryFilter::default()).next().is_some()
                        || newborn.iter().any(|(shape, there)| {
                            rapier3d::parry::query::intersection_test(&here, &*p.shape, there, &**shape).is_ok_and(|hit| hit.intersecting)
                        });
                    if !blocked {
                        newborn.push((p.shape.clone(), here));
                        break;
                    }
                    at += up * lift;
                }
                builder = builder.pose(pose(at, rotation));
            }
            let handle = world.insert_body(builder);
            let collider = ColliderBuilder::new(p.shape.clone())
                .translation(p.offset)
                .friction(p.props.friction)
                .restitution(p.props.restitution);
            let collider = match p.props.mass {
                Some(m) => collider.mass(m),
                None => collider.density(p.props.density),
            };
            world.insert_collider(collider, Some(handle));
            live.push((order[next], handle));
            next += 1;
        }
        record(&world, &live, &mut tracks, &planned);
        if k == steps {
            break;
        }
        for &(p, handle) in &live {
            if let Some((node, to_frame)) = planned[p].clip {
                let (t, r, _) = rigid(scene.world(node, clip_time(k + 1)) * to_frame);
                world.bodies[handle].set_next_kinematic_position(pose(t, r));
            }
        }
        // Rolling resistance: a body touching another loses spin, so a
        // coin rolling on its rim slows and falls flat.
        let mut touching = vec![false; world.bodies.len()];
        for pair in world.narrow_phase.contact_pairs().filter(|pair| pair.has_any_active_contact()) {
            for collider in [pair.collider1, pair.collider2] {
                if let Some(handle) = world.colliders[collider].parent() {
                    if let Some(slot) = touching.get_mut(handle.into_raw_parts().0 as usize) {
                        *slot = true;
                    }
                }
            }
        }
        for &(p, handle) in &live {
            let rolling = planned[p].props.rolling_resistance;
            let index = handle.into_raw_parts().0 as usize;
            if rolling > 0.0 && touching.get(index) == Some(&true) {
                let body = &mut world.bodies[handle];
                if body.is_dynamic() && !body.is_sleeping() {
                    let spin = body.angvel() / (1.0 + rolling * world.integration_parameters.dt);
                    body.set_angvel(spin, false);
                }
            }
        }
        world.step();
    }
    let millis = started.elapsed().as_secs_f64() * 1000.0;
    let sleeping = live
        .iter()
        .filter(|(p, h)| planned[*p].props.kind == BodyKind::Dynamic && world.bodies[*h].is_sleeping())
        .count();
    let mut penetration = 0.0f32;
    let mut deep = 0;
    let shallow = 0.02 * world.integration_parameters.length_unit;
    for pair in world.narrow_phase.contact_pairs() {
        let depth = pair.manifolds().iter().flat_map(|m| &m.points).map(|point| -point.dist).fold(0.0f32, f32::max);
        penetration = penetration.max(depth);
        deep += usize::from(depth > shallow);
    }
    // A body at rest at the end keeps one pose for its rest.
    for track in &mut tracks {
        while track.poses.len() > 1 && track.poses[track.poses.len() - 1] == track.poses[track.poses.len() - 2] {
            track.poses.pop();
        }
    }
    let bytes = tracks.iter().map(|t| t.poses.len() * 28).sum();
    Ok(Bake {
        hash,
        start: spec.start as f64,
        rate,
        steps,
        stats: BakeStats { millis, bodies: planned.len(), sleeping, penetration, deep, bytes },
        tracks,
        fixed,
    })
}

fn pose(t: Vec3, r: Quat) -> rapier3d::math::Pose {
    rapier3d::math::Pose::from_parts(t, r)
}

// ----------------------------------------------------------- the stage --

/// A body's entity in a stage and how its pose becomes its transform.
pub(crate) struct Placed {
    entity: Entity,
    /// The inverse of the parent's matrix in the stage root's space.
    parent: Mat4,
    scale: Vec3,
}

/// What `spawn` added to a stage.
#[derive(Default)]
pub(crate) struct Spawned {
    /// One per track of the bake.
    pub placed: Vec<Placed>,
    /// Copies, which stay out of the stage bounds.
    pub copies: Vec<Entity>,
}

fn material(world: &mut World, body: &PhysicsBody) -> Handle<StandardMaterial> {
    let [r, g, b] = body.color;
    world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
        base_color: Color::srgb(r, g, b),
        metallic: body.metallic,
        perceptual_roughness: body.roughness,
        ..default()
    })
}

fn builtin_mesh(world: &mut World, shape: BodyShape, size: [f32; 3]) -> Handle<Mesh> {
    let mesh = match shape {
        BodyShape::Box => Mesh::from(Cuboid::new(size[0], size[1], size[2])),
        BodyShape::Sphere => Sphere::new(size[0] / 2.0).mesh().uv(48, 24),
        BodyShape::Cylinder => Cylinder::new(size[0] / 2.0, size[1]).mesh().resolution(48).build(),
    };
    world.resource_mut::<Assets<Mesh>>().add(mesh)
}

/// Adds the bodies of `physics` under the stage `root`: built-in shapes,
/// copies of spawner templates (hidden until born) and the glTF nodes that
/// move, whose templates hide.
pub(crate) fn spawn(world: &mut World, root: Entity, physics: &Physics) -> Result<Spawned, String> {
    let spec = &physics.spec;
    let mut shapes: HashMap<usize, Entity> = HashMap::new();
    for (index, body) in spec.bodies.iter().enumerate() {
        let Some(shape) = body.shape else { continue };
        let mesh = builtin_mesh(world, shape, body.size);
        let material = material(world, body);
        let transform = Transform::from_translation(Vec3::from(body.position)).with_rotation(euler(body.rotation));
        let mut entity = world.spawn((Mesh3d(mesh), MeshMaterial3d(material), transform, Visibility::default(), ChildOf(root)));
        if let Some(name) = &body.name {
            entity.insert(Name::new(name.clone()));
        }
        shapes.insert(index, entity.id());
    }
    let by_name = |world: &World, name: &str| {
        crate::studio::descendants(world, root)
            .into_iter()
            .find(|&e| world.get::<Name>(e).is_some_and(|n| n.as_str() == name))
            .ok_or_else(|| format!("physics node '{name}' is not in the loaded scene"))
    };
    // Each spawner's template: the meshes of a node relative to its rigid
    // frame, or a built-in mesh.
    let mut templates: Vec<Vec<Part>> = Vec::new();
    for spawner in &spec.spawners {
        let body = &spawner.body;
        let parts = match (&body.node, body.shape) {
            (Some(name), _) => {
                let node = by_name(world, name)?;
                let at = crate::studio::world_transform(world, node, root);
                let frame = Transform::from_translation(at.translation).with_rotation(at.rotation).to_matrix().inverse();
                let mut parts = Vec::new();
                for e in std::iter::once(node).chain(crate::studio::descendants(world, node)) {
                    let (Some(mesh), Some(material)) = (world.get::<Mesh3d>(e), world.get::<MeshMaterial3d<StandardMaterial>>(e)) else {
                        continue;
                    };
                    let (mesh, material) = (mesh.0.clone(), material.0.clone());
                    let relative = frame * crate::studio::world_transform(world, e, root).to_matrix();
                    parts.push((mesh, material, Transform::from_matrix(relative)));
                }
                // The template shows only through its copies.
                world.entity_mut(node).insert((Visibility::Hidden, Transform::from_scale(Vec3::ZERO)));
                parts
            }
            (None, Some(shape)) => vec![(builtin_mesh(world, shape, body.size), material(world, body), Transform::IDENTITY)],
            (None, None) => Vec::new(),
        };
        templates.push(parts);
    }
    let mut out = Spawned::default();
    for track in &physics.bake.tracks {
        let placed = match &track.source {
            Source::Shape(index) => Placed { entity: shapes[index], parent: Mat4::IDENTITY, scale: Vec3::ONE },
            Source::Node(name) => {
                let entity = by_name(world, name)?;
                let parent = world.get::<ChildOf>(entity).map(|c| c.parent()).filter(|&p| p != root);
                let parent = parent.map_or(Mat4::IDENTITY, |p| crate::studio::world_transform(world, p, root).to_matrix().inverse());
                let scale = crate::studio::world_transform(world, entity, root).scale;
                Placed { entity, parent, scale }
            }
            Source::Copy(index) => {
                let entity = world.spawn((Transform::from_scale(Vec3::ZERO), Visibility::Hidden, ChildOf(root))).id();
                for (mesh, material, transform) in &templates[*index] {
                    world.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), *transform, Visibility::default(), ChildOf(entity)));
                }
                out.copies.push(entity);
                Placed { entity, parent: Mat4::IDENTITY, scale: Vec3::splat(track.scale) }
            }
        };
        out.placed.push(placed);
    }
    Ok(out)
}

/// Poses every moving body at element seconds `t`. A body before its birth
/// hides and shrinks to nothing, since the path tracer ignores visibility.
pub(crate) fn place(world: &mut World, spawned: &Spawned, bake: &Bake, t: f64) {
    for (i, placed) in spawned.placed.iter().enumerate() {
        let Ok(mut entity) = world.get_entity_mut(placed.entity) else { continue };
        match bake.pose(i, t) {
            Some((position, rotation)) => {
                let m = placed.parent * Mat4::from_scale_rotation_translation(placed.scale, rotation, position);
                entity.insert((Transform::from_matrix(m), Visibility::Inherited));
            }
            None => {
                entity.insert((Transform::from_scale(Vec3::ZERO), Visibility::Hidden));
            }
        }
    }
}

/// The names of the built-in bodies, which shadow catchers may name.
pub fn shape_names(spec: &ModelPhysics) -> Vec<String> {
    spec.bodies.iter().filter_map(|b| b.name.clone()).collect()
}
