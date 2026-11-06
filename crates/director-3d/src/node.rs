//! `ModelNode`: a glTF scene painted as one element.

use std::any::Any;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use director_core::scene_export::{ElementExport, ExportContext, ExportError};
use director_core::scene_import::{ImportContext, ImportError, ImportRegistry};
use director_core::{Element, RenderError};
use bevy::math::{Quat, Vec3};
use director_scene::{Channel, ModelBody, ModelFog, ModelLight, ModelSplat, ModelText, ModelTrack, ModelView, NodeBody, NodeRecord};
use skia_safe::gpu::{backend_textures, vk as sk_vk, DirectContext, SurfaceOrigin};
use skia_safe::{images, AlphaType, Canvas, Color4f, ColorSpace, ColorType, Data, Image, ImageInfo, Paint, Rect};
use taffy::Style;

use crate::glb::{inspect, GlbInfo};
use crate::studio::{self, camera_bits, Shot, ShotCamera};
use crate::text::{texts_hash, Text3d};

/// Bytes a model reads, with a content hash that keys the studio's stages.
#[derive(Clone)]
pub struct Asset {
    pub bytes: Arc<[u8]>,
    pub hash: u64,
    /// File extension Bevy's loader picks by (`glb`, `hdr`, `exr` or `ply`),
    /// or a font's: `ttf`, `otf` or `ttc`.
    pub ext: &'static str,
    /// Source path, kept so an export references the file.
    pub path: Option<String>,
}

impl Asset {
    pub fn new(bytes: Vec<u8>, ext: &'static str, path: Option<String>) -> Self {
        let mut hasher = std::hash::DefaultHasher::new();
        bytes.hash(&mut hasher);
        ext.hash(&mut hasher);
        Self {
            bytes: bytes.into(),
            hash: hasher.finish(),
            ext,
            path,
        }
    }

    /// An environment map, typed by its magic bytes.
    pub fn environment(bytes: Vec<u8>, path: Option<String>) -> Result<Self, String> {
        let ext = if bytes.starts_with(b"#?") {
            "hdr"
        } else if bytes.starts_with(&[0x76, 0x2f, 0x31, 0x01]) {
            "exr"
        } else {
            return Err("model environment must be a Radiance .hdr or OpenEXR .exr image".into());
        };
        Ok(Self::new(bytes, ext, path))
    }

    /// A Gaussian splat cloud in the 3DGS `.ply` layout.
    pub fn splat(bytes: Vec<u8>, path: Option<String>) -> Result<Self, String> {
        if !(bytes.starts_with(b"ply\n") || bytes.starts_with(b"ply\r\n")) {
            return Err("splat src must be a Gaussian splat .ply file".into());
        }
        Ok(Self::new(bytes, "ply", path))
    }

    /// A TrueType or OpenType font file, typed by its magic bytes.
    pub fn font(bytes: Vec<u8>, path: Option<String>) -> Result<Self, String> {
        let ext = match bytes.get(..4) {
            Some([0, 1, 0, 0] | b"true") => "ttf",
            Some(b"OTTO") => "otf",
            Some(b"ttcf") => "ttc",
            _ => return Err("text3d font must be a TrueType or OpenType file".into()),
        };
        Ok(Self::new(bytes, ext, path))
    }

    /// The scene of a model made only of splats or type: one empty node.
    pub fn empty_scene() -> Self {
        Self::new(crate::glb::empty(), "glb", None)
    }

    fn media_type(&self) -> &'static str {
        match self.ext {
            "glb" => "model/gltf-binary",
            "ply" => "application/x-ply",
            "hdr" => "image/vnd.radiance",
            "ttf" => "font/ttf",
            "otf" => "font/otf",
            "ttc" => "font/collection",
            _ => "image/x-exr",
        }
    }

    fn export(&self, cx: &mut ExportContext, prefix: &str) -> String {
        match &self.path {
            Some(path) => cx.path_asset(prefix, self.media_type(), path),
            None => cx.embedded_asset(prefix, self.media_type(), &self.bytes),
        }
    }
}

impl std::fmt::Debug for Asset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Asset")
            .field("hash", &format_args!("{:016x}", self.hash))
            .field("ext", &self.ext)
            .field("len", &self.bytes.len())
            .field("path", &self.path)
            .finish()
    }
}

