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
    camera::{
        primitives::Aabb, visibility::RenderLayers, CameraMainTextureUsages, Exposure, Hdr, ImageRenderTarget, RenderTarget, SubCameraView,
    },
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    gltf::Gltf,
    image::Image,
    light::{EnvironmentMapLight, GeneratedEnvironmentMapLight, Skybox},
    log::LogPlugin,
    mesh::morph::MeshMorphWeights,
    post_process::bloom::Bloom,
    prelude::*,
    render::{
        render_asset::RenderAssets,
        render_resource::{
            Extent3d, PollType, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
            TextureViewDimension,
        },
        renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue},
        settings::{RenderCreation, RenderResources},
        texture::GpuImage,
        RenderApp, RenderPlugin,
    },
    solari::scene::{RaytracingMesh3d, RaytracingScenePlugin},
    transform::TransformSystems,
    window::ExitCondition,
    world_serialization::WorldAssetRoot,
};
use bevy_gaussian_splatting::{
    CloudSettings, GaussianCamera, GaussianSplattingPlugin, PlanarGaussian3d, PlanarGaussian3dHandle,
};
use director_scene::{ModelLight, ModelLightKind, ModelView};

use crate::accumulate::Accumulator;
use crate::catcher::ShadowCatcher;
use crate::hero::{self, HeroPass, Traceable, Traced};
use crate::lights;
use crate::node::{catchers_hash, lights_hash, splats_hash, Splat};
use crate::text::{texts_hash, Text3d};
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
    /// Path traced paths per pixel in each sample. 0 rasterizes the frame.
    pub paths: u32,
    /// Filter each traced sample's noise, guided by its first surfaces.
    pub denoise: bool,
    /// Lights added to the file's own.
    pub lights: Arc<Vec<ModelLight>>,
    /// Gaussian splat clouds beside the file's meshes.
    pub splats: Arc<Vec<Splat>>,
    /// glTF node names whose meshes show only the shadows cast on them.
    pub catchers: Arc<Vec<String>>,
    /// The catchers' reflectance facing the camera, 0 to 1. 0 mirrors
    /// nothing.
    pub reflection: f32,
    /// Lines of extruded type beside the file's meshes.
    pub texts: Arc<Vec<Text3d>>,
    /// A camera that films the shot in place of the file's, such as one
    /// solved from footage.
    pub camera: Option<ShotCamera>,
    /// The cameras at the open and close of a shutter centred on `camera`:
    /// the samples sweep from one through `camera` to the other, so the
    /// model blurs as the footage does.
    pub shutter: Option<[ShotCamera; 2]>,
}

/// A camera pose in the model's world, looking down -Z with +Y up, and its
/// vertical field of view in radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotCamera {
    pub position: Vec3,
    pub rotation: Quat,
    pub fov: f32,
}

