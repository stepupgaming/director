//! The Hero level: a path tracer over Solari's ray traced scene.
//!
//! Solari's `RaytracingScenePlugin` keeps a TLAS of every entity with a
//! `RaytracingMesh3d`, its materials, its suns and the active camera's
//! environment. `HeroPlugin` adds one compute pass after the opaque and
//! transmissive passes that overwrites the view target with path traced
//! radiance for the camera that carries a `HeroPass`. The transparent pass
//! then draws Gaussian splats over it against the raster depth, and the
//! usual tonemapping, bloom and SMAA run.
//! Solari has no point or spot lights, so the pass samples them itself from
//! the `Lamp`s its camera carries.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use bevy::{
    asset::{embedded_asset, load_embedded_asset, RenderAssetUsages},
    core_pipeline::{
        core_3d::{main_opaque_pass_3d, main_transparent_pass_3d},
        schedule::{Core3d, Core3dSystems},
    },
    mesh::{
        morph::{MeshMorphWeights, MorphAttributes, MorphWeights},
        skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
        Indices, PrimitiveTopology, VertexAttributeValues,
    },
    prelude::*,
    render::{
        camera::ExtractedCamera,
        init_gpu_resource,
        render_resource::{
            binding_types::{storage_buffer_read_only_sized, texture_storage_2d, uniform_buffer, uniform_buffer_sized},
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor, BufferSize,
            BufferUsages, CachedComputePipelineId, ComputePassDescriptor, ComputePipelineDescriptor, PipelineCache,
            ShaderStages, StorageTextureAccess, TextureFormat,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        sync_world::RenderEntity,
        view::{ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
        Extract, ExtractSchedule, RenderApp, RenderStartup,
    },
    pbr::main_transmissive_pass_3d,
    solari::{scene::RaytracingSceneBindings, SolariPlugins},
};

/// The deepest bounce a path reaches before Russian roulette ends it.
const BOUNCES: u32 = 32;

/// Path traces the camera it sits on: `paths` paths per pixel for sample
/// `sample` of the shot, lit by `lamps` beside Solari's own lights.
#[derive(Component, Clone, Debug)]
pub(crate) struct HeroPass {
    pub sample: u32,
    pub paths: u32,
    pub background: bool,
    pub lamps: Arc<Vec<Lamp>>,
}

/// A point or spot light as the path tracer samples it, in Bevy's raster
/// units so both levels light a scene alike: `color` is the linear colour
/// times the luminous intensity (lumens / 4 pi), and a spot's cone falls
/// off as `saturate(cos * scale + offset)^2` about `direction`. A point light
/// has scale 0 and offset 1. A light with a `radius` casts soft shadows from
/// a disc of that radius facing each shaded point.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Lamp {
    pub position: Vec3,
    pub radius: f32,
    pub direction: Vec3,
    pub scale: f32,
    pub color: Vec3,
    pub offset: f32,
    pub range: f32,
}

impl Lamp {
    /// A light of `lumens` at `transform`, with `angles` (inner, outer) for
    /// a spot, matching Bevy's raster lights.
    pub fn new(transform: &GlobalTransform, color: Color, lumens: f32, range: f32, radius: f32, angles: Option<(f32, f32)>) -> Self {
        let (scale, offset) = match angles {
            Some((inner, outer)) => {
                let scale = 1.0 / (inner.cos() - outer.cos()).max(1e-4);
                (scale, -outer.cos() * scale)
            }
            None => (0.0, 1.0),
        };
        let linear = LinearRgba::from(color);
        Lamp {
            position: transform.translation(),
            radius,
            direction: transform.forward().into(),
            scale,
            color: Vec3::new(linear.red, linear.green, linear.blue) * lumens / (4.0 * std::f32::consts::PI),
            offset,
            range,
        }
    }

    /// The 64 bytes of `HeroLight` in `hero.wesl`.
    fn bytes(&self) -> [u8; LAMP as usize] {
        let words = [
            self.position.x,
            self.position.y,
            self.position.z,
            self.radius,
            self.direction.x,
            self.direction.y,
            self.direction.z,
            self.scale,
            self.color.x,
            self.color.y,
            self.color.z,
            self.offset,
            self.range,
            0.0,
            0.0,
            0.0,
        ];
        let mut out = [0; LAMP as usize];
        for (chunk, w) in out.as_chunks_mut::<4>().0.iter_mut().zip(words) {
            *chunk = w.to_le_bytes();
        }
        out
    }
}

/// Counts dispatches of the Hero pass, shared by the main and render worlds,
/// so the studio knows each sample was traced.
#[derive(Resource, Clone, Default)]
pub(crate) struct Traced(pub Arc<AtomicU32>);

