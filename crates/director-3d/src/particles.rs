//! Particles: closed-form emitters drawn as camera-facing cards or small
//! meshes.
//!
//! An emitter's particles have no state to step. `particles.wesl` computes
//! each one in the vertex shader from the seed, its index and the element
//! time the material holds, so a frame renders the same whatever rendered
//! before it, and each sample of a shutter sets its own time. The mesh holds
//! one card or shape per slot, enough slots for every particle alive at
//! once, and slot j shows the latest particle whose index leaves remainder
//! j. Particles draw in the transparent pass with premultiplied alpha, lit
//! by the scene's lights, probe and fog and glowing by their emissive
//! strength. At the Hero level they draw over the traced image as splats
//! do, since Solari traces only standard materials.

use std::hash::{Hash, Hasher};

use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    camera::visibility::NoFrustumCulling,
    image::{ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin},
    prelude::*,
    render::{
        render_resource::{
            AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension,
            TextureFormat,
        },
        storage::ShaderBuffer,
    },
    mesh::MeshVertexBufferLayoutRef,
    shader::ShaderRef,
};
use director_scene::{ModelParticles, ParticleEmitter};

use crate::Asset as SourceAsset;

const SHADER: &str = "embedded://director_3d/particles.wesl";

/// The most particles one emitter keeps alive at once.
pub const MAX_PARTICLES: u32 = 1_000_000;

/// The most vertices one emitter's mesh may hold.
const MAX_VERTICES: u64 = 16_000_000;

/// Points a surface emitter births from, spread over the surface by area.
const SURFACE_POINTS: usize = 4096;

/// One emitter: its settings and the image its cards show.
#[derive(Clone, Debug)]
pub struct Particles {
    /// The emitter's settings. Its `sprite` asset id is ignored here.
    pub spec: ModelParticles,
    /// The image each card shows, `None` for a soft disc.
    pub sprite: Option<SourceAsset>,
}

/// Slots an emitter needs: every particle that can be alive at once.
pub fn slots(spec: &ModelParticles) -> u64 {
    if spec.rate > 0.0 {
        (spec.rate as f64 * spec.lifetime as f64).ceil() as u64 + 2
    } else {
        spec.burst as u64
    }
}

/// Checks an emitter's settings, naming the first one out of range.
pub fn check(spec: &ModelParticles) -> Result<(), String> {
    let finite = |v: &[f32]| v.iter().all(|n| n.is_finite());
    let positive = |name: &str, v: f32| {
        if v.is_finite() && v >= 0.0 { Ok(()) } else { Err(format!("particles {name} must be a number from 0")) }
    };
    let unit = |name: &str, v: f32| {
        if (0.0..=1.0).contains(&v) { Ok(()) } else { Err(format!("particles {name} must be a number from 0 to 1")) }
    };
    if !finite(&spec.position) || !finite(&spec.gravity) || !spec.direction.is_none_or(|d| finite(&d)) {
        return Err("particles position, direction and gravity must be three finite numbers".into());
    }
    positive("radius", spec.radius)?;
    if !finite(&spec.extent) || spec.extent.iter().any(|e| *e < 0.0) {
        return Err("particles extent must be three numbers from 0".into());
    }
    if spec.direction.is_some_and(|d| d.iter().all(|c| *c == 0.0)) {
        return Err("particles direction must not be 0 0 0".into());
    }
    match (spec.emitter, &spec.node) {
        (ParticleEmitter::Surface, None) => return Err("particles emitter surface needs a node".into()),
        (ParticleEmitter::Surface, Some(_)) | (_, None) => {}
        (_, Some(_)) => return Err("particles node needs emitter surface".into()),
    }
    positive("rate", spec.rate)?;
    match (spec.rate > 0.0, spec.burst > 0) {
        (true, true) => return Err("particles take a rate or a burst, not both".into()),
        (false, false) => return Err("particles need a rate or a burst above 0".into()),
        _ => {}
    }
    if !(spec.lifetime.is_finite() && spec.lifetime > 0.0) {
        return Err("particles lifetime must be above 0 seconds".into());
    }
    unit("lifetime-jitter", spec.lifetime_jitter)?;
    if !spec.start.is_finite() {
        return Err("particles start must be a number of seconds".into());
    }
    if spec.stop.is_some_and(|s| !s.is_finite() || s < spec.start) {
        return Err("particles stop must be a number of seconds from start".into());
    }
    if slots(spec) > MAX_PARTICLES as u64 {
        return Err(format!(
            "particles keep up to {} alive at once (rate times lifetime, or the burst), above {MAX_PARTICLES}",
            slots(spec)
        ));
    }
    positive("speed", spec.speed)?;
    unit("speed-jitter", spec.speed_jitter)?;
    if !(0.0..=180.0).contains(&spec.spread) {
        return Err("particles spread must be a number from 0 to 180 degrees".into());
    }
    positive("drag", spec.drag)?;
    positive("turbulence", spec.turbulence)?;
    if !(spec.turbulence_scale.is_finite() && spec.turbulence_scale > 0.0) {
        return Err("particles turbulence-scale must be above 0 metres".into());
    }
    positive("size", spec.size)?;
    if let Some(end) = spec.size_end {
        positive("size-end", end)?;
    }
    unit("size-jitter", spec.size_jitter)?;
    if !spec.spin.is_finite() {
        return Err("particles spin must be a number of degrees per second".into());
    }
    for c in spec.color.iter().chain(spec.color_end.iter().flatten()) {
        unit("color channels", *c)?;
    }
    unit("opacity", spec.opacity)?;
    positive("fade-in", spec.fade_in)?;
    positive("fade-out", spec.fade_out)?;
    positive("emissive", spec.emissive)?;
    if spec.sprite.is_some() && spec.mesh.is_some() {
        return Err("particles take a sprite src or a mesh, not both".into());
    }
    if spec.mesh.as_deref().is_some_and(str::is_empty) {
        return Err("particles mesh must be card, cube, tetra or a node name".into());
    }
    Ok(())
}