/// The bits of a shot camera for frame cache keys, all zero without one.
pub(crate) fn camera_bits(camera: Option<ShotCamera>) -> [u32; 8] {
    camera.map_or([0; 8], |c| {
        let (p, q) = (c.position, c.rotation);
        [p.x, p.y, p.z, q.x, q.y, q.z, q.w, c.fov].map(f32::to_bits)
    })
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

/// Marks the camera that films a stage reflected in its shadow catchers.
#[derive(Component)]
struct MirrorCamera;

/// The render layer of shadow catcher meshes. The Director camera sees it
/// beside the default layer, and the mirror camera leaves it out, so a
/// catcher never reflects itself.
const CATCHER_LAYER: usize = 1;

/// The camera placement the orbit system writes before transforms propagate.
#[derive(Resource, Default)]
struct Orbit(Option<Placement>);

#[derive(Clone, Copy)]
struct Placement {
    camera: Entity,
    /// The file's camera, whose animated pose and lens the shot starts from.
    source: Option<Entity>,
    /// The shot's own camera, which stands in for the file's.
    tracked: Option<ShotCamera>,
    center: Vec3,
    radius: f32,
    view: ModelView,
    aspect: f32,
    /// Target size in pixels.
    size: UVec2,
    /// Whether the shot averages several renders.
    sampled: bool,
    /// This sample's sub-pixel offset in pixels.
    jitter: Vec2,
    /// This sample's point on the unit lens disk.
    lens: Vec2,
    /// The mirror camera and the catcher plane (unit normal n and offset d
    /// with n . x = d) it reflects the Director camera in.
    mirror: Option<(Entity, Vec4)>,
}

/// Scene nodes and lenses, apart from the two cameras `orbit` moves.
type NotCamera = (Without<DirectorCamera>, Without<MirrorCamera>);
/// The pose, lens and camera `orbit` writes.
type CameraParts = (&'static mut Transform, &'static mut Projection, &'static mut Camera);

/// Framing lens for a file without a camera: 30 degrees vertical.
const FRAMING_FOV: f32 = std::f32::consts::PI / 6.0;

/// Places the director camera from the file's camera or the framing camera,
/// then orbits it about the bounding sphere centre. Runs after animation, so
/// the file camera's pose is this frame's.
fn orbit(
    orbit: Res<Orbit>,
    nodes: Query<(&Transform, Option<&ChildOf>), NotCamera>,
    lenses: Query<&Projection, NotCamera>,
    mut cameras: Query<CameraParts, (With<DirectorCamera>, Without<MirrorCamera>)>,
    mut mirrors: Query<CameraParts, With<MirrorCamera>>,
) {
    let Some(p) = orbit.0 else { return };
    let Ok((mut transform, mut projection, mut camera)) = cameras.get_mut(p.camera) else { return };
    let (base, mut lens) = match (p.tracked, p.source) {
        (Some(c), _) => {
            let lens = Projection::Perspective(PerspectiveProjection { fov: c.fov, near: 0.05, far: 2000.0, ..default() });
            (Transform::from_translation(c.position).with_rotation(c.rotation), lens)
        }
        (None, Some(source)) => {
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
        (None, None) => {
            let half_v = FRAMING_FOV / 2.0;
            let half_h = (half_v.tan() * p.aspect).atan();
            // The near and far planes follow the orbit distance, so a closer
            // camera keeps the whole sphere in front of its near plane.
            let distance = p.radius / half_v.min(half_h).sin() * 1.05;
            let reach = distance * p.view.distance.max(1e-3);
            let lens = Projection::Perspective(PerspectiveProjection {
                fov: FRAMING_FOV,
                near: (reach - p.radius * 2.0).max(p.radius * 0.01).max(1e-4),
                far: reach + p.radius * 8.0,
                ..default()
            });
            (Transform::from_translation(p.center + Vec3::Z * distance), lens)
        }
    };
    let yaw = Quat::from_rotation_y(p.view.yaw.to_radians());
    let right = yaw * (base.rotation * Vec3::X);
    let pitch = Quat::from_axis_angle(right, -p.view.pitch.to_radians());
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
        if p.sampled {
            let focal = 0.5 * SENSOR_HEIGHT / half;
            let eye = p.lens * focal / (2.0 * p.view.aperture);
            let step = transform.rotation * eye.extend(0.0);
            transform.translation += step;
            // A sub view offset moves the image by -2 * offset / size in NDC
            // on x and +2 * offset / size on y.
            shift = Vec2::new(-eye.x, eye.y) * p.size.y as f32 / (2.0 * half * focus);
        }
    }
    // The sample offset goes into the projection itself as a sub view, so
    // meshes and splats (whose shader reads the unjittered projection) both
    // move with it.
    let sub = p.sampled.then(|| SubCameraView {
        full_size: p.size,
        offset: p.jitter + shift,
        size: p.size,
    });
    if camera.sub_camera_view != sub {
        camera.sub_camera_view = sub;
    }
    // The mirror camera is the Director camera reflected in the catcher
    // plane and flipped along its own X, which keeps it a rotation and its
    // triangles' winding. Its image is the reflection mirrored left to
    // right, so its sample offset mirrors too.
    if let Some((mirror, plane)) = p.mirror {
        if let Ok((mut m_transform, mut m_projection, mut m_camera)) = mirrors.get_mut(mirror) {
            let n = plane.truncate();
            let reflect = Mat4::from_cols(
                (Vec3::X - 2.0 * n.x * n).extend(0.0),
                (Vec3::Y - 2.0 * n.y * n).extend(0.0),
                (Vec3::Z - 2.0 * n.z * n).extend(0.0),
                (2.0 * plane.w * n).extend(1.0),
            );
            let world = reflect * transform.to_matrix() * Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0));
            *m_transform = Transform::from_matrix(world);
            let sub = sub.map(|s| SubCameraView {
                offset: Vec2::new(-s.offset.x, s.offset.y),
                ..s
            });
            if m_camera.sub_camera_view != sub {
                m_camera.sub_camera_view = sub;
            }
            *m_projection = lens.clone();
        }
    }
    *projection = lens;
}

/// The camera at `s` of the way through a shutter that opens at `shutter[0]`,
/// passes `middle` halfway and closes at `shutter[1]`.
fn sweep(middle: ShotCamera, shutter: [ShotCamera; 2], s: f32) -> ShotCamera {
    let (a, b, t) = if s < 0.5 { (shutter[0], middle, s * 2.0) } else { (middle, shutter[1], s * 2.0 - 1.0) };
    ShotCamera {
        position: a.position.lerp(b.position, t),
        rotation: a.rotation.slerp(b.rotation, t),
        fov: a.fov + (b.fov - a.fov) * t,
    }
}

/// Bevy's default sensor: Super 35, 18.66 mm tall.
const SENSOR_HEIGHT: f32 = 0.01866;

/// The fewest renders a lens with an aperture or a sized light averages.
const SOFT_SAMPLES: u32 = 16;