/// The most path traced paths per pixel a sample takes.
pub const MAX_PATHS: u32 = 4096;

type FrameKey = (u64, Option<u64>, u64, u64, u64, u64, u32, u32, [u32; 18], [u32; 8], [[u32; 8]; 2]);

/// A Gaussian splat cloud placed in a model's world space.
#[derive(Clone, Debug)]
pub struct Splat {
    pub cloud: Asset,
    /// Metres.
    pub position: [f32; 3],
    /// Degrees about the world X axis, then Y, then Z.
    pub rotation: [f32; 3],
    pub scale: f32,
    /// Multiplies every gaussian's opacity.
    pub opacity: f32,
}

/// A hash of every splat's cloud and placement, which keys the studio's
/// stages and frames.
pub(crate) fn splats_hash(splats: &[Splat]) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    for splat in splats {
        splat.cloud.hash.hash(&mut hasher);
        let numbers = splat.position.iter().chain(&splat.rotation).chain([&splat.scale, &splat.opacity]);
        for n in numbers {
            n.to_bits().hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// A hash of the shadow catcher names, which keys the studio's stages and
/// frames.
pub(crate) fn catchers_hash(names: &[String]) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    names.hash(&mut hasher);
    hasher.finish()
}

/// A hash of every field of `lights`, which keys frames and the studio's
/// spawned lights.
pub(crate) fn lights_hash(lights: &[ModelLight]) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    for light in lights {
        light.kind.hash(&mut hasher);
        let sizes = [light.intensity, light.radius, light.angle, light.cone, light.width, light.height, light.range];
        let numbers = light
            .color
            .iter()
            .chain(light.position.iter().flatten())
            .chain(light.target.iter().flatten())
            .chain(&sizes);
        for n in numbers {
            n.to_bits().hash(&mut hasher);
        }
        (light.position.is_some(), light.target.is_some(), light.shadows).hash(&mut hasher);
    }
    hasher.finish()
}

static TEXTURE_DRAWS: AtomicU64 = AtomicU64::new(0);

/// Model frames this process has drawn from a GPU texture in place.
pub fn texture_draws() -> u64 {
    TEXTURE_DRAWS.load(Ordering::Relaxed)
}

/// A glTF binary scene lit by an environment probe and filmed by the file's
/// first camera, or by a framing camera when the file has none, then orbited
/// by `view`.
pub struct ModelNode {
    pub style: Style,
    pub model: Asset,
    pub environment: Option<Asset>,
    /// Probe brightness in cd/m².
    pub environment_intensity: f32,
    /// Camera exposure in EV100.
    pub exposure: f32,
    /// Paint the probe behind the model. Without it the frame is transparent
    /// where no geometry covers it.
    pub background: bool,
    /// Wrap clips at the longest clip. Without it clips hold their last pose.
    pub looped: bool,
    /// Jittered sub-pixel renders averaged into each frame, 1 to 256.
    pub samples: u32,
    /// Path traced paths per pixel in each sample, 0 to 4096. 0 rasterizes.
    pub paths: u32,
    /// Filter the Hero level's noise, guided by each pixel's first surface.
    pub denoise: bool,
    /// The orbit before any animation, which an export records.
    pub base: ModelView,
    /// The orbit this frame. The HTML element writes its animated camera here.
    pub view: ModelView,
    /// Lights added to the file's own.
    pub lights: Arc<Vec<ModelLight>>,
    /// Gaussian splat clouds beside the file's meshes.
    pub splats: Arc<Vec<Splat>>,
    /// glTF node names whose meshes show only the shadows cast on them.
    pub shadow_catchers: Arc<Vec<String>>,
    /// How strongly the shadow catchers mirror the model, 0 to 1: their
    /// reflectance facing the camera, which rises to 1 at grazing angles.
    pub reflection: f32,
    /// Lines of extruded type beside the file's meshes.
    pub texts: Arc<Vec<Text3d>>,
    /// Cameras solved from footage, one per footage frame, which film the
    /// model in place of the file's camera.
    pub track: Option<Arc<ModelTrack>>,
    /// Distance haze and a sunlit fog volume.
    pub fog: Option<ModelFog>,
    pub info: GlbInfo,
    time: f64,
    frame: Mutex<Option<(FrameKey, Arc<Vec<u8>>)>>,
}