impl Traced {
    pub fn count(&self) -> u32 {
        self.0.load(Ordering::Acquire)
    }
}

/// Whether the device can trace rays, set once the plugin finishes.
#[derive(Resource, Clone, Copy, Default)]
pub(crate) struct Traceable(pub bool);

pub(crate) struct HeroPlugin;

impl Plugin for HeroPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "hero.wesl");
        app.init_resource::<Traced>().init_resource::<Traceable>().add_systems(
            PostUpdate,
            gather
                .after(bevy::transform::TransformSystems::Propagate)
                .after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
    }

    fn finish(&self, app: &mut App) {
        let traced = app.world().resource::<Traced>().clone();
        let render = app.sub_app_mut(RenderApp);
        let features = render.world().resource::<RenderDevice>().features();
        if !features.contains(SolariPlugins::required_wgpu_features()) {
            return;
        }
        render
            .insert_resource(traced)
            .add_systems(RenderStartup, init.after(init_gpu_resource::<RaytracingSceneBindings>))
            .add_systems(ExtractSchedule, extract)
            .add_systems(
                Core3d,
                trace
                    .after(main_opaque_pass_3d)
                    .after(main_transmissive_pass_3d)
                    .before(main_transparent_pass_3d)
                    .in_set(Core3dSystems::MainPass),
            );
        app.insert_resource(Traceable(true));
    }
}

/// Gives each Hero camera the visible point and spot lights at their poses
/// this frame: the file's lights and the authored ones, minus the spots
/// that stand in for rect lights.
fn gather(
    mut cameras: Query<&mut HeroPass>,
    points: Query<(&PointLight, &GlobalTransform, &InheritedVisibility, Option<&crate::lights::Authored>)>,
    spots: Query<(&SpotLight, &GlobalTransform, &InheritedVisibility, Option<&crate::lights::Authored>)>,
) {
    if cameras.is_empty() {
        return;
    }
    let radius = |own: f32, authored: Option<&crate::lights::Authored>| authored.map_or(own, |a| a.radius);
    let mut lamps: Vec<Lamp> = points
        .iter()
        .filter(|(_, _, seen, _)| seen.get())
        .map(|(l, t, _, a)| Lamp::new(t, l.color, l.intensity, l.range, radius(l.radius, a), None))
        .collect();
    lamps.extend(
        spots
            .iter()
            .filter(|(_, _, seen, a)| seen.get() && !a.is_some_and(|a| a.rect))
            .map(|(l, t, _, a)| Lamp::new(t, l.color, l.intensity, l.range, radius(l.radius, a), Some((l.inner_angle, l.outer_angle)))),
    );
    for mut pass in &mut cameras {
        if *pass.lamps != lamps {
            pass.lamps = Arc::new(lamps.clone());
        }
    }
}

#[derive(Resource)]
struct HeroPipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline: CachedComputePipelineId,
}

/// The settings uniform: sample, paths, background, bounces, lamp count and
/// padding.
const SETTINGS: u64 = 32;

/// The size of one `HeroLight`.
const LAMP: u64 = 64;

fn init(mut commands: Commands, cache: Res<PipelineCache>, scene: Res<RaytracingSceneBindings>, server: Res<AssetServer>) {
    let layout = BindGroupLayoutDescriptor::new(
        "director_hero",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::WriteOnly),
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer_sized(false, BufferSize::new(SETTINGS)),
                storage_buffer_read_only_sized(false, BufferSize::new(LAMP)),
            ),
        ),
    );
    let pipeline = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("director_hero".into()),
        layout: vec![scene.bind_group_layout.clone(), layout.clone()],
        shader: load_embedded_asset!(server.as_ref(), "hero.wesl"),
        entry_point: Some("hero_trace".into()),
        ..default()
    });
    commands.insert_resource(HeroPipeline { layout, pipeline });
}

fn extract(cameras: Extract<Query<(RenderEntity, &Camera, Option<&HeroPass>)>>, mut commands: Commands) {
    for (entity, camera, pass) in &cameras {
        let Ok(mut e) = commands.get_entity(entity) else { continue };
        match pass.filter(|_| camera.is_active) {
            Some(pass) => e.insert(pass.clone()),
            None => e.remove::<HeroPass>(),
        };
    }
}