struct Stage {
    /// The loaded glTF. Stages of one file share its scene asset, and
    /// dropping this handle would unload the file, so the next stage's load
    /// would replace the scene and respawn every stage's instance of it.
    _gltf: Handle<Gltf>,
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
    /// The added lights and the hash of the set they came from.
    lights: Vec<lights::Placed>,
    lit: u64,
    /// Each mesh entity and its copy in Solari's vertex layout, built on the
    /// stage's first Hero shot.
    traced: Option<Vec<hero::TracedMesh>>,
    /// The copies are in the ray traced scene.
    tracing: bool,
    /// The ray traced emitters of the rect lights, and the hash of the light
    /// set they came from.
    emitters: Vec<Entity>,
    emitted: Option<u64>,
    /// Mesh entities with blended materials. The path tracer traces them
    /// itself, so Hero shots hide them from the raster's transparent pass,
    /// which draws over the traced image.
    blended: Vec<Entity>,
    /// The shadow catchers' material and mirror camera.
    mirror: Option<Mirror>,
}

/// A stage's shadow catchers and the camera that films their reflection.
struct Mirror {
    material: Handle<ShadowCatcher>,
    /// The catchers' common plane (unit normal n and offset d with
    /// n . x = d), or `None` when they do not lie in one.
    plane: Option<Vec4>,
    camera: Entity,
    /// The mirror image of each target size.
    images: HashMap<(u32, u32), Handle<Image>>,
}

struct Target {
    image: Handle<Image>,
    buffer: wgpu::Buffer,
    row: u32,
    /// The shot whose frame the target holds.
    holds: Option<ShotKey>,
}

/// (model hash, environment hash, splat set hash, shadow catcher hash).
type StageKey = (u64, Option<u64>, u64, u64, u64);
type ShotKey = (StageKey, usize, u64, u64, [u32; 12], [u32; 8], [[u32; 8]; 2]);

