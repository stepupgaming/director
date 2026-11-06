//! The studio: one headless Bevy app on the shared Vulkan device, owned by
//! its own thread.
//!
//! Every model in the process loads into this app once as a stage. A shot
//! picks a stage, seeks its clips, places the camera and renders one frame
//! into an sRGB target of the requested size. The frame either reads back to
//! the CPU or stays on the GPU as a Vulkan image ready for sampling. A shot
//! sets every input it depends on, so shots render the same in any order.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock};

use ash::vk::Handle as _;
use bevy::{
    anti_alias::smaa::Smaa,
    app::{AnimationSystems, AppLabel, SubApps},
    asset::{
        io::{
            memory::{Dir, MemoryAssetReader},
            AssetSourceBuilder,
        },
        RenderAssetUsages,
    },
    camera::{primitives::Aabb, Exposure, Hdr, ImageRenderTarget, RenderTarget},
    core_pipeline::tonemapping::Tonemapping,
    gltf::GltfAssetLabel,
    image::Image,
    light::{GeneratedEnvironmentMapLight, Skybox},
    log::LogPlugin,
    mesh::morph::MeshMorphWeights,
    post_process::bloom::Bloom,
    prelude::*,
    render::{
        camera::TemporalJitter,
        render_asset::RenderAssets,
        render_resource::{
            Extent3d, PollType, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
        },
        renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue},
        settings::{RenderCreation, RenderResources},
        texture::GpuImage,
        RenderApp, RenderPlugin,
    },
    transform::TransformSystems,
    window::ExitCondition,
    world_serialization::WorldAssetRoot,
};
use director_scene::ModelView;

use crate::accumulate::Accumulator;
use crate::Asset;

/// Everything one frame of a model depends on.
#[derive(Clone, Debug)]
pub struct Shot {
    pub model: Asset,
    pub clips: usize,
    pub environment: Option<Asset>,
    pub environment_intensity: f32,
    pub exposure: f32,
    pub background: bool,
    pub view: ModelView,
    /// Clip seconds after looping or holding.
    pub time: f64,
    pub width: u32,
    pub height: u32,
    /// Jittered sub-pixel renders averaged into the frame.
    pub samples: u32,
}

/// Premultiplied sRGB RGBA8 pixels, rows packed.
pub struct Frame {
    pub rgba: Vec<u8>,
}

/// A frame left on the shared device: an `R8G8B8A8_SRGB` Vulkan image that
/// allows an `R8G8B8A8_UNORM` view, in `SHADER_READ_ONLY_OPTIMAL` layout,
/// holding premultiplied sRGB colour. It stays valid until the next shot of
/// the same size.
#[derive(Clone, Copy, Debug)]
pub struct Texture {
    pub image: u64,
    pub width: u32,
    pub height: u32,
}

enum Rendered {
    Pixels(Frame),
    Texture(Texture),
}

type Job = (Shot, bool, mpsc::Sender<Result<Rendered, String>>);

static STUDIO: OnceLock<Result<mpsc::Sender<Job>, String>> = OnceLock::new();

/// Whether this machine has a Vulkan adapter for the studio.
pub fn available() -> bool {
    STUDIO.get_or_init(start).is_ok()
}

fn send(shot: Shot, texture: bool) -> Result<Rendered, String> {
    let studio = STUDIO.get_or_init(start).as_ref().map_err(Clone::clone)?;
    let (reply, answer) = mpsc::channel();
    studio.send((shot, texture, reply)).map_err(|_| "the 3D studio stopped".to_string())?;
    answer.recv().map_err(|_| "the 3D studio stopped".to_string())?
}

/// Held from a shot until its frame is no longer read, so no other shot
/// refilms a target while Skia samples it.
static LEASE: Mutex<()> = Mutex::new(());

fn lease() -> MutexGuard<'static, ()> {
    LEASE.lock().unwrap_or_else(|e| e.into_inner())
}

/// A texture shot and the lease that keeps its target unchanged.
pub struct Lease {
    pub texture: Texture,
    _guard: MutexGuard<'static, ()>,
}

/// Renders one shot on the studio thread and reads it back.
pub fn render(shot: Shot) -> Result<Frame, String> {
    let _guard = lease();
    match send(shot, false)? {
        Rendered::Pixels(frame) => Ok(frame),
        Rendered::Texture(_) => Err("the studio answered a pixel shot with a texture".into()),
    }
}