#[allow(clippy::too_many_arguments)]
fn trace(
    view: ViewQuery<(&HeroPass, &ExtractedCamera, &ViewTarget, &ViewUniformOffset)>,
    pipeline: Option<Res<HeroPipeline>>,
    cache: Res<PipelineCache>,
    scene: Res<RaytracingSceneBindings>,
    uniforms: Res<ViewUniforms>,
    device: Res<RenderDevice>,
    traced: Res<Traced>,
    mut ctx: RenderContext,
) {
    let (pass, camera, target, offset) = view.into_inner();
    let Some(pipeline) = pipeline else { return };
    let (Some(compute), Some(scene_group), Some(size), Some(view_binding)) = (
        cache.get_compute_pipeline(pipeline.pipeline),
        &scene.bind_group,
        camera.physical_viewport_size,
        uniforms.uniforms.binding(),
    ) else {
        return;
    };
    let lamps = pass.lamps.len() as u32;
    let settings: Vec<u8> = [pass.sample, pass.paths.max(1), pass.background as u32, BOUNCES, lamps, 0, 0, 0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let settings = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("director_hero_settings"),
        contents: &settings,
        usage: BufferUsages::UNIFORM,
    });
    // The buffer holds at least one light, because a binding is never empty.
    let mut lights: Vec<u8> = pass.lamps.iter().flat_map(|l| l.bytes()).collect();
    if lights.is_empty() {
        lights = Lamp::default().bytes().to_vec();
    }
    let lights = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("director_hero_lights"),
        contents: &lights,
        usage: BufferUsages::STORAGE,
    });
    let group = device.create_bind_group(
        "director_hero",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            target.get_unsampled_color_attachment().view,
            view_binding,
            settings.as_entire_binding(),
            lights.as_entire_binding(),
        )),
    );
    let mut compute_pass = ctx.command_encoder().begin_compute_pass(&ComputePassDescriptor {
        label: Some("director_hero"),
        timestamp_writes: None,
    });
    compute_pass.set_pipeline(compute);
    compute_pass.set_bind_group(0, scene_group, &[]);
    compute_pass.set_bind_group(1, &group, &[offset.offset]);
    compute_pass.dispatch_workgroups(size.x.div_ceil(8), size.y.div_ceil(8), 1);
    drop(compute_pass);
    traced.0.fetch_add(1, Ordering::AcqRel);
}

/// A copy of `mesh` in the vertex layout Solari builds acceleration
/// structures from: positions, normals, UVs and tangents, u32 indices, as a
/// triangle list. `None` for lines and points.
pub(crate) fn solari_copy(mesh: &Mesh) -> Option<Mesh> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION)? {
        VertexAttributeValues::Float32x3(p) => p.clone(),
        _ => return None,
    };
    let count = positions.len();
    let indices: Vec<u32> = match mesh.indices() {
        Some(indices) => indices.iter().map(|i| i as u32).collect(),
        None => (0..count as u32).collect(),
    };
    let mut out = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    out.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    out.insert_indices(Indices::U32(indices));
    match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(n)) => out.insert_attribute(Mesh::ATTRIBUTE_NORMAL, n.clone()),
        _ => out.compute_normals(),
    }
    let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(uv)) => Some(uv.clone()),
        _ => None,
    };
    let tangents = match mesh.attribute(Mesh::ATTRIBUTE_TANGENT) {
        Some(VertexAttributeValues::Float32x4(t)) => Some(t.clone()),
        _ => None,
    };
    let has_uvs = uvs.is_some();
    out.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs.unwrap_or_else(|| vec![[0.0, 0.0]; count]));
    match tangents {
        Some(t) => out.insert_attribute(Mesh::ATTRIBUTE_TANGENT, t),
        None => {
            if !has_uvs || out.generate_tangents().is_err() {
                out.insert_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0, 0.0, 0.0, 1.0]; count]);
            }
        }
    }
    Some(out)
}

/// The rest pose of a skinned or morphing mesh, which each Hero shot deforms
/// into its copy on the CPU, because Solari's acceleration structures hold
/// the vertices as given.
pub(crate) struct Rest {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    tangents: Vec<Vec4>,
    /// Morph target deltas, target-major.
    targets: Vec<MorphAttributes>,
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
}