/// A hash of every emitter, which keys frames and the studio's spawned
/// emitters.
pub(crate) fn particles_hash(particles: &[Particles]) -> u64 {
    if particles.is_empty() {
        return 0;
    }
    let mut hasher = std::hash::DefaultHasher::new();
    for p in particles {
        serde_json::to_string(&p.spec).expect("particles serialize").hash(&mut hasher);
        p.sprite.as_ref().map(|s| s.hash).hash(&mut hasher);
    }
    hasher.finish()
}

/// Whether any emitter may show a particle between element seconds `t0`
/// and `t1`.
pub(crate) fn alive(particles: &[Particles], t0: f64, t1: f64) -> bool {
    particles.iter().any(|p| {
        let s = &p.spec;
        let last_birth = if s.rate > 0.0 { s.stop.map_or(f64::INFINITY, f64::from) } else { s.start as f64 };
        t0.max(t1) >= s.start as f64 && t0.min(t1) < last_birth + s.lifetime as f64
    })
}

/// The uniform `particles.wesl` reads.
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub(crate) struct Params {
    emitter: Vec4,
    extent: Vec4,
    direction: Vec4,
    gravity: Vec4,
    motion: Vec4,
    timing: Vec4,
    window: Vec4,
    size: Vec4,
    color: Vec4,
    color_end: Vec4,
    fade: Vec4,
    seed: UVec4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ParticleKey {
    /// Mesh particles write depth so they hide each other; cards blend.
    solid: bool,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(ParticleKey)]
pub(crate) struct ParticleMaterial {
    #[uniform(0)]
    params: Params,
    #[texture(1)]
    #[sampler(2)]
    sprite: Handle<Image>,
    #[storage(3, read_only)]
    surface: Handle<ShaderBuffer>,
    solid: bool,
}

impl From<&ParticleMaterial> for ParticleKey {
    fn from(material: &ParticleMaterial) -> Self {
        Self { solid: material.solid }
    }
}

impl Material for ParticleMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if key.bind_group_data.solid {
            if let Some(depth) = descriptor.depth_stencil.as_mut() {
                depth.depth_write_enabled = Some(true);
            }
        }
        Ok(())
    }
}

pub(crate) struct ParticlesPlugin;

impl Plugin for ParticlesPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "particles.wesl");
        app.add_plugins(MaterialPlugin::<ParticleMaterial>::default());
    }
}

/// An emitter spawned on a stage.
pub(crate) struct Spawned {
    entity: Entity,
    material: Handle<ParticleMaterial>,
}

/// Linear light from an sRGB channel.
fn linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn rgb(c: [f32; 3]) -> Vec3 {
    Vec3::new(linear(c[0]), linear(c[1]), linear(c[2]))
}

/// A soft white disc: alpha falls smoothly from the centre to the edge.
fn disc() -> (u32, Vec<u8>) {
    let size = 64u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let dy = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let r2 = (dx * dx + dy * dy).min(1.0);
            let a = (1.0 - r2) * (1.0 - r2);
            rgba.extend_from_slice(&[255, 255, 255, (a * 255.0).round() as u8]);
        }
    }
    (size, rgba)
}