/// Renders one shot on the studio thread and leaves it on the shared device.
/// The texture stays valid while the returned lease lives.
pub fn render_texture(shot: Shot) -> Result<Lease, String> {
    let guard = lease();
    match send(shot, true)? {
        Rendered::Texture(texture) => Ok(Lease { texture, _guard: guard }),
        Rendered::Pixels(_) => Err("the studio answered a texture shot with pixels".into()),
    }
}

fn start() -> Result<mpsc::Sender<Job>, String> {
    crate::gpu::device()?;
    let (jobs, queue) = mpsc::channel::<Job>();
    let (ready, booted) = mpsc::channel();
    std::thread::Builder::new()
        .name("director-3d".into())
        .spawn(move || {
            let mut studio = match std::panic::catch_unwind(Studio::new) {
                Ok(studio) => {
                    let _ = ready.send(Ok(()));
                    studio
                }
                Err(_) => {
                    let _ = ready.send(Err("Bevy failed to start".to_string()));
                    return;
                }
            };
            for (shot, texture, reply) in queue {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| studio.shoot(&shot, texture)))
                    .unwrap_or_else(|_| Err("the 3D studio panicked".to_string()));
                let _ = reply.send(result);
            }
        })
        .map_err(|e| format!("3D studio thread: {e}"))?;
    booted.recv().map_err(|_| "the 3D studio stopped".to_string())??;
    Ok(jobs)
}

/// Marks the camera Director renders a stage through.
#[derive(Component)]
struct DirectorCamera;

/// The camera placement the orbit system writes before transforms propagate.
#[derive(Resource, Default)]
struct Orbit(Option<Placement>);

#[derive(Clone, Copy)]
struct Placement {
    camera: Entity,
    /// The file's camera, whose animated pose and lens the shot starts from.
    source: Option<Entity>,
    center: Vec3,
    radius: f32,
    view: ModelView,
    aspect: f32,
    /// Target height in pixels.
    height: f32,
    /// This sample's sub-pixel offset in pixels.
    jitter: Vec2,
    /// This sample's point on the unit lens disk.
    lens: Vec2,
}

/// Framing lens for a file without a camera: 30 degrees vertical.
const FRAMING_FOV: f32 = std::f32::consts::PI / 6.0;

/// Places the director camera from the file's camera or the framing camera,
/// then orbits it about the bounding sphere centre. Runs after animation, so
/// the file camera's pose is this frame's.
fn orbit(
    orbit: Res<Orbit>,
    nodes: Query<(&Transform, Option<&ChildOf>), Without<DirectorCamera>>,
    lenses: Query<&Projection, Without<DirectorCamera>>,
    mut cameras: Query<
        (&mut Transform, &mut Projection, Option<&mut TemporalJitter>),
        With<DirectorCamera>,
    >,
) {
    let Some(p) = orbit.0 else { return };
    let Ok((mut transform, mut projection, jitter)) = cameras.get_mut(p.camera) else { return };
    let (base, mut lens) = match p.source {
        Some(source) => {
            let mut world = Transform::IDENTITY;
            let mut at = Some(source);
            while let Some(entity) = at {
                let Ok((local, parent)) = nodes.get(entity) else { break };
                world = local.mul_transform(world);
                at = parent.map(|parent| parent.parent());
            }
            let lens = lenses.get(source).cloned().unwrap_or_default();
            (world, lens)
        }
        None => {
            let half_v = FRAMING_FOV / 2.0;
            let half_h = (half_v.tan() * p.aspect).atan();
            let distance = p.radius / half_v.min(half_h).sin() * 1.05;
            let lens = Projection::Perspective(PerspectiveProjection {
                fov: FRAMING_FOV,
                near: (distance - p.radius * 2.0).max(p.radius * 0.01).max(1e-4),
                far: distance + p.radius * 8.0,
                ..default()
            });
            (Transform::from_translation(p.center + Vec3::Z * distance), lens)
        }
    };
    let yaw = Quat::from_rotation_y(p.view.yaw.to_radians());
    let right = yaw * (base.rotation * Vec3::X);
    let pitch = Quat::from_axis_angle(right, -p.view.pitch.clamp(-89.0, 89.0).to_radians());
    let turn = pitch * yaw;
    let offset = (base.translation - p.center) * p.view.distance.max(1e-3);
    *transform = Transform {
        translation: p.center + turn * offset,
        rotation: turn * base.rotation,
        scale: Vec3::ONE,
    };
    if let Projection::Perspective(perspective) = &mut lens {
        perspective.aspect_ratio = p.aspect;
        if p.view.distance > 1.0 {
            perspective.far *= p.view.distance;
        }
    }
    // The lens: the focus plane sits `focus` metres ahead, or at the centre's
    // depth. Each sample moves the eye across an aperture of focal length /
    // f-number and shifts its image so the focus plane stays put, which is a
    // thin lens.
    let mut shift = Vec2::ZERO;
    if let (Projection::Perspective(perspective), true) = (&lens, p.view.aperture > 0.0) {
        let forward = transform.rotation * Vec3::NEG_Z;
        let focus = if p.view.focus > 0.0 {
            p.view.focus
        } else {
            (p.center - transform.translation).dot(forward).max(1e-3)
        };
        let half = (perspective.fov / 2.0).tan();
        if jitter.is_some() {
            let focal = 0.5 * SENSOR_HEIGHT / half;
            let eye = p.lens * focal / (2.0 * p.view.aperture);
            let step = transform.rotation * eye.extend(0.0);
            transform.translation += step;
            // Bevy's jitter moves the image by -2 * offset / size in NDC on
            // x and +2 * offset / size on y.
            shift = Vec2::new(-eye.x, eye.y) * p.height / (2.0 * half * focus);
        }
    }
    if let Some(mut jitter) = jitter {
        jitter.offset = p.jitter + shift;
    }
    *projection = lens;
}