impl ModelNode {
    pub fn new(model: Asset) -> Result<Self, String> {
        let info = inspect(&model.bytes)?;
        Ok(Self {
            style: Style::default(),
            model,
            environment: None,
            environment_intensity: 1000.0,
            exposure: 9.7,
            background: false,
            looped: false,
            samples: 1,
            paths: 0,
            denoise: false,
            base: ModelView::default(),
            view: ModelView::default(),
            lights: Arc::new(Vec::new()),
            splats: Arc::new(Vec::new()),
            shadow_catchers: Arc::new(Vec::new()),
            reflection: 0.0,
            texts: Arc::new(Vec::new()),
            track: None,
            fog: None,
            info,
            time: 0.0,
            frame: Mutex::new(None),
        })
    }

    /// Clip seconds at element time `t`.
    pub fn clip_time(&self, t: f64) -> f64 {
        let d = self.info.duration;
        if d <= 0.0 {
            0.0
        } else if self.looped {
            t.rem_euclid(d)
        } else {
            t.clamp(0.0, d)
        }
    }

    /// The solved camera of the footage frame on show at element time `t`,
    /// held until the next frame so the model stays locked to the plate.
    pub fn tracked_camera(&self, t: f64) -> Option<ShotCamera> {
        let track = self.track.as_ref()?;
        Some(track_pose(track, track_frame(track, t)? as f64))
    }

    /// The cameras at the open and close of the footage shutter centred on
    /// the frame on show at element time `t`, along the solved path between
    /// frames. `None` for a sharp track.
    pub fn tracked_shutter(&self, t: f64) -> Option<[ShotCamera; 2]> {
        let track = self.track.as_ref().filter(|track| track.shutter > 0.0)?;
        let frame = track_frame(track, t)? as f64;
        let half = track.shutter as f64 / 720.0;
        Some([track_pose(track, frame - half), track_pose(track, frame + half)])
    }

    fn body(&self, cx: &mut ExportContext) -> ModelBody {
        ModelBody {
            asset: self.model.export(cx, "model"),
            environment: self.environment.as_ref().map(|e| e.export(cx, "environment")),
            environment_intensity: self.environment_intensity,
            exposure: self.exposure,
            background: self.background,
            looped: self.looped,
            view: self.base,
            samples: self.samples,
            paths: self.paths,
            denoise: self.denoise,
            lights: self.lights.to_vec(),
            splats: self
                .splats
                .iter()
                .map(|s| ModelSplat {
                    asset: s.cloud.export(cx, "splat"),
                    position: s.position,
                    rotation: s.rotation,
                    scale: s.scale,
                    opacity: s.opacity,
                })
                .collect(),
            shadow_catchers: self.shadow_catchers.to_vec(),
            reflection: self.reflection,
            texts: self
                .texts
                .iter()
                .map(|t| ModelText {
                    text: t.text.clone(),
                    font: t.font.export(cx, "font"),
                    weight: t.weight,
                    stretch: t.stretch,
                    size: t.size,
                    depth: t.depth,
                    bevel: t.bevel,
                    position: t.position,
                    rotation: t.rotation,
                    color: t.color,
                    side_color: t.side_color,
                    metallic: t.metallic,
                    roughness: t.roughness,
                })
                .collect(),
            track: self.track.as_deref().cloned(),
            fog: self.fog,
        }
    }
}

impl Clone for ModelNode {
    fn clone(&self) -> Self {
        Self {
            style: self.style.clone(),
            model: self.model.clone(),
            environment: self.environment.clone(),
            environment_intensity: self.environment_intensity,
            exposure: self.exposure,
            background: self.background,
            looped: self.looped,
            samples: self.samples,
            paths: self.paths,
            denoise: self.denoise,
            base: self.base,
            view: self.view,
            lights: self.lights.clone(),
            splats: self.splats.clone(),
            shadow_catchers: self.shadow_catchers.clone(),
            reflection: self.reflection,
            texts: self.texts.clone(),
            track: self.track.clone(),
            fog: self.fog,
            info: self.info,
            time: self.time,
            frame: Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for ModelNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelNode")
            .field("model", &self.model)
            .field("environment", &self.environment)
            .field("splats", &self.splats)
            .field("shadow_catchers", &self.shadow_catchers)
            .field("reflection", &self.reflection)
            .field("texts", &self.texts)
            .field("track", &self.track.as_ref().map(|t| t.frames.len()))
            .field("fog", &self.fog)
            .field("view", &self.view)
            .field("info", &self.info)
            .field("time", &self.time)
            .finish()
    }
}

impl Element for ModelNode {
    fn kind(&self) -> &'static str {
        "model"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout_style(&self) -> Style {
        self.style.clone()
    }