/// An image file decoded to straight-alpha RGBA8 with its width and height.
pub(crate) fn decode_sprite(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(bytes))
        .ok_or("particles src is not an image Director can decode")?;
    let (w, h) = (image.width().max(1) as u32, image.height().max(1) as u32);
    let info = skia_safe::ImageInfo::new(
        (w as i32, h as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    if !image.read_pixels(&info, &mut rgba, w as usize * 4, (0, 0), skia_safe::image::CachingHint::Disallow) {
        return Err("particles src could not be read".into());
    }
    Ok((w, h, rgba))
}

/// An sRGB image with a full mip chain, each level a box filter of the one
/// above in premultiplied alpha, so small cards stay smooth.
fn mipmapped(width: u32, height: u32, rgba: Vec<u8>) -> Image {
    let mut data = rgba.clone();
    let (mut w, mut h, mut level) = (width, height, rgba);
    let mut levels = 1;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let mut sum = [0f32; 4];
                let mut n = 0.0;
                for (sx, sy) in [(2 * x, 2 * y), (2 * x + 1, 2 * y), (2 * x, 2 * y + 1), (2 * x + 1, 2 * y + 1)] {
                    let (sx, sy) = (sx.min(w - 1), sy.min(h - 1));
                    let i = ((sy * w + sx) * 4) as usize;
                    let a = level[i + 3] as f32 / 255.0;
                    for c in 0..3 {
                        sum[c] += linear(level[i + c] as f32 / 255.0) * a;
                    }
                    sum[3] += a;
                    n += 1.0;
                }
                let o = ((y * nw + x) * 4) as usize;
                let a = sum[3] / n;
                for c in 0..3 {
                    let v = if a > 0.0 { sum[c] / n / a } else { 0.0 };
                    let s = if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
                    next[o + c] = (s * 255.0).round().clamp(0.0, 255.0) as u8;
                }
                next[o + 3] = (a * 255.0).round() as u8;
            }
        }
        data.extend_from_slice(&next);
        (w, h, level) = (nw, nh, next);
        levels += 1;
    }
    let mut image = Image::new(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    image
}

/// Triangles of every mesh at or below the node called `name`, in the
/// stage root's space.
fn node_triangles(world: &World, root: Entity, name: &str) -> Result<Vec<[Vec3; 3]>, String> {
    let named: Vec<Entity> = crate::studio::descendants(world, root)
        .into_iter()
        .filter(|&e| world.get::<Name>(e).is_some_and(|n| n.as_str() == name))
        .collect();
    let meshes = world.resource::<Assets<Mesh>>();
    let mut triangles = Vec::new();
    for node in named {
        for entity in std::iter::once(node).chain(crate::studio::descendants(world, node)) {
            let Some(handle) = world.get::<Mesh3d>(entity) else { continue };
            let Some(mesh) = meshes.get(&handle.0) else { continue };
            if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
                continue;
            }
            let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
                continue;
            };
            let to_root = crate::studio::world_transform(world, entity, root).to_matrix();
            let points: Vec<Vec3> = positions.iter().map(|p| to_root.transform_point3(Vec3::from(*p))).collect();
            let corners: Vec<usize> = match mesh.indices() {
                Some(indices) => indices.iter().collect(),
                None => (0..points.len()).collect(),
            };
            for t in corners.as_chunks::<3>().0 {
                triangles.push(t.map(|i| points[i]));
            }
        }
    }
    if triangles.is_empty() {
        return Err(format!("particles node '{name}' names no node with a mesh in the model"));
    }
    Ok(triangles)
}

/// `SURFACE_POINTS` points with their face normals, stratified over the
/// triangles by area, as position and normal pairs.
fn surface_points(triangles: &[[Vec3; 3]], seed: u32) -> Vec<Vec4> {
    let areas: Vec<f32> = triangles.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).length() / 2.0).collect();
    let total: f32 = areas.iter().sum();
    let mut out = Vec::with_capacity(SURFACE_POINTS * 2);
    let (mut tri, mut below) = (0, 0.0);
    let mut state = seed as u64 ^ 0x9e37_79b9_7f4a_7c15;
    let mut random = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 40) as f32) / (1u64 << 24) as f32
    };
    for i in 0..SURFACE_POINTS {
        let target = (i as f32 + random()) / SURFACE_POINTS as f32 * total;
        while tri + 1 < triangles.len() && below + areas[tri] < target {
            below += areas[tri];
            tri += 1;
        }
        let [a, b, c] = triangles[tri];
        let (mut u, mut v) = (random(), random());
        if u + v > 1.0 {
            (u, v) = (1.0 - u, 1.0 - v);
        }
        let p = a + (b - a) * u + (c - a) * v;
        let n = (b - a).cross(c - a).try_normalize().unwrap_or(Vec3::Y);
        out.push(p.extend(1.0));
        out.push(n.extend(0.0));
    }
    out
}