/// Bevy's default sensor: Super 35, 18.66 mm tall.
const SENSOR_HEIGHT: f32 = 0.01866;

/// The fewest renders a lens with an aperture averages.
const LENS_SAMPLES: u32 = 16;

struct Stage {
    root: Entity,
    camera: Entity,
    source: Option<Entity>,
    players: Vec<Entity>,
    nodes: Vec<AnimationNodeIndex>,
    center: Vec3,
    radius: f32,
    probe: Handle<Image>,
    /// Target sizes this stage has rendered, so their pipelines and the
    /// filtered probe exist before a shot's frame.
    warm: HashSet<(u32, u32)>,
    /// The scene holds morph-target meshes.
    morphs: bool,
}

struct Target {
    image: Handle<Image>,
    buffer: wgpu::Buffer,
    row: u32,
    /// The shot whose frame the target holds.
    holds: Option<ShotKey>,
}

type StageKey = (u64, Option<u64>);
type ShotKey = (StageKey, usize, u64, [u32; 9]);

fn shot_key(stage: StageKey, shot: &Shot) -> ShotKey {
    (
        stage,
        shot.clips,
        shot.time.to_bits(),
        [
            shot.view.yaw.to_bits(),
            shot.view.pitch.to_bits(),
            shot.view.distance.to_bits(),
            shot.view.focus.to_bits(),
            shot.view.aperture.to_bits(),
            shot.exposure.to_bits(),
            shot.environment_intensity.to_bits(),
            shot.background as u32,
            shot.samples,
        ],
    )
}

struct Studio {
    apps: SubApps,
    dir: Dir,
    stages: HashMap<StageKey, Stage>,
    targets: HashMap<(u32, u32), Target>,
    accumulator: Option<Accumulator>,
    /// The stage the last shot rendered.
    shown: Option<StageKey>,
}