fn shot_key(stage: StageKey, shot: &Shot) -> ShotKey {
    (
        stage,
        shot.clips,
        shot.time.to_bits(),
        lights_hash(&shot.lights),
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
            shot.paths,
            shot.denoise as u32,
            shot.reflection.to_bits(),
        ],
        camera_bits(shot.camera),
        shot.shutter.map_or([[0; 8]; 2], |s| s.map(|c| camera_bits(Some(c)))),
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
    /// The Hero pass has dispatched once, so its pipeline exists.
    traced_once: bool,
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
                // The splat crate turns on Bevy's windowing; the studio has no window.
                .disable::<bevy::winit::WinitPlugin>()
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
        app.add_plugins((GaussianSplattingPlugin, RaytracingScenePlugin, hero::HeroPlugin, crate::display::DisplayPlugin, crate::catcher::CatcherPlugin));
        app.init_resource::<Orbit>();
        app.add_systems(PostUpdate, orbit.after(AnimationSystems).before(TransformSystems::Propagate).before(bevy::camera::CameraUpdateSystems));
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
            traced_once: false,
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
        // A model made only of splats has nothing to trace and rasterizes.
        let (hero, mut retraced) = if shot.paths > 0 {
            self.check_hero()?;
            match self.trace_only(key, !shot.splats.is_empty())? {
                Some(changed) => (true, changed),
                None => (false, false),
            }
        } else {
            (false, false)
        };
        {
            let stage = &self.stages[&key];
            let world = self.apps.main.world_mut();
            let shown = if hero { Visibility::Hidden } else { Visibility::Inherited };
            for &e in &stage.blended {
                world.entity_mut(e).insert(shown);
            }
        }
        let target = self.targets[&size].image.clone();
        let stage = &self.stages[&key];
        let (camera, source, center, radius, probe) = (stage.camera, stage.source, stage.center, stage.radius, stage.probe.clone());
        let players = stage.players.clone();
        let nodes = stage.nodes.clone();
        let warm = stage.warm.contains(&size);
        let morphs = stage.morphs;
        let others: Vec<(Entity, Entity, Option<Entity>)> = self
            .stages
            .iter()
            .filter(|(k, _)| **k != key)
            .map(|(_, s)| (s.root, s.camera, s.mirror.as_ref().map(|m| m.camera)))
            .collect();
        let world = self.apps.main.world_mut();
        for (root, other, mirror) in others {
            world.entity_mut(root).insert(Visibility::Hidden);
            for camera in std::iter::once(other).chain(mirror) {
                if let Some(mut c) = world.get_mut::<Camera>(camera) {
                    c.is_active = false;
                }
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
            // Bloom is a lens effect of the whole frame. Over a transparent
            // sky the frame is a layer that lands on a page or footage, and
            // its glow would haze them as added light, so only a shot that
            // paints its own background blooms.
            if shot.background {
                e.insert((
                    Skybox {
                        image: Some(probe.clone()),
                        brightness: shot.environment_intensity,
                        ..default()
                    },
                    Bloom::NATURAL,
                ));
            } else {
                e.remove::<(Skybox, Bloom)>();
            }
            // Bevy copies the probe intensity into the generated
            // `EnvironmentMapLight` once, and the view reads it from there.
            if let Some(mut light) = e.get_mut::<EnvironmentMapLight>() {
                light.intensity = shot.environment_intensity;
            }
            // Warm-up frames trace one path; the samples trace them all.
            if hero {
                e.insert(HeroPass {
                    sample: 0,
                    paths: 1,
                    denoise: false,
                    background: shot.background,
                    lamps: Default::default(),
                });
            } else {
                e.remove::<HeroPass>();
            }
            let mut c = e.get_mut::<Camera>().expect("director camera");
            c.is_active = true;
            c.clear_color = ClearColorConfig::Custom(Color::NONE);
        }
        let (mirror, mirror_steps) = film_mirror(self.stages.get_mut(&key).expect("stage"), world, shot, size, &probe)?;
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
            tracked: shot.camera,
            center,
            radius,
            view: shot.view,
            aspect: shot.width as f32 / shot.height as f32,
            size: UVec2::new(shot.width, shot.height),
            sampled: false,
            jitter: Vec2::ZERO,
            lens: Vec2::ZERO,
            mirror,
        });
        // A lens or a sized light averages at least `SOFT_SAMPLES` renders,
        // at either level, so preview and export blur alike.
        // A Hero shot samples point, spot and rect lights over their emitters,
        // so only a sized sun moves between its samples.
        let soft = if hero {
            shot.lights.iter().any(|l| l.kind == ModelLightKind::Sun && l.angle > 0.0)
        } else {
            lights::sized(&shot.lights)
        };
        let samples = if shot.view.aperture > 0.0 || soft || shot.shutter.is_some() {
            shot.samples.max(SOFT_SAMPLES)
        } else {
            shot.samples.max(1)
        };
        let lit = lights_hash(&shot.lights);
        let stage = self.stages.get_mut(&key).expect("stage");
        let world = self.apps.main.world_mut();
        let respawned = stage.lit != lit;
        if respawned {
            lights::despawn(world, std::mem::take(&mut stage.lights));
            stage.lights = lights::spawn(world, stage.root, &shot.lights, center, radius);
            stage.lit = lit;
        }
        lights::place(world, &stage.lights, None);
        if hero && stage.emitted != Some(lit) {
            for e in std::mem::take(&mut stage.emitters) {
                world.despawn(e);
            }
            stage.emitters = lights::emitters(world, stage.root, &shot.lights, center, radius);
            // Solari binds no scene without a light source, and the probe
            // and the Hero lamps are not Solari lights, so a stage lit by
            // them alone gets a dark sun.
            if !solari_lit(world, stage.root) {
                let sun = DirectionalLight { illuminance: 0.0, shadow_maps_enabled: false, ..default() };
                stage.emitters.push(world.spawn((sun, Transform::default(), ChildOf(stage.root))).id());
            }
            stage.emitted = Some(lit);
            retraced = true;
        }
        if let Some(p) = &mut world.resource_mut::<Orbit>().0 {
            p.sampled = samples > 1;
        }
        // A changed catcher material binds a frame late, and a new mirror
        // image needs its own warm-up like a new target.
        if !warm || mirror_steps == 3 {
            for _ in 0..3 {
                self.step();
            }
            self.stages.get_mut(&key).expect("stage").warm.insert(size);
        } else {
            for _ in 0..mirror_steps {
                self.step();
            }
        }
        // A stage that just came back into view gets its morph descriptors
        // (see `touch_morph_meshes`) and its lights' shadow casters late, and
        // so does a new light set, so either takes two extra frames.
        if warm && (self.shown != Some(key) || respawned) {
            self.step();
            self.step();
        }
        self.shown = Some(key);
        if morphs {
            touch_morph_meshes(self.apps.main.world_mut());
        }
        // A new ray traced scene builds its acceleration structures, and the
        // first Hero shot its pipeline, before the samples count.
        let traced = self.apps.main.world().resource::<Traced>().clone();
        if hero && (retraced || !self.traced_once) {
            let start = traced.count();
            let mut steps = 0;
            while steps < 3 || traced.count() == start {
                if steps == 64 {
                    return Err("the Hero path tracer never ran".into());
                }
                self.step();
                steps += 1;
            }
            self.traced_once = true;
        }
        // Skins and morph targets pose on the CPU from this frame's joints
        // and weights, then their acceleration structures rebuild.
        let posed: Vec<_> = self.stages[&key].traced.iter().flatten().filter(|t| t.2.is_some()).cloned().collect();
        if hero && !posed.is_empty() {
            self.step();
            hero::deform(self.apps.main.world_mut(), &posed);
            self.step();
        }
        let start = traced.count();
        for sample in 0..samples {
            if hero {
                let world = self.apps.main.world_mut();
                let mut pass = world.get_mut::<HeroPass>(camera).expect("hero pass");
                pass.sample = sample;
                pass.paths = shot.paths;
                pass.denoise = shot.denoise;
            }
            if samples > 1 {
                let mut orbit = self.apps.main.world_mut().resource_mut::<Orbit>();
                let p = orbit.0.as_mut().expect("placement");
                // Each path jitters within its pixel, so Hero samples keep
                // the pixel grid and vary the lens and lights.
                p.jitter = if hero { Vec2::ZERO } else { jitter(sample) };
                p.lens = lens(sample);
                if let (Some(middle), Some(shutter)) = (shot.camera, shot.shutter) {
                    p.tracked = Some(sweep(middle, shutter, (sample as f32 + 0.5) / samples as f32));
                }
                if soft {
                    let square = Vec2::new(halton(sample + 1, 11), halton(sample + 1, 13));
                    let stage = &self.stages[&key];
                    let world = self.apps.main.world_mut();
                    lights::place(world, &stage.lights, Some((square, disk(square * 2.0 - 1.0))));
                }
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
        if hero && traced.count().wrapping_sub(start) != samples {
            return Err(format!(
                "the Hero path tracer traced {} of {samples} samples",
                traced.count().wrapping_sub(start)
            ));
        }
        if samples > 1 {
            let (device, queue, texture) = self.gpu_target(size)?;
            self.accumulator.as_ref().expect("accumulator").resolve(&device, &queue, &texture);
        }
        Ok(())
    }

    /// Refuses a Hero shot on a GPU that cannot trace rays.
    fn check_hero(&self) -> Result<(), String> {
        if !self.apps.main.world().resource::<Traceable>().0 {
            return Err("Hero path tracing needs a GPU with hardware ray tracing".into());
        }
        Ok(())
    }

    /// Puts the mesh copies of stage `key` alone in the ray traced scene,
    /// building them on its first Hero shot. Returns whether the scene
    /// changed, or `None` for a stage of splats with no triangle meshes.
    fn trace_only(&mut self, key: StageKey, splats: bool) -> Result<Option<bool>, String> {
        let mut changed = false;
        let world = self.apps.main.world_mut();
        for (k, stage) in self.stages.iter_mut() {
            if *k == key || !stage.tracing {
                continue;
            }
            for (entity, ..) in stage.traced.iter().flatten() {
                world.entity_mut(*entity).remove::<RaytracingMesh3d>();
            }
            for e in std::mem::take(&mut stage.emitters) {
                world.despawn(e);
            }
            stage.emitted = None;
            stage.tracing = false;
            changed = true;
        }
        let stage = self.stages.get_mut(&key).expect("stage");
        if stage.tracing {
            return Ok(Some(changed));
        }
        if stage.traced.is_none() {
            let mut copies = Vec::new();
            for entity in descendants(world, stage.root) {
                let Some(mesh) = world.get::<Mesh3d>(entity).map(|m| m.0.clone()) else { continue };
                let Some(material) = world.get::<MeshMaterial3d<StandardMaterial>>(entity).map(|m| m.0.clone()) else {
                    continue;
                };
                let blends = world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&material)
                    .is_some_and(|m| !matches!(m.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)));
                if blends {
                    stage.blended.push(entity);
                }
                let meshes = world.resource::<Assets<Mesh>>();
                let Some(source) = meshes.get(&mesh) else { continue };
                let Some(copy) = hero::solari_copy(source) else { continue };
                let rest = hero::rest(source, &copy).map(Arc::new);
                copies.push((entity, world.resource_mut::<Assets<Mesh>>().add(copy), rest));
            }
            stage.traced = Some(copies);
        }
        if stage.traced.as_ref().is_some_and(Vec::is_empty) {
            return if splats {
                Ok(None)
            } else {
                Err("Hero path tracing needs a model with triangle meshes".into())
            };
        }
        for (entity, copy, _) in stage.traced.iter().flatten() {
            world.entity_mut(*entity).insert(RaytracingMesh3d(copy.clone()));
        }
        stage.tracing = true;
        Ok(Some(true))
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
        let key = (
            shot.model.hash,
            shot.environment.as_ref().map(|e| e.hash),
            splats_hash(&shot.splats),
            catchers_hash(&shot.catchers),
            texts_hash(&shot.texts),
        );
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
        // The splat crate's scene loader also claims `.glb`, and a labelled
        // path picks its loader by extension, so the root loads by type and
        // the scene and clips come from the loaded `Gltf`.
        let gltf: Handle<Gltf> = server.load(format!("director://{model}"));
        let mut parsed = None;
        for _ in 0..4000 {
            self.step();
            let world = self.apps.main.world_mut();
            if let Some(bevy::asset::LoadState::Failed(e)) = world.resource::<AssetServer>().get_load_state(&gltf) {
                return Err(format!("glTF load failed: {e}"));
            }
            if let Some(g) = world.resource::<Assets<Gltf>>().get(&gltf) {
                parsed = Some((g.scenes.first().cloned(), g.animations.clone()));
                break;
            }
        }
        let Some((Some(scene), animations)) = parsed else {
            return Err("glTF scene never finished loading".into());
        };
        let world = self.apps.main.world_mut();
        let server = world.resource::<AssetServer>().clone();
        let clips: Vec<Handle<AnimationClip>> = animations.into_iter().take(shot.clips).collect();
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
        let caught = match catch_shadows(self.apps.main.world_mut(), root, &shot.catchers) {
            Ok(caught) => caught,
            Err(e) => {
                self.apps.main.world_mut().despawn(root);
                return Err(e);
            }
        };
        // Splat clouds join the root once the scene has spawned beside them.
        let mut clouds = Vec::new();
        for splat in shot.splats.iter() {
            let name = format!("{:016x}.ply", splat.cloud.hash);
            self.dir.insert_asset(Path::new(&name), splat.cloud.bytes.to_vec());
            let world = self.apps.main.world_mut();
            let handle: Handle<PlanarGaussian3d> = world.resource::<AssetServer>().load(format!("director://{name}"));
            let [x, y, z] = splat.rotation.map(f32::to_radians);
            let transform = Transform {
                translation: Vec3::from(splat.position),
                rotation: Quat::from_euler(EulerRot::ZYX, z, y, x),
                scale: Vec3::splat(splat.scale),
            };
            let entity = world
                .spawn((
                    PlanarGaussian3dHandle(handle.clone()),
                    CloudSettings {
                        global_opacity: splat.opacity,
                        ..default()
                    },
                    // The studio sets the cloud's `Aabb` itself once it loads.
                    bevy::camera::visibility::NoFrustumCulling,
                    transform,
                    Visibility::default(),
                    ChildOf(root),
                ))
                .id();
            clouds.push((entity, handle));
        }
        let mut ready = clouds.is_empty();
        for _ in 0..4000 {
            if ready {
                break;
            }
            self.step();
            let world = self.apps.main.world_mut();
            let server = world.resource::<AssetServer>();
            for (_, h) in &clouds {
                if let Some(bevy::asset::LoadState::Failed(e)) = server.get_load_state(h) {
                    return Err(format!("splat load failed: {e}"));
                }
            }
            let assets = world.resource::<Assets<PlanarGaussian3d>>();
            ready = clouds.iter().all(|(_, h)| assets.get(h).is_some());
        }
        if !ready {
            return Err("splat cloud never finished loading".into());
        }
        // The splat draw queue needs an `Aabb` for its depth order, and the
        // crate's own one spans every padding gaussian, so each cloud gets
        // the box of its visible gaussians.
        for (entity, handle) in &clouds {
            let world = self.apps.main.world_mut();
            let cloud = world.resource::<Assets<PlanarGaussian3d>>().get(handle).expect("cloud loaded");
            let (l, h) = cloud_bounds(cloud, &GlobalTransform::IDENTITY);
            world.entity_mut(*entity).insert(Aabb::from_min_max(l, h));
        }
        if let Err(e) = spawn_texts(self.apps.main.world_mut(), root, &shot.texts) {
            self.apps.main.world_mut().despawn(root);
            return Err(e);
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
            if clouds.iter().any(|(cloud, _)| *cloud == entity) {
                continue;
            }
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
        for (entity, handle) in &clouds {
            let global = world.get::<GlobalTransform>(*entity).copied().unwrap_or_default();
            let cloud = world.resource::<Assets<PlanarGaussian3d>>().get(handle).expect("cloud loaded");
            let (l, h) = cloud_bounds(cloud, &global);
            lo = lo.min(l);
            hi = hi.max(h);
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
                // The Hero pass writes the main texture from a compute shader.
                CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
                Msaa::Off,
                Smaa::default(),
                bevy::pbr::ScreenSpaceTransmission::default(),
                GaussianCamera::default(),
                Projection::default(),
                Transform::IDENTITY,
            ))
            .id();
        // Splats skip the filmic curve and bloom in `display`, so every stage
        // takes the curve, and each shot with a background blooms.
        world
            .entity_mut(camera)
            .insert((Tonemapping::TonyMcMapface, RenderLayers::from_layers(&[0, CATCHER_LAYER])));
        // The mirror camera renders linear radiance for the catchers to
        // sample, before the Director camera, with no tone curve or dither.
        let mirror = caught.map(|(material, plane)| Mirror {
            material,
            plane,
            camera: world
                .spawn((
                    MirrorCamera,
                    Camera3d::default(),
                    Camera {
                        is_active: false,
                        order: -1,
                        clear_color: ClearColorConfig::Custom(Color::NONE),
                        ..default()
                    },
                    Hdr,
                    Msaa::Off,
                    Tonemapping::None,
                    DebandDither::Disabled,
                    Projection::default(),
                    Transform::IDENTITY,
                ))
                .id(),
            images: HashMap::new(),
        });
        self.stages.insert(
            key,
            Stage {
                _gltf: gltf,
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
                lights: Vec::new(),
                lit: lights_hash(&[]),
                traced: None,
                tracing: false,
                emitters: Vec::new(),
                emitted: None,
                blended: Vec::new(),
                mirror,
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
    disk(Vec2::new(halton(i + 1, 5), halton(i + 1, 7)) * 2.0 - 1.0)
}

/// Shirley's concentric map from the square [-1, 1]² to the unit disk.
fn disk(Vec2 { x: a, y: b }: Vec2) -> Vec2 {
    if a == 0.0 && b == 0.0 {
        return Vec2::ZERO;
    }
    let quarter = std::f32::consts::FRAC_PI_4;
    let (r, theta) = if a.abs() > b.abs() { (a, quarter * (b / a)) } else { (b, 2.0 * quarter - quarter * (a / b)) };
    Vec2::new(r * theta.cos(), r * theta.sin())
}

/// The box a cloud's gaussians fill in world space, from the 1st to the 99th
/// percentile of their centres on each axis so stray floaters do not widen
/// the framing, padded by three times the median gaussian size.
fn cloud_bounds(cloud: &PlanarGaussian3d, global: &GlobalTransform) -> (Vec3, Vec3) {
    let mut axes: [Vec<f32>; 3] = Default::default();
    let mut sizes = Vec::new();
    for (p, s) in cloud.position_visibility.iter().zip(&cloud.scale_opacity) {
        if s.opacity <= 0.0 {
            continue;
        }
        let world = global.transform_point(Vec3::from(p.position));
        for (axis, v) in axes.iter_mut().zip(world.to_array()) {
            axis.push(v);
        }
        sizes.push(s.scale.iter().copied().fold(0.0f32, f32::max));
    }
    if sizes.is_empty() {
        return (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    }
    let pick = |v: &mut Vec<f32>, q: f32| {
        v.sort_by(f32::total_cmp);
        v[((v.len() - 1) as f32 * q).round() as usize]
    };
    let pad = 3.0 * pick(&mut sizes, 0.5) * global.scale().max_element();
    let lo = Vec3::new(pick(&mut axes[0], 0.01), pick(&mut axes[1], 0.01), pick(&mut axes[2], 0.01));
    let hi = Vec3::new(pick(&mut axes[0], 0.99), pick(&mut axes[1], 0.99), pick(&mut axes[2], 0.99));
    (lo - pad, hi + pad)
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

/// Points the mirror camera of `stage` at this shot's target size and sets
/// the catchers' reflectance. Returns the mirror camera and plane when the
/// shot reflects, and the extra frames the change needs first: 3 for a new
/// mirror image, 1 for a changed catcher material, which binds a frame late.
fn film_mirror(
    stage: &mut Stage,
    world: &mut World,
    shot: &Shot,
    size: (u32, u32),
    probe: &Handle<Image>,
) -> Result<(Option<(Entity, Vec4)>, u32), String> {
    let Some(m) = stage.mirror.as_mut() else {
        return if shot.reflection > 0.0 {
            Err("reflection needs a shadow catcher to mirror the model".into())
        } else {
            Ok((None, 0))
        };
    };
    let plane = match (shot.reflection > 0.0, m.plane) {
        (false, _) => None,
        (true, Some(plane)) => Some(plane),
        (true, None) => return Err("reflection needs shadow catchers that lie in one plane".into()),
    };
    let fresh = plane.is_some() && !m.images.contains_key(&size);
    let image = plane.map(|_| {
        m.images
            .entry(size)
            .or_insert_with(|| {
                let mut image = Image::new_target_texture(size.0, size.1, TextureFormat::Rgba16Float, None);
                image.asset_usage = RenderAssetUsages::RENDER_WORLD;
                world.resource_mut::<Assets<Image>>().add(image)
            })
            .clone()
    });
    let strength = if plane.is_some() { shot.reflection } else { 0.0 };
    // A changed material rebuilds its bind group, so only a change writes it.
    let mut materials = world.resource_mut::<Assets<ShadowCatcher>>();
    let current = materials.get(&m.material).map(|c| (c.params.x, c.mirror.clone()));
    let changed = current != Some((strength, image.clone()));
    if changed {
        if let Some(mut c) = materials.get_mut(&m.material) {
            c.params.x = strength;
            c.mirror = image.clone();
        }
    }
    let mut e = world.entity_mut(m.camera);
    let Some(handle) = image else {
        e.get_mut::<Camera>().expect("mirror camera").is_active = false;
        return Ok((None, changed as u32));
    };
    e.insert((
        RenderTarget::Image(ImageRenderTarget {
            handle,
            scale_factor: 1.0,
        }),
        Exposure { ev100: shot.exposure },
        GeneratedEnvironmentMapLight {
            environment_map: probe.clone(),
            intensity: shot.environment_intensity,
            ..default()
        },
    ));
    // Over a painted probe the catchers mirror the sky too.
    if shot.background {
        e.insert(Skybox {
            image: Some(probe.clone()),
            brightness: shot.environment_intensity,
            ..default()
        });
    } else {
        e.remove::<Skybox>();
    }
    if let Some(mut light) = e.get_mut::<EnvironmentMapLight>() {
        light.intensity = shot.environment_intensity;
    }
    e.get_mut::<Camera>().expect("mirror camera").is_active = true;
    let steps = if fresh { 3 } else { changed as u32 };
    Ok((plane.map(|plane| (m.camera, plane)), steps))
}

/// The world transform of `entity` from the local transforms up to `root`.
fn world_transform(world: &World, entity: Entity, root: Entity) -> Transform {
    let mut out = Transform::IDENTITY;
    let mut at = Some(entity);
    while let Some(e) = at {
        if let Some(local) = world.get::<Transform>(e) {
            out = local.mul_transform(out);
        }
        at = (e != root).then(|| world.get::<ChildOf>(e).map(ChildOf::parent)).flatten();
    }
    out
}

/// The plane through every triangle of `meshes` as a unit normal n and
/// offset d with n . x = d, or `None` when they do not lie in one plane.
fn catcher_plane(world: &World, root: Entity, meshes: &[(Entity, Handle<Mesh>)]) -> Option<Vec4> {
    let assets = world.resource::<Assets<Mesh>>();
    let mut points = Vec::new();
    let mut normal = Vec3::ZERO;
    for (entity, handle) in meshes {
        let mesh = assets.get(handle)?;
        let to_world = world_transform(world, *entity, root).to_matrix();
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return None;
        };
        let start = points.len();
        points.extend(positions.iter().map(|p| to_world.transform_point3(Vec3::from(*p))));
        let corners: Vec<usize> = match mesh.indices() {
            Some(indices) => indices.iter().collect(),
            None => (0..positions.len()).collect(),
        };
        for t in corners.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| points[start + i]);
            normal += (b - a).cross(c - a);
        }
    }
    let normal = normal.try_normalize()?;
    let centre = points.iter().copied().sum::<Vec3>() / points.len() as f32;
    let (lo, hi) = points.iter().fold((Vec3::MAX, Vec3::MIN), |(l, h), p| (l.min(*p), h.max(*p)));
    let tolerance = (hi - lo).length() * 1e-4 + 1e-5;
    points
        .iter()
        .all(|p| normal.dot(*p - centre).abs() <= tolerance)
        .then(|| normal.extend(normal.dot(centre)))
}