/// A shape's flat-shaded triangles, scaled to a unit bounding diameter and
/// centred on the origin.
fn shape(world: &World, root: Entity, name: &str) -> Result<Vec<[Vec3; 3]>, String> {
    let s = 0.5;
    let quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3| [[a, b, c], [a, c, d]];
    let triangles: Vec<[Vec3; 3]> = match name {
        "card" => quad(Vec3::new(-s, -s, 0.0), Vec3::new(s, -s, 0.0), Vec3::new(s, s, 0.0), Vec3::new(-s, s, 0.0)).to_vec(),
        "cube" => {
            let v = |x: f32, y: f32, z: f32| Vec3::new(x, y, z) * s;
            let faces = [
                quad(v(-1., -1., 1.), v(1., -1., 1.), v(1., 1., 1.), v(-1., 1., 1.)),
                quad(v(1., -1., -1.), v(-1., -1., -1.), v(-1., 1., -1.), v(1., 1., -1.)),
                quad(v(1., -1., 1.), v(1., -1., -1.), v(1., 1., -1.), v(1., 1., 1.)),
                quad(v(-1., -1., -1.), v(-1., -1., 1.), v(-1., 1., 1.), v(-1., 1., -1.)),
                quad(v(-1., 1., 1.), v(1., 1., 1.), v(1., 1., -1.), v(-1., 1., -1.)),
                quad(v(-1., -1., -1.), v(1., -1., -1.), v(1., -1., 1.), v(-1., -1., 1.)),
            ];
            faces.into_iter().flatten().collect()
        }
        "tetra" => {
            let p = [Vec3::new(1., 1., 1.), Vec3::new(1., -1., -1.), Vec3::new(-1., 1., -1.), Vec3::new(-1., -1., 1.)]
                .map(|p| p * s);
            vec![[p[0], p[1], p[2]], [p[0], p[3], p[1]], [p[0], p[2], p[3]], [p[1], p[3], p[2]]]
        }
        node => node_triangles(world, root, node)?,
    };
    let (lo, hi) = triangles.iter().flatten().fold((Vec3::MAX, Vec3::MIN), |(l, h), p| (l.min(*p), h.max(*p)));
    let centre = (lo + hi) / 2.0;
    let diameter = (hi - lo).length().max(1e-6);
    Ok(triangles.into_iter().map(|t| t.map(|p| (p - centre) / diameter)).collect())
}