impl Studio {
    fn new() -> Self {
        let dir = Dir::new(PathBuf::new());
        let reader = dir.clone();
        let shared = crate::gpu::device().expect("the studio starts after the device");
        let resources = RenderResources(
            RenderDevice::from(shared.device.clone()),
            RenderQueue::new(shared.queue.clone()),
            RenderAdapterInfo::new(shared.adapter.get_info()),
            RenderAdapter::new(shared.adapter.clone()),
            RenderInstance::new(shared.instance.clone()),
            Default::default(),
        );
        let mut app = App::new();
        app.register_asset_source(
            "director",
            AssetSourceBuilder::new(move || Box::new(MemoryAssetReader { root: reader.clone() })),
        );
        app.add_plugins(
            DefaultPlugins
                .build()
                .disable::<LogPlugin>()
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    render_creation: RenderCreation::Manual(Box::new(resources)),
                    synchronous_pipeline_compilation: true,
                    ..default()
                }),
        );
        app.init_resource::<Orbit>();
        app.add_systems(PostUpdate, orbit.after(AnimationSystems).before(TransformSystems::Propagate));
        app.finish();
        app.cleanup();
        let apps = std::mem::take(app.sub_apps_mut());
        Self {
            apps,
            dir,
            stages: HashMap::new(),
            targets: HashMap::new(),
            accumulator: None,
            shown: None,
        }
    }

    fn step(&mut self) {
        self.apps.update();
        self.apps
            .main
            .world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("wgpu poll");
    }

    fn shoot(&mut self, shot: &Shot, texture: bool) -> Result<Rendered, String> {
        let key = self.stage(shot)?;
        let size = (shot.width, shot.height);
        self.target(size);
        let holds = Some(shot_key(key, shot));
        if self.targets[&size].holds != holds {
            self.film(key, shot, size)?;
            self.targets.get_mut(&size).expect("target").holds = holds;
        }
        if texture {
            self.texture(size).map(Rendered::Texture)
        } else {
            self.read(size).map(Rendered::Pixels)
        }
    }

    /// Renders `shot` of stage `key` into the target of `size`.
    fn film(&mut self, key: StageKey, shot: &Shot, size: (u32, u32)) -> Result<(), String> {
        let target = self.targets[&size].image.clone();
        let stage = &self.stages[&key];
        let (camera, source, center, radius, probe) = (stage.camera, stage.source, stage.center, stage.radius, stage.probe.clone());
        let players = stage.players.clone();
        let nodes = stage.nodes.clone();
        let warm = stage.warm.contains(&size);
        let morphs = stage.morphs;
        let others: Vec<(Entity, Entity)> = self
            .stages
            .iter()
            .filter(|(k, _)| **k != key)
            .map(|(_, s)| (s.root, s.camera))
            .collect();
        let world = self.apps.main.world_mut();
        for (root, other) in others {
            world.entity_mut(root).insert(Visibility::Hidden);
            if let Some(mut c) = world.get_mut::<Camera>(other) {
                c.is_active = false;
            }
        }
        world.entity_mut(stage_root(&self.stages, key)).insert(Visibility::Inherited);
        {
            let mut e = world.entity_mut(camera);
            e.insert((
                RenderTarget::Image(ImageRenderTarget {
                    handle: target,
                    scale_factor: 1.0,
                }),
                Exposure { ev100: shot.exposure },
                GeneratedEnvironmentMapLight {
                    environment_map: probe.clone(),
                    intensity: shot.environment_intensity,
                    ..default()
                },
            ));
            if shot.background {
                e.insert(Skybox {
                    image: Some(probe),
                    brightness: shot.environment_intensity,
                    ..default()
                });
            } else {
                e.remove::<Skybox>();
            }
            let mut c = e.get_mut::<Camera>().expect("director camera");
            c.is_active = true;
            c.clear_color = ClearColorConfig::Custom(Color::NONE);
        }
        for player in players {
            if let Some(mut player) = world.get_mut::<AnimationPlayer>(player) {
                for node in &nodes {
                    if let Some(active) = player.animation_mut(*node) {
                        active.seek_to(shot.time as f32);
                    }
                }
            }
        }
        world.resource_mut::<Orbit>().0 = Some(Placement {
            camera,
            source,
            center,
            radius,
            view: shot.view,
            aspect: shot.width as f32 / shot.height as f32,
            height: shot.height as f32,
            jitter: Vec2::ZERO,
            lens: Vec2::ZERO,
        });
        // A lens averages at least `LENS_SAMPLES` eye positions, at either
        // level, so preview and export blur alike.
        let samples = if shot.view.aperture > 0.0 {
            shot.samples.max(LENS_SAMPLES)
        } else {
            shot.samples.max(1)
        };
        if samples > 1 {
            world.entity_mut(camera).insert(TemporalJitter::default());
        } else {
            world.entity_mut(camera).remove::<TemporalJitter>();
        }
        if !warm {
            for _ in 0..3 {
                self.step();
            }
            self.stages.get_mut(&key).expect("stage").warm.insert(size);
        }
        // A morph mesh that just came back into view gets its morph
        // descriptor one frame after its mesh instance (see
        // `touch_morph_meshes`), so a stage switch takes one extra frame.
        if warm && morphs && self.shown != Some(key) {
            self.step();
        }
        self.shown = Some(key);
        if morphs {
            touch_morph_meshes(self.apps.main.world_mut());
        }
        for sample in 0..samples {
            if samples > 1 {
                let mut orbit = self.apps.main.world_mut().resource_mut::<Orbit>();
                let p = orbit.0.as_mut().expect("placement");
                p.jitter = jitter(sample);
                p.lens = lens(sample);
            }
            // Queue order keeps each sample's render ahead of its share, so
            // only the last sample waits for the GPU.
            if sample + 1 == samples {
                self.step();
            } else {
                self.apps.update();
            }
            if samples > 1 {
                let (device, queue, texture) = self.gpu_target(size)?;
                self.accumulator
                    .get_or_insert_with(|| Accumulator::new(&device))
                    .add(&device, &queue, &texture, sample, samples);
            }
        }
        if samples > 1 {
            let (device, queue, texture) = self.gpu_target(size)?;
            self.accumulator.as_ref().expect("accumulator").resolve(&device, &queue, &texture);
        }
        Ok(())
    }

    /// The render device, queue and GPU texture of the target of `size`.
    fn gpu_target(&self, size: (u32, u32)) -> Result<(wgpu::Device, wgpu::Queue, wgpu::Texture), String> {
        let render = self
            .apps
            .sub_apps
            .get(&RenderApp.intern())
            .ok_or("no render app")?
            .world();
        let gpu = render
            .resource::<RenderAssets<GpuImage>>()
            .get(&self.targets[&size].image)
            .ok_or("render target is not on the GPU")?;
        Ok((
            render.resource::<RenderDevice>().wgpu_device().clone(),
            (**render.resource::<RenderQueue>()).clone(),
            (*gpu.texture).clone(),
        ))
    }

    /// Loads the model and probe of `shot` once, keyed by their bytes.
    fn stage(&mut self, shot: &Shot) -> Result<StageKey, String> {
        let key = (shot.model.hash, shot.environment.as_ref().map(|e| e.hash));
        if self.stages.contains_key(&key) {
            return Ok(key);
        }
        let model = format!("{:016x}.glb", shot.model.hash);
        self.dir.insert_asset(Path::new(&model), shot.model.bytes.to_vec());
        let environment = shot.environment.as_ref().map(|e| {
            let name = format!("{:016x}.{}", e.hash, e.ext);
            self.dir.insert_asset(Path::new(&name), e.bytes.to_vec());
            name
        });
        let world = self.apps.main.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let path = format!("director://{model}");
        let scene = server.load(GltfAssetLabel::Scene(0).from_asset(path.clone()));
        let clips: Vec<Handle<AnimationClip>> = (0..shot.clips)
            .map(|i| server.load(GltfAssetLabel::Animation(i).from_asset(path.clone())))
            .collect();
        let (graph, nodes) = AnimationGraph::from_clips(clips);
        let graph = world.resource_mut::<Assets<AnimationGraph>>().add(graph);
        let equirect: Option<Handle<Image>> = environment.map(|name| server.load(format!("director://{name}")));
        let root = world.spawn((WorldAssetRoot(scene.clone()), Transform::IDENTITY, Visibility::Hidden)).id();

        let mut loaded = false;
        for _ in 0..4000 {
            self.step();
            let world = self.apps.main.world_mut();
            let server = world.resource::<AssetServer>();
            if let Some(bevy::asset::LoadState::Failed(e)) = server.get_load_state(&scene) {
                return Err(format!("glTF load failed: {e}"));
            }
            if let Some(h) = &equirect {
                if let Some(bevy::asset::LoadState::Failed(e)) = server.get_load_state(h) {
                    return Err(format!("environment load failed: {e}"));
                }
            }
            let spawned = world.get::<Children>(root).is_some_and(|c| !c.is_empty());
            let probe_ready = equirect.as_ref().is_none_or(|h| world.resource::<Assets<Image>>().get(h).is_some());
            let players_ready = shot.clips == 0 || !players_below(world, root).is_empty();
            if spawned && probe_ready && players_ready {
                loaded = true;
                break;
            }
        }
        if !loaded {
            return Err("glTF scene never finished loading".into());
        }
        // Bounds and global transforms of the spawned scene.
        self.step();
        let world = self.apps.main.world_mut();
        let below = descendants(world, root);
        let mut source = None;
        for &entity in &below {
            if world.get::<Camera3d>(entity).is_some() {
                if let Some(mut camera) = world.get_mut::<Camera>(entity) {
                    camera.is_active = false;
                }
                source = source.or(Some(entity));
            }
            if let Some(mut light) = world.get_mut::<DirectionalLight>(entity) {
                light.shadow_maps_enabled = true;
            }
            if let Some(mut light) = world.get_mut::<SpotLight>(entity) {
                light.shadow_maps_enabled = true;
            }
            if let Some(mut light) = world.get_mut::<PointLight>(entity) {
                light.shadow_maps_enabled = true;
            }
        }
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for &entity in &below {
            let (Some(aabb), Some(global)) = (world.get::<Aabb>(entity), world.get::<GlobalTransform>(entity)) else {
                continue;
            };
            let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
            for i in 0..8 {
                let corner = c + h * Vec3::new(sign(i & 1), sign(i & 2), sign(i & 4));
                let p = global.transform_point(corner);
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        let (center, radius) = if lo.x <= hi.x {
            ((lo + hi) / 2.0, ((hi - lo).length() / 2.0).max(1e-3))
        } else {
            (Vec3::ZERO, 1.0)
        };
        let morphs = below.iter().any(|&e| world.get::<MeshMorphWeights>(e).is_some());
        let players = players_below(world, root);
        for &entity in &players {
            let mut e = world.entity_mut(entity);
            e.insert(AnimationGraphHandle(graph.clone()));
            let mut player = e.get_mut::<AnimationPlayer>().expect("player");
            for node in &nodes {
                player.play(*node).pause();
            }
        }
        let cube = match &equirect {
            Some(h) => {
                let images = world.resource::<Assets<Image>>();
                equirect_to_cube(images.get(h).expect("probe loaded"), 512)?
            }
            None => studio_probe(64),
        };
        let probe = world.resource_mut::<Assets<Image>>().add(cube);
        let camera = world
            .spawn((
                DirectorCamera,
                Camera3d::default(),
                Camera {
                    is_active: false,
                    ..default()
                },
                Hdr,
                Tonemapping::TonyMcMapface,
                Bloom::NATURAL,
                Msaa::Off,
                Smaa::default(),
                bevy::pbr::ScreenSpaceTransmission::default(),
                Projection::default(),
                Transform::IDENTITY,
            ))
            .id();
        self.stages.insert(
            key,
            Stage {
                root,
                camera,
                source,
                players,
                nodes,
                center,
                radius,
                probe,
                warm: HashSet::new(),
                morphs,
            },
        );
        Ok(key)
    }

    fn target(&mut self, (width, height): (u32, u32)) {
        if self.targets.contains_key(&(width, height)) {
            return;
        }
        let world = self.apps.main.world_mut();
        let mut image = Image::new_target_texture(width, height, TextureFormat::Rgba8UnormSrgb, None);
        // Bevy renders through the sRGB view. The allowed UNORM view lets
        // Skia sample the encoded values the way its sRGB raster images hold
        // them.
        image.texture_descriptor.view_formats = &[TextureFormat::Rgba8Unorm];
        image.texture_descriptor.usage |= wgpu::TextureUsages::COPY_SRC;
        image.asset_usage = RenderAssetUsages::RENDER_WORLD;
        let image = world.resource_mut::<Assets<Image>>().add(image);
        let row = (width * 4).div_ceil(256) * 256;
        let buffer = world.resource::<RenderDevice>().wgpu_device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("director-3d readback"),
            size: row as u64 * height as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.targets.insert(
            (width, height),
            Target {
                image,
                buffer,
                row,
                holds: None,
            },
        );
    }

    /// Moves the target to shader-read layout and returns its Vulkan image.
    fn texture(&self, (width, height): (u32, u32)) -> Result<Texture, String> {
        let target = &self.targets[&(width, height)];
        let render = self
            .apps
            .sub_apps
            .get(&RenderApp.intern())
            .ok_or("no render app")?
            .world();
        let gpu = render
            .resource::<RenderAssets<GpuImage>>()
            .get(&target.image)
            .ok_or("render target is not on the GPU")?;
        let device = render.resource::<RenderDevice>().wgpu_device();
        let queue = render.resource::<RenderQueue>();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.transition_resources(
            std::iter::empty::<wgpu::BufferTransition<&wgpu::Buffer>>(),
            std::iter::once(wgpu::TextureTransition {
                texture: &*gpu.texture,
                selector: None,
                state: wgpu::TextureUses::RESOURCE,
            }),
        );
        queue.submit([encoder.finish()]);
        device
            .poll(PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| format!("texture poll: {e}"))?;
        // SAFETY: the target keeps this Vulkan texture alive.
        let image = unsafe { gpu.texture.as_hal::<wgpu::hal::api::Vulkan>() }
            .map(|t| unsafe { t.raw_handle() }.as_raw())
            .ok_or("render target is not a Vulkan image")?;
        Ok(Texture { image, width, height })
    }

    /// Copies the target into its buffer and unpads the rows.
    fn read(&self, (width, height): (u32, u32)) -> Result<Frame, String> {
        let target = &self.targets[&(width, height)];
        let render = self
            .apps
            .sub_apps
            .get(&RenderApp.intern())
            .ok_or("no render app")?
            .world();
        let gpu = render
            .resource::<RenderAssets<GpuImage>>()
            .get(&target.image)
            .ok_or("render target is not on the GPU")?;
        let device = render.resource::<RenderDevice>().wgpu_device();
        let queue = render.resource::<RenderQueue>();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &target.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(target.row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let slice = target.buffer.slice(..);
        let (mapped, done) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = mapped.send(r);
        });
        device
            .poll(PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| format!("readback poll: {e}"))?;
        done.recv()
            .map_err(|_| "readback never mapped".to_string())?
            .map_err(|e| format!("readback map: {e}"))?;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        {
            let view = slice.get_mapped_range().map_err(|e| format!("readback range: {e:?}"))?;
            for y in 0..height as usize {
                let at = y * target.row as usize;
                rgba.extend_from_slice(&view[at..at + width as usize * 4]);
            }
        }
        target.buffer.unmap();
        Ok(Frame { rgba })
    }
}