/// The rest pose of `source`, read through its Solari `copy`, when it has
/// joints or morph targets.
pub(crate) fn rest(source: &Mesh, copy: &Mesh) -> Option<Rest> {
    let joints = match source.attribute(Mesh::ATTRIBUTE_JOINT_INDEX) {
        Some(VertexAttributeValues::Uint16x4(j)) => j.clone(),
        _ => Vec::new(),
    };
    let weights = match source.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT) {
        Some(VertexAttributeValues::Float32x4(w)) => w.clone(),
        _ => Vec::new(),
    };
    let targets = source.morph_targets().cloned().unwrap_or_default();
    if (joints.is_empty() || weights.is_empty()) && targets.is_empty() {
        return None;
    }
    let float3 = |attribute| match copy.attribute(attribute) {
        Some(VertexAttributeValues::Float32x3(v)) => v.iter().map(|&p| Vec3::from(p)).collect(),
        _ => Vec::new(),
    };
    let tangents = match copy.attribute(Mesh::ATTRIBUTE_TANGENT) {
        Some(VertexAttributeValues::Float32x4(t)) => t.iter().map(|&t| Vec4::from(t)).collect(),
        _ => Vec::new(),
    };
    Some(Rest {
        positions: float3(Mesh::ATTRIBUTE_POSITION),
        normals: float3(Mesh::ATTRIBUTE_NORMAL),
        tangents,
        targets,
        joints,
        weights,
    })
}

/// A traced mesh entity, its copy in Solari's layout, and the rest pose of a
/// copy that skins or morphs.
pub(crate) type TracedMesh = (Entity, Handle<Mesh>, Option<Arc<Rest>>);

/// Poses each copy with a rest pose like the rasterizer would this frame:
/// morph targets by their current weights, then skinning by the current
/// joint transforms, written in the mesh entity's own space.
pub(crate) fn deform(world: &mut World, traced: &[TracedMesh]) {
    for (entity, copy, rest) in traced {
        let Some(rest) = rest else { continue };
        let count = rest.positions.len();
        let mut positions = rest.positions.clone();
        let mut normals = rest.normals.clone();
        let mut tangents = rest.tangents.clone();
        let weights = match world.get::<MeshMorphWeights>(*entity) {
            Some(MeshMorphWeights::Value { weights }) => weights.clone(),
            Some(MeshMorphWeights::Reference(source)) => {
                world.get::<MorphWeights>(*source).map(|w| w.weights().to_vec()).unwrap_or_default()
            }
            None => Vec::new(),
        };
        for (target, &w) in weights.iter().enumerate() {
            let Some(deltas) = rest.targets.get(target * count..(target + 1) * count) else { break };
            if w == 0.0 {
                continue;
            }
            for (v, d) in deltas.iter().enumerate() {
                positions[v] += d.position * w;
                if let Some(n) = normals.get_mut(v) {
                    *n += d.normal * w;
                }
                if let Some(t) = tangents.get_mut(v) {
                    *t += (d.tangent * w).extend(0.0);
                }
            }
        }
        let skin = world.get::<SkinnedMesh>(*entity).and_then(|skin| {
            let bindposes = world.resource::<Assets<SkinnedMeshInverseBindposes>>().get(&skin.inverse_bindposes)?;
            let local = world.get::<GlobalTransform>(*entity)?.affine().inverse();
            let matrices: Vec<Mat4> = skin
                .joints
                .iter()
                .zip(bindposes.iter())
                .map(|(joint, bind)| {
                    let global = world.get::<GlobalTransform>(*joint).copied().unwrap_or_default();
                    Mat4::from(local) * Mat4::from(global.affine()) * *bind
                })
                .collect();
            Some(matrices)
        });
        if let (Some(matrices), false) = (&skin, rest.joints.is_empty()) {
            for (v, (&j, &w)) in rest.joints.iter().zip(&rest.weights).enumerate().take(count) {
                let m = (0..4)
                    .filter(|&k| w[k] != 0.0)
                    .fold(Mat4::ZERO, |m, k| m + matrices.get(j[k] as usize).copied().unwrap_or(Mat4::IDENTITY) * w[k]);
                positions[v] = m.transform_point3(positions[v]);
                let normal = Mat3::from_mat4(m).inverse().transpose();
                if let Some(n) = normals.get_mut(v) {
                    *n = normal * *n;
                }
                if let Some(t) = tangents.get_mut(v) {
                    *t = (Mat3::from_mat4(m) * t.truncate()).extend(t.w);
                }
            }
        }
        let normals: Vec<[f32; 3]> = normals.iter().map(|n| n.normalize_or(Vec3::Y).into()).collect();
        let tangents: Vec<[f32; 4]> = tangents
            .iter()
            .map(|t| t.truncate().normalize_or(Vec3::X).extend(t.w).into())
            .collect();
        let positions: Vec<[f32; 3]> = positions.iter().map(|&p| p.into()).collect();
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let Some(mut mesh) = meshes.get_mut(copy) else { continue };
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        if !normals.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        }
        if !tangents.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents);
        }
    }
}

/// Whether the shared device traces rays, so Hero shots render.
pub fn traceable() -> bool {
    crate::gpu::device().is_ok_and(|d| d.device.features().contains(SolariPlugins::required_wgpu_features()))
}