/// A stage's catcher material and the plane its catchers lie in, if any.
type Caught = Option<(Handle<ShadowCatcher>, Option<Vec4>)>;

/// Gives every mesh at or below a node named in `names` the shadow catcher
/// material on the catcher layer, and fails on a name that reaches no mesh.
/// Returns the material and the plane the catchers lie in, if they do.
fn catch_shadows(world: &mut World, root: Entity, names: &[String]) -> Result<Caught, String> {
    if names.is_empty() {
        return Ok(None);
    }
    let material = world.resource_mut::<Assets<ShadowCatcher>>().add(ShadowCatcher::default());
    let mut meshes = Vec::new();
    let mut caught = HashSet::new();
    for entity in descendants(world, root) {
        if world.get::<Mesh3d>(entity).is_none() {
            continue;
        }
        let mut at = Some(entity);
        let mut named = None;
        while let Some(e) = at {
            if let Some(name) = world.get::<Name>(e).and_then(|n| names.iter().find(|c| c.as_str() == n.as_str())) {
                named = Some(name.clone());
                break;
            }
            at = (e != root).then(|| world.get::<ChildOf>(e).map(ChildOf::parent)).flatten();
        }
        let Some(name) = named else { continue };
        caught.insert(name);
        meshes.push((entity, world.get::<Mesh3d>(entity).expect("mesh").0.clone()));
        world
            .entity_mut(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert((MeshMaterial3d(material.clone()), RenderLayers::layer(CATCHER_LAYER)));
    }
    if let Some(missing) = names.iter().find(|n| !caught.contains(*n)) {
        return Err(format!("shadow-catcher '{missing}' names no node with a mesh in the model"));
    }
    Ok(Some((material, catcher_plane(world, root, &meshes))))
}

/// Spawns each line of extruded type under `root`: a placed parent holding
/// the faces and the sides, each a mesh with its own material.
fn spawn_texts(world: &mut World, root: Entity, texts: &[Text3d]) -> Result<(), String> {
    for text in texts {
        let solid = crate::text::solid(text)?;
        let material = |color: [f32; 3]| StandardMaterial {
            base_color: Color::srgb(color[0], color[1], color[2]),
            metallic: text.metallic,
            perceptual_roughness: text.roughness,
            ..default()
        };
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let face = materials.add(material(text.color));
        let side = match text.side_color {
            Some(color) => materials.add(material(color)),
            None => face.clone(),
        };
        let [x, y, z] = text.rotation.map(f32::to_radians);
        let transform = Transform {
            translation: Vec3::from(text.position),
            rotation: Quat::from_euler(EulerRot::ZYX, z, y, x),
            scale: Vec3::ONE,
        };
        let parent = world.spawn((transform, Visibility::default(), ChildOf(root))).id();
        for (part, material) in [(&solid.faces, face), (&solid.sides, side)] {
            if part.indices.is_empty() {
                continue;
            }
            let mesh = world.resource_mut::<Assets<Mesh>>().add(part.mesh(text.size));
            world.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::IDENTITY, Visibility::default(), ChildOf(parent)));
        }
    }
    Ok(())
}

/// Whether a stage holds a Solari light source: a sun, or an emissive mesh
/// such as a rect light's emitter.
fn solari_lit(world: &World, root: Entity) -> bool {
    let materials = world.resource::<Assets<StandardMaterial>>();
    descendants(world, root).into_iter().any(|e| {
        world.get::<DirectionalLight>(e).is_some()
            || world
                .get::<MeshMaterial3d<StandardMaterial>>(e)
                .and_then(|m| materials.get(&m.0))
                .is_some_and(|m| m.emissive.red > 0.0 || m.emissive.green > 0.0 || m.emissive.blue > 0.0)
    })
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