fn halton(mut i: u32, base: u32) -> f32 {
    let (mut f, mut r) = (1.0, 0.0);
    while i > 0 {
        f /= base as f32;
        r += f * (i % base) as f32;
        i /= base;
    }
    r
}

/// Sub-pixel offset of sample `i`: the Halton (2, 3) sequence about the
/// pixel centre, so any sample count covers the pixel evenly.
fn jitter(i: u32) -> Vec2 {
    Vec2::new(halton(i + 1, 2), halton(i + 1, 3)) - 0.5
}

/// Lens point of sample `i` on the unit disk: Halton (5, 7) through
/// Shirley's concentric map, which keeps the points evenly spread.
fn lens(i: u32) -> Vec2 {
    let (a, b) = (2.0 * halton(i + 1, 5) - 1.0, 2.0 * halton(i + 1, 7) - 1.0);
    if a == 0.0 && b == 0.0 {
        return Vec2::ZERO;
    }
    let quarter = std::f32::consts::FRAC_PI_4;
    let (r, theta) = if a.abs() > b.abs() { (a, quarter * (b / a)) } else { (b, 2.0 * quarter - quarter * (a / b)) };
    Vec2::new(r * theta.cos(), r * theta.sin())
}

fn stage_root(stages: &HashMap<StageKey, Stage>, key: StageKey) -> Entity {
    stages[&key].root
}