    fn set_layout_style(&mut self, style: Style) {
        self.style = style;
    }

    fn update(&mut self, time: f64) -> bool {
        self.time = time;
        true
    }

    fn varies_with_time(&self) -> bool {
        self.info.clips > 0 || self.track.is_some()
    }

    fn moves_between(&self, t0: f64, t1: f64) -> bool {
        let clips = self.info.clips > 0 && self.clip_time(t0) != self.clip_time(t1);
        let camera = self.track.as_ref().is_some_and(|track| track_frame(track, t0) != track_frame(track, t1));
        clips || camera
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        let device = canvas.local_to_device_as_3x3().map_rect(rect).0;
        let mut width = device.width().round().clamp(1.0, 8192.0) as u32;
        let mut height = device.height().round().clamp(1.0, 8192.0) as u32;
        let time = self.clip_time(self.time);
        let mut camera = self.tracked_camera(self.time);
        let mut shutter = self.tracked_shutter(self.time);
        // A distorted footage lens films through a wider pinhole camera,
        // bent into the lens as it paints. The shutter's cameras widen by
        // the same factor.
        let lens = self.track.as_ref().zip(camera.as_mut()).and_then(|(track, camera)| {
            let lens = crate::lens::overscan(width, height, camera.fov, track.distortion)?;
            let wider = (lens.fov / 2.0).tan() / (camera.fov / 2.0).tan();
            for c in shutter.iter_mut().flatten() {
                c.fov = 2.0 * ((c.fov / 2.0).tan() * wider).atan();
            }
            camera.fov = lens.fov;
            Some(lens)
        });
        if let Some(lens) = &lens {
            (width, height) = (lens.width, lens.height);
        }
        let fog_bits = studio::fog_bits(self.fog);
        let key: FrameKey = (
            self.model.hash,
            self.environment.as_ref().map(|e| e.hash),
            lights_hash(&self.lights),
            splats_hash(&self.splats),
            catchers_hash(&self.shadow_catchers),
            texts_hash(&self.texts),
            width,
            height,
            [
                (time as f32).to_bits(),
                self.view.yaw.to_bits(),
                self.view.pitch.to_bits(),
                self.view.distance.to_bits(),
                self.view.focus.to_bits(),
                self.view.aperture.to_bits(),
                self.exposure.to_bits(),
                self.environment_intensity.to_bits(),
                self.background as u32,
                self.samples,
                self.paths,
                self.denoise as u32,
                self.reflection.to_bits(),
                fog_bits[0],
                fog_bits[1],
                fog_bits[2],
                fog_bits[3],
                fog_bits[4],
            ],
            camera_bits(camera),
            shutter.map_or([[0; 8]; 2], |s| s.map(|c| camera_bits(Some(c)))),
        );
        let cached = self
            .frame
            .lock()
            .expect("model frame cache")
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, rgba)| rgba.clone());
        let shot = Shot {
            model: self.model.clone(),
            clips: self.info.clips,
            environment: self.environment.clone(),
            environment_intensity: self.environment_intensity,
            exposure: self.exposure,
            background: self.background,
            view: self.view,
            time,
            width,
            height,
            samples: self.samples,
            paths: self.paths,
            denoise: self.denoise,
            lights: self.lights.clone(),
            splats: self.splats.clone(),
            catchers: self.shadow_catchers.clone(),
            reflection: self.reflection,
            texts: self.texts.clone(),
            camera,
            shutter,
            fog: self.fog,
        };
        let paint = Paint::new(Color4f::new(1.0, 1.0, 1.0, opacity), None);
        let sampling = skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::None);
        let draw = |image: &Image| match &lens {
            Some(lens) => lens.draw(canvas, image, rect, &paint),
            None => {
                canvas.draw_image_rect_with_sampling_options(image, None, rect, sampling, &paint);
                Ok(())
            }
        };
        if let Some(mut context) = canvas.direct_context().filter(crate::gpu::shares_device) {
            let lease = studio::render_texture(shot).map_err(RenderError::SkiaError)?;
            let texture = lease.texture;
            let image = borrow(&mut context, texture)?;
            let drawn = draw(&image);
            drop(image);
            context.flush_submit_and_sync_cpu();
            drop(lease);
            drawn?;
            TEXTURE_DRAWS.fetch_add(1, Ordering::Relaxed);
            draw_children(canvas);
            return Ok(());
        }
        let rgba = match cached {
            Some(rgba) => rgba,
            None => {
                let frame = studio::render(shot).map_err(RenderError::SkiaError)?;
                let rgba = Arc::new(frame.rgba);
                *self.frame.lock().expect("model frame cache") = Some((key, rgba.clone()));
                rgba
            }
        };
        let info = ImageInfo::new(
            (width as i32, height as i32),
            ColorType::RGBA8888,
            AlphaType::Premul,
            Some(ColorSpace::new_srgb()),
        );
        let image = images::raster_from_data(&info, Data::new_copy(&rgba), width as usize * 4)
            .ok_or_else(|| RenderError::SkiaError("model frame image".into()))?;
        draw(&image)?;
        draw_children(canvas);
        Ok(())
    }

    fn animate_property(
        &mut self,
        property: &str,
        _start: f32,
        _target: f32,
        _duration: f64,
        _easing: &str,
    ) -> Result<(), String> {
        Err(format!("model has no animatable property '{property}'; animate the --camera-* properties on its HTML element"))
    }

    fn export_body(&self, cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
        Ok(ElementExport {
            opacity: Channel::still(1.0),
            body: NodeBody::Model(self.body(cx)),
        })
    }
}