/// The mesh of `slots` cards, or of `slots` copies of `triangles`, with the
/// slot in UV_1.x.
fn slot_mesh(slots: u32, triangles: Option<&[[Vec3; 3]]>) -> Mesh {
    let (mut positions, mut normals, mut uvs, mut slot_uvs, mut indices) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for slot in 0..slots {
        let s = slot as f32;
        let base = positions.len() as u32;
        match triangles {
            None => {
                for uv in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]] {
                    positions.push([0.0f32; 3]);
                    normals.push([0.0, 0.0, 1.0]);
                    uvs.push(uv);
                    slot_uvs.push([s, 0.0]);
                }
                indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
            }
            Some(triangles) => {
                for [a, b, c] in triangles {
                    let n = (*b - *a).cross(*c - *a).try_normalize().unwrap_or(Vec3::Z);
                    for p in [a, b, c] {
                        indices.push(positions.len() as u32);
                        positions.push(p.to_array());
                        normals.push(n.to_array());
                        uvs.push([0.5, 0.5]);
                        slot_uvs.push([s, 0.0]);
                    }
                }
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, slot_uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// The uniform for `spec` at element seconds `time`.
fn params(spec: &ModelParticles, surface: usize, mesh: bool, time: f64) -> Params {
    let shape = match spec.emitter {
        ParticleEmitter::Point => 0.0,
        ParticleEmitter::Sphere => 1.0,
        ParticleEmitter::Box => 2.0,
        ParticleEmitter::Surface => 3.0,
    };
    let extent = match spec.emitter {
        ParticleEmitter::Sphere => Vec3::new(spec.radius, 0.0, 0.0),
        _ => Vec3::from(spec.extent) / 2.0,
    };
    let direction = match (spec.direction, spec.emitter) {
        (Some(d), _) => Vec3::from(d).normalize_or(Vec3::Y),
        (None, ParticleEmitter::Surface) => Vec3::ZERO,
        (None, _) => Vec3::Y,
    };
    let color = rgb(spec.color);
    let color_end = spec.color_end.map_or(color, rgb);
    Params {
        emitter: Vec3::from(spec.position).extend(shape),
        extent: extent.extend(surface as f32),
        direction: direction.extend(spec.spread.to_radians() / 2.0),
        gravity: Vec3::from(spec.gravity).extend(spec.drag),
        motion: Vec4::new(spec.speed, spec.speed_jitter, spec.turbulence, spec.turbulence_scale),
        timing: Vec4::new(time as f32, spec.rate, spec.lifetime, spec.lifetime_jitter),
        window: Vec4::new(spec.start, spec.stop.unwrap_or(1e30), spec.burst as f32, slots(spec) as f32),
        size: Vec4::new(spec.size, spec.size_end.unwrap_or(spec.size), spec.size_jitter, spec.spin.to_radians()),
        color: color.extend(spec.opacity),
        color_end: color_end.extend(spec.emissive),
        fade: Vec4::new(spec.fade_in, spec.fade_out, mesh as u32 as f32, 1.0),
        seed: UVec4::new(spec.seed, 0, 0, 0),
    }
}

/// Spawns every emitter under the stage `root`.
pub(crate) fn spawn(world: &mut World, root: Entity, particles: &[Particles], time: f64) -> Result<Vec<Spawned>, String> {
    let mut out = Vec::with_capacity(particles.len());
    for p in particles {
        let spec = &p.spec;
        let slots = slots(spec) as u32;
        let points = match (&spec.node, spec.emitter) {
            (Some(node), ParticleEmitter::Surface) => surface_points(&node_triangles(world, root, node)?, spec.seed),
            _ => vec![Vec4::ZERO; 2],
        };
        let surface = if spec.emitter == ParticleEmitter::Surface { points.len() / 2 } else { 0 };
        let triangles = match &spec.mesh {
            Some(name) => Some(shape(world, root, name)?),
            None => None,
        };
        let vertices = slots as u64 * triangles.as_ref().map_or(4, |t| t.len() as u64 * 3);
        if vertices > MAX_VERTICES {
            return Err(format!(
                "particles draw {vertices} vertices ({slots} particles of their mesh), above {MAX_VERTICES}"
            ));
        }
        let sprite = match &p.sprite {
            Some(asset) => {
                let (w, h, rgba) = decode_sprite(&asset.bytes)?;
                mipmapped(w, h, rgba)
            }
            None => {
                let (size, rgba) = disc();
                mipmapped(size, size, rgba)
            }
        };
        let sprite = world.resource_mut::<Assets<Image>>().add(sprite);
        let surface_buffer = world.resource_mut::<Assets<ShaderBuffer>>().add(ShaderBuffer::from(points));
        let mesh = world.resource_mut::<Assets<Mesh>>().add(slot_mesh(slots, triangles.as_deref()));
        let solid = triangles.is_some();
        let mut uniform = params(spec, surface, solid, time);
        uniform.fade.w = 1.0;
        let material = world.resource_mut::<Assets<ParticleMaterial>>().add(ParticleMaterial {
            params: uniform,
            sprite,
            surface: surface_buffer,
            solid,
        });
        let entity = world
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                NoFrustumCulling,
                bevy::light::NotShadowCaster,
                Transform::IDENTITY,
                ChildOf(root),
            ))
            .id();
        out.push(Spawned { entity, material });
    }
    Ok(out)
}

/// Shows every spawned emitter at element seconds `time`.
pub(crate) fn set_time(world: &mut World, spawned: &[Spawned], time: f64) {
    let mut materials = world.resource_mut::<Assets<ParticleMaterial>>();
    for s in spawned {
        if let Some(mut m) = materials.get_mut(&s.material) {
            m.params.timing.x = time as f32;
        }
    }
}

pub(crate) fn despawn(world: &mut World, spawned: Vec<Spawned>) {
    for s in spawned {
        world.despawn(s.entity);
        world.resource_mut::<Assets<ParticleMaterial>>().remove(&s.material);
    }
}