fn sign(bit: usize) -> f32 {
    if bit == 0 {
        -1.0
    } else {
        1.0
    }
}

fn descendants(world: &World, root: Entity) -> Vec<Entity> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if let Some(children) = world.get::<Children>(entity) {
            for child in children.iter().rev() {
                out.push(child);
                stack.push(child);
            }
        }
    }
    out
}

/// Marks every morph-target mesh changed. Bevy drops the mesh instance of a
/// hidden stage, and when the stage shows again it prepares morph descriptors
/// before it collects that instance, so the instance records no descriptor.
/// The descriptor exists from the next frame, and only a changed instance is
/// collected again.
fn touch_morph_meshes(world: &mut World) {
    let meshes: Vec<Entity> = world
        .query_filtered::<Entity, With<MeshMorphWeights>>()
        .iter(world)
        .collect();
    for entity in meshes {
        if let Some(mut mesh) = world.get_mut::<Mesh3d>(entity) {
            mesh.set_changed();
        }
    }
}

fn players_below(world: &World, root: Entity) -> Vec<Entity> {
    descendants(world, root)
        .into_iter()
        .filter(|&e| world.get::<AnimationPlayer>(e).is_some())
        .collect()
}

/// Cube face directions in Bevy's +X, -X, +Y, -Y, +Z, -Z order.
fn face_direction(face: usize, s: f32, t: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1.0, -t, -s),
        1 => Vec3::new(-1.0, -t, s),
        2 => Vec3::new(s, 1.0, t),
        3 => Vec3::new(s, -1.0, -t),
        4 => Vec3::new(s, -t, 1.0),
        _ => Vec3::new(-s, -t, -1.0),
    }
    .normalize()
}

