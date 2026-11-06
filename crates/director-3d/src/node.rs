//! `ModelNode`: a glTF scene painted as one element.

use std::any::Any;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use director_core::scene_export::{ElementExport, ExportContext, ExportError};
use director_core::scene_import::{ImportContext, ImportError, ImportRegistry};
use director_core::{Element, RenderError};
use director_scene::{Channel, ModelBody, ModelView, NodeBody, NodeRecord};
use skia_safe::{images, AlphaType, Canvas, Color4f, ColorSpace, ColorType, Data, ImageInfo, Paint, Rect};
use taffy::Style;

use crate::glb::{inspect, GlbInfo};
use crate::studio::{self, Shot};

/// Bytes a model reads, with a content hash that keys the studio's stages.
#[derive(Clone)]
pub struct Asset {
    pub bytes: Arc<[u8]>,
    pub hash: u64,
    /// File extension Bevy's loader picks by: `glb`, `hdr` or `exr`.
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

    fn media_type(&self) -> &'static str {
        match self.ext {
            "glb" => "model/gltf-binary",
            "hdr" => "image/vnd.radiance",
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

type FrameKey = (u64, Option<u64>, u32, u32, [u32; 7]);

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
    /// The orbit before any animation, which an export records.
    pub base: ModelView,
    /// The orbit this frame. The HTML element writes its animated camera here.
    pub view: ModelView,
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
            base: ModelView::default(),
            view: ModelView::default(),
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

    fn body(&self, cx: &mut ExportContext) -> ModelBody {
        ModelBody {
            asset: self.model.export(cx, "model"),
            environment: self.environment.as_ref().map(|e| e.export(cx, "environment")),
            environment_intensity: self.environment_intensity,
            exposure: self.exposure,
            background: self.background,
            looped: self.looped,
            view: self.base,
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
            base: self.base,
            view: self.view,
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
        self.info.clips > 0
    }

    fn moves_between(&self, t0: f64, t1: f64) -> bool {
        self.info.clips > 0 && self.clip_time(t0) != self.clip_time(t1)
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        let device = canvas.local_to_device_as_3x3().map_rect(rect).0;
        let width = device.width().round().clamp(1.0, 8192.0) as u32;
        let height = device.height().round().clamp(1.0, 8192.0) as u32;
        let time = self.clip_time(self.time);
        let key: FrameKey = (
            self.model.hash,
            self.environment.as_ref().map(|e| e.hash),
            width,
            height,
            [
                (time as f32).to_bits(),
                self.view.yaw.to_bits(),
                self.view.pitch.to_bits(),
                self.view.distance.to_bits(),
                self.exposure.to_bits(),
                self.environment_intensity.to_bits(),
                self.background as u32,
            ],
        );
        let cached = self
            .frame
            .lock()
            .expect("model frame cache")
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, rgba)| rgba.clone());
        let rgba = match cached {
            Some(rgba) => rgba,
            None => {
                let frame = studio::render(Shot {
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
                })
                .map_err(RenderError::SkiaError)?;
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
        let paint = Paint::new(Color4f::new(1.0, 1.0, 1.0, opacity), None);
        canvas.draw_image_rect_with_sampling_options(
            &image,
            None,
            rect,
            skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::None),
            &paint,
        );
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
        Err(format!("model has no animatable property '{property}'; animate --camera-yaw, --camera-pitch and --camera-distance on its HTML element"))
    }

    fn export_body(&self, cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
        Ok(ElementExport {
            opacity: Channel::still(1.0),
            body: NodeBody::Model(self.body(cx)),
        })
    }
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
    node.base = body.view;
    node.view = body.view;
    Ok(Box::new(node))
}

/// Registers the `model` decoder.
pub fn register_import(registry: &mut ImportRegistry) {
    registry.register("model", decode);
}