/// Wraps a studio texture as a Skia image on the shared device. The UNORM
/// view reads the sRGB-encoded premultiplied values unchanged, matching the
/// raster image the CPU path builds.
fn borrow(context: &mut DirectContext, texture: studio::Texture) -> Result<Image, RenderError> {
    use ash::vk::{Handle, ImageUsageFlags};
    let usage = ImageUsageFlags::SAMPLED
        | ImageUsageFlags::COLOR_ATTACHMENT
        | ImageUsageFlags::TRANSFER_SRC
        | ImageUsageFlags::TRANSFER_DST;
    let size = (texture.width as i32, texture.height as i32);
    // SAFETY: the studio keeps the image alive, and the caller holds the
    // texture lease until Skia has finished with it.
    let backend = unsafe {
        let mut info = sk_vk::ImageInfo::new(
            ash::vk::Image::from_raw(texture.image).as_raw() as sk_vk::Image,
            sk_vk::Alloc::default(),
            sk_vk::ImageTiling::OPTIMAL,
            sk_vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            sk_vk::Format::R8G8B8A8_UNORM,
            1,
            crate::gpu::family(),
            None,
            None,
            None,
        );
        info.image_usage_flags = usage.as_raw();
        backend_textures::make_vk(size, &info, "director-3d model")
    };
    skia_safe::gpu::images::borrow_texture_from(
        context,
        &backend,
        SurfaceOrigin::TopLeft,
        ColorType::RGBA8888,
        AlphaType::Premul,
        ColorSpace::new_srgb(),
    )
    .ok_or_else(|| RenderError::SkiaError("Skia could not wrap the model texture".into()))
}