fn cube(size: u32, radiance: impl Fn(Vec3) -> [f32; 3]) -> Image {
    let mut out = Vec::with_capacity((size * size * 6 * 8) as usize);
    for face in 0..6 {
        for y in 0..size {
            for x in 0..size {
                let s = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let t = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let [r, g, b] = radiance(face_direction(face, s, t));
                for ch in [r, g, b, 1.0] {
                    out.extend_from_slice(&half::f16::from_f32(ch).to_le_bytes());
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        out,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

/// The neutral studio probe: a grey ground, a brighter zenith and one soft key
/// above and to the right of the default camera.
fn studio_probe(size: u32) -> Image {
    let key = Vec3::new(0.5, 0.7, 0.5).normalize();
    cube(size, |d| {
        let sky = if d.y >= 0.0 { 0.6 + 0.4 * d.y } else { 0.6 - 0.4 * (-d.y).sqrt() };
        let v = sky + 6.0 * d.dot(key).max(0.0).powf(32.0);
        [v, v, v]
    })
}

/// Samples an equirectangular float image into an Rgba16Float cube.
fn equirect_to_cube(src: &Image, size: u32) -> Result<Image, String> {
    let (sw, sh) = (src.width() as usize, src.height() as usize);
    let data = src.data.as_ref().ok_or("environment has no CPU pixels")?;
    let format = src.texture_descriptor.format;
    let texel: Arc<dyn Fn(usize, usize) -> [f32; 3]> = match format {
        TextureFormat::Rgba32Float => {
            let data = data.clone();
            Arc::new(move |x, y| {
                let o = (y * sw + x) * 16;
                let f = |k: usize| f32::from_le_bytes(data[o + k * 4..o + k * 4 + 4].try_into().unwrap());
                [f(0), f(1), f(2)]
            })
        }
        TextureFormat::Rgba16Float => {
            let data = data.clone();
            Arc::new(move |x, y| {
                let o = (y * sw + x) * 8;
                let f = |k: usize| half::f16::from_le_bytes([data[o + k * 2], data[o + k * 2 + 1]]).to_f32();
                [f(0), f(1), f(2)]
            })
        }
        other => return Err(format!("environment format {other:?} (a float .hdr or .exr is supported)")),
    };
    Ok(cube(size, |d| {
        let u = 0.5 + d.x.atan2(-d.z) / std::f32::consts::TAU;
        let v = d.y.clamp(-1.0, 1.0).acos() / std::f32::consts::PI;
        let fx = (u * sw as f32 - 0.5).rem_euclid(sw as f32);
        let fy = (v * sh as f32 - 0.5).clamp(0.0, (sh - 1) as f32);
        let (x0, y0) = (fx as usize % sw, fy as usize);
        let (x1, y1) = ((x0 + 1) % sw, (y0 + 1).min(sh - 1));
        let (ax, ay) = (fx.fract(), fy.fract());
        let (c00, c10, c01, c11) = (texel(x0, y0), texel(x1, y0), texel(x0, y1), texel(x1, y1));
        let mut c = [0.0f32; 3];
        for k in 0..3 {
            let top = c00[k] + (c10[k] - c00[k]) * ax;
            let bottom = c01[k] + (c11[k] - c01[k]) * ax;
            c[k] = top + (bottom - top) * ay;
        }
        c
    }))
}