/// Rebuilds a model from its document body.
pub fn decode(record: &NodeRecord, cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::Model(body) = &record.body else {
        return Err(ImportError::Invalid(format!("node kind {} has no model body", record.kind)));
    };
    let asset = |cx: &mut ImportContext, id: &str| -> Result<(Vec<u8>, Option<String>), ImportError> {
        Ok((cx.asset_bytes(id)?, cx.asset_path(id).map(str::to_string)))
    };
    let (bytes, path) = asset(cx, &body.asset)?;
    let mut node = ModelNode::new(Asset::new(bytes, "glb", path)).map_err(|reason| ImportError::Asset {
        id: body.asset.clone(),
        reason,
    })?;
    if let Some(id) = &body.environment {
        let (bytes, path) = asset(cx, id)?;
        node.environment = Some(Asset::environment(bytes, path).map_err(|reason| ImportError::Asset {
            id: id.clone(),
            reason,
        })?);
    }
    node.environment_intensity = body.environment_intensity;
    node.exposure = body.exposure;
    node.background = body.background;
    node.looped = body.looped;
    node.samples = body.samples.clamp(1, 256);
    node.paths = body.paths.min(MAX_PATHS);
    node.denoise = body.denoise;
    node.base = body.view;
    node.view = body.view;
    node.lights = Arc::new(body.lights.clone());
    let mut splats = Vec::with_capacity(body.splats.len());
    for splat in &body.splats {
        let (bytes, path) = asset(cx, &splat.asset)?;
        splats.push(Splat {
            cloud: Asset::splat(bytes, path).map_err(|reason| ImportError::Asset {
                id: splat.asset.clone(),
                reason,
            })?,
            position: splat.position,
            rotation: splat.rotation,
            scale: splat.scale,
            opacity: splat.opacity,
        });
    }
    node.splats = Arc::new(splats);
    node.shadow_catchers = Arc::new(body.shadow_catchers.clone());
    node.reflection = body.reflection.clamp(0.0, 1.0);
    if let Some(fog) = body.fog {
        let channels = fog.color.iter().all(|c| (0.0..=1.0).contains(c));
        let amounts = [fog.visibility, fog.density].iter().all(|v| v.is_finite() && *v >= 0.0);
        if !(channels && amounts && (fog.visibility > 0.0 || fog.density > 0.0)) || body.paths > 0 {
            return Err(ImportError::Invalid(
                "model fog needs colour channels from 0 to 1, a visibility or a density above 0 and both from 0, on a rasterized model".into(),
            ));
        }
        node.fog = Some(fog);
    }
    if let Some(track) = &body.track {
        let fov = |f: f32| f > 0.0 && f < 180.0;
        let lens = fov(track.fov) && track.frames.iter().all(|f| f.fov.is_none_or(fov)) && track.distortion.iter().all(|k| k.is_finite());
        if !(track.fps > 0.0 && lens && (0.0..=360.0).contains(&track.shutter) && !track.frames.is_empty()) {
            return Err(ImportError::Invalid(
                "a model track needs a positive fps, a field of view between 0 and 180 degrees, finite distortion, a shutter from 0 to 360 degrees and at least one frame".into(),
            ));
        }
        node.track = Some(Arc::new(track.clone()));
    }
    let mut texts = Vec::with_capacity(body.texts.len());
    for t in &body.texts {
        let (bytes, path) = asset(cx, &t.font)?;
        texts.push(Text3d {
            font: Asset::font(bytes, path).map_err(|reason| ImportError::Asset {
                id: t.font.clone(),
                reason,
            })?,
            text: t.text.clone(),
            weight: t.weight,
            stretch: t.stretch,
            size: t.size,
            depth: t.depth,
            bevel: t.bevel,
            position: t.position,
            rotation: t.rotation,
            color: t.color,
            side_color: t.side_color,
            metallic: t.metallic,
            roughness: t.roughness,
        });
    }
    node.texts = Arc::new(texts);
    Ok(Box::new(node))
}

/// Registers the `model` decoder.
pub fn register_import(registry: &mut ImportRegistry) {
    registry.register("model", decode);
}

/// The footage frame on show at element time `t`: frame k from k / fps
/// seconds, the first before it and the last after the footage ends.
fn track_frame(track: &ModelTrack, t: f64) -> Option<usize> {
    let last = track.frames.len().checked_sub(1)?;
    Some(((t * track.fps + 1e-6).floor().max(0.0) as usize).min(last))
}

/// The camera at fractional footage frame `f`, held at the ends and blended
/// between neighbours: positions and lenses linearly, rotations by slerp.
fn track_pose(track: &ModelTrack, f: f64) -> ShotCamera {
    let last = track.frames.len() - 1;
    let f = f.clamp(0.0, last as f64);
    let k = (f.floor() as usize).min(last);
    let pose = |k: usize| {
        let p = &track.frames[k];
        ShotCamera {
            position: Vec3::from_array(p.position),
            rotation: Quat::from_array(p.rotation).normalize(),
            fov: p.fov.unwrap_or(track.fov).to_radians(),
        }
    };
    let (a, blend) = (pose(k), (f - k as f64) as f32);
    if blend <= 0.0 || k == last {
        return a;
    }
    let b = pose(k + 1);
    ShotCamera {
        position: a.position.lerp(b.position, blend),
        rotation: a.rotation.slerp(b.rotation, blend),
        fov: a.fov + (b.fov - a.fov) * blend,
    }
}
