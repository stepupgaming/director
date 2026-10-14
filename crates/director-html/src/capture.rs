//! Load an explicit compile-time browser extraction. No browser is used by this module.
use crate::{
    compiler::{fit, load_fonts, read_asset, span, text_content, validate_options},
    css::{diagnostic, Values},
    element::HtmlElement,
    style::{self, Metrics},
    CompileError, CompileOptions, CompiledHtml,
};
use director_core::{
    node::{ImageNode, VectorNode},
    scene::SceneGraph,
    types::NodeId,
    Director, Element, RenderError,
};
use serde::{Deserialize, Serialize};
use std::{
    any::Any,
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{Arc, Mutex},
};

pub const CAPTURE_FORMAT: &str = "director-html-capture/1";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub format: String,
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub duration: f64,
    pub sample_rate: f64,
    #[serde(default)]
    pub fonts: Vec<(String, String)>,
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
    pub frames: Vec<CapturedFrame>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedFrame {
    pub time: f64,
    pub root: CapturedNode,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedNode {
    pub tag: String,
    #[serde(default)]
    pub id: String,
    pub rect: [f32; 4],
    pub styles: Values,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub svg: Option<String>,
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub children: Vec<CapturedNode>,
}

pub fn compile_capture_file(path: &Path) -> Result<CompiledHtml, CompileError> {
    let bytes = std::fs::read(path)
        .map_err(|e| CompileError::Input(format!("capture {}: {e}", path.display())))?;
    let capture: Capture = serde_json::from_slice(&bytes)
        .map_err(|e| CompileError::Input(format!("capture JSON: {e}")))?;
    compile_capture(capture, path.parent().unwrap_or(Path::new(".")))
}
pub fn compile_capture(capture: Capture, base: &Path) -> Result<CompiledHtml, CompileError> {
    if capture.format != CAPTURE_FORMAT {
        return Err(CompileError::Input(format!(
            "unsupported capture format '{}'",
            capture.format
        )));
    }
    validate_options(&CompileOptions {
        width: capture.width,
        height: capture.height,
        fps: capture.fps,
        duration: capture.duration,
        ..Default::default()
    })?;
    if !capture.sample_rate.is_finite()
        || capture.sample_rate <= 0.0
        || capture.sample_rate > 1000.0
        || capture.frames.is_empty()
    {
        return Err(CompileError::Input(
            "capture must have frames and a finite sample rate in (0,1000]".into(),
        ));
    }
    let expected = (capture.duration * capture.sample_rate).ceil() as usize;
    if capture.frames.len() != expected {
        return Err(CompileError::Input(format!(
            "capture needs {expected} frames, got {}",
            capture.frames.len()
        )));
    }
    for (i, frame) in capture.frames.iter().enumerate() {
        if !frame.time.is_finite() || (frame.time - i as f64 / capture.sample_rate).abs() > 1e-7 {
            return Err(CompileError::Input(format!(
                "capture frame {i} has incorrect timestamp"
            )));
        }
    }
    let mut director = Director::new(
        capture.width,
        capture.height,
        capture.fps,
        Arc::new(crate::compiler::HtmlAssetLoader),
        director_core::video_wrapper::RenderMode::Export,
        None,
    );
    load_fonts(&mut director, &capture.fonts, base)?;
    let metrics = Metrics {
        width: capture.width as f32,
        height: capture.height as f32,
        font_size: 16.0,
    };
    let mut frame_cache: HashMap<String, Arc<Mutex<SceneGraph>>> = HashMap::new();
    let mut image_cache: HashMap<String, skia_safe::Image> = HashMap::new();
    let mut frames = Vec::new();
    let mut node_count = 0;
    for frame in &capture.frames {
        let signature =
            serde_json::to_string(&frame.root).map_err(|e| CompileError::Input(e.to_string()))?;
        let (scene, root) = if let Some(scene) = frame_cache.get(&signature) {
            (scene.clone(), NodeId::at(0))
        } else {
            let mut scene = SceneGraph::new();
            let root = build(
                &frame.root,
                &mut scene,
                &director,
                &capture.assets,
                &mut image_cache,
                base,
                metrics,
                0,
            )?;
            node_count += scene.iter_alive().count();
            let scene = Arc::new(Mutex::new(scene));
            frame_cache.insert(signature, scene.clone());
            (scene, root)
        };
        frames.push((scene, root));
    }
    let mut element = CapturedElement {
        frames: Arc::new(frames),
        sample_rate: capture.sample_rate,
        index: 0,
        style: taffy::Style {
            size: taffy::geometry::Size {
                width: taffy::prelude::length(capture.width as f32),
                height: taffy::prelude::length(capture.height as f32),
            },
            ..Default::default()
        },
        assets: director.assets.clone(),
        cache: Mutex::new(HashMap::new()),
    };
    element.update(0.0);
    let root = director.scene.add_node(Box::new(element));
    director.push_scene_root(root, capture.duration);
    director.compile();
    Ok(CompiledHtml {
        director,
        element_ids: BTreeMap::new(),
        source_kind: "sampled-dom".into(),
        sample_rate: Some(capture.sample_rate),
        node_count,
    })
}

#[allow(clippy::too_many_arguments)]
fn build(
    n: &CapturedNode,
    scene: &mut SceneGraph,
    director: &Director,
    assets: &BTreeMap<String, String>,
    images: &mut HashMap<String, skia_safe::Image>,
    base: &Path,
    m: Metrics,
    depth: usize,
) -> Result<NodeId, CompileError> {
    if depth > 256 || scene.nodes.len() > 100000 {
        return Err(CompileError::Input(
            "captured tree exceeds compiler limits".into(),
        ));
    }
    if n.rect.iter().any(|v| !v.is_finite()) || n.rect[2] < 0.0 || n.rect[3] < 0.0 {
        return Err(CompileError::Input(format!(
            "{}: invalid captured rectangle",
            n.id
        )));
    }
    let (style, paint) = style::translate(&n.styles, m).map_err(|e| {
        CompileError::Diagnostics(e.into_iter().map(|e| diagnostic(&n.id, e)).collect())
    })?;
    let content: Option<Box<dyn Element>> = if let Some(svg) = &n.svg {
        crate::compiler::validate_svg(svg)?;
        usvg::Tree::from_data(svg.as_bytes(), &usvg::Options::default())
            .map_err(|e| CompileError::Input(format!("{}: SVG: {e}", n.id)))?;
        Some(Box::new(VectorNode::new(svg.as_bytes())))
    } else if let Some(text) = &n.text {
        Some(Box::new(text_content(
            vec![span(text.clone(), &paint)],
            &paint,
            director,
        )))
    } else if n.tag == "img" {
        let src = n
            .src
            .as_ref()
            .ok_or_else(|| CompileError::Input("captured image missing src".into()))?;
        let data = assets
            .get(src)
            .ok_or_else(|| CompileError::Input(format!("captured image asset missing: {src}")))?;
        let image = if let Some(image) = images.get(src) {
            image.clone()
        } else {
            let bytes = read_asset(data, base)?;
            let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&bytes))
                .ok_or_else(|| CompileError::Input("invalid captured image".into()))?;
            images.insert(src.clone(), image.clone());
            image
        };
        let mut image = ImageNode::from_image(Some(image));
        image.object_fit = fit(&n.styles);
        Some(Box::new(image))
    } else if matches!(n.tag.as_str(), "canvas" | "video" | "audio" | "iframe") {
        return Err(CompileError::Diagnostics(vec![diagnostic(&n.id,format!("captured {} requires native media/shader translation; use the static compiler for media",n.tag))]));
    } else {
        None
    };
    let mut el = HtmlElement::new(
        style,
        paint.clone(),
        content,
        Metrics {
            font_size: paint.font_size,
            ..m
        },
    );
    el.post_layout(skia_safe::Rect::from_wh(n.rect[2], n.rect[3]));
    let id = scene.add_node(Box::new(el));
    let node = scene.get_node_mut(id).unwrap();
    node.layout_rect = skia_safe::Rect::from_xywh(n.rect[0], n.rect[1], n.rect[2], n.rect[3]);
    node.z_index = paint.z_index;
    for child in &n.children {
        let child = build(child, scene, director, assets, images, base, m, depth + 1)?;
        scene.add_child(id, child);
    }
    Ok(id)
}

struct CapturedElement {
    frames: Arc<Vec<(Arc<Mutex<SceneGraph>>, NodeId)>>,
    sample_rate: f64,
    index: usize,
    style: taffy::Style,
    assets: director_core::systems::assets::AssetManager,
    cache: Mutex<HashMap<NodeId, skia_safe::Image>>,
}
impl std::fmt::Debug for CapturedElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapturedElement")
            .field("frames", &self.frames.len())
            .field("sample_rate", &self.sample_rate)
            .finish()
    }
}
impl Clone for CapturedElement {
    fn clone(&self) -> Self {
        Self {
            frames: self.frames.clone(),
            sample_rate: self.sample_rate,
            index: self.index,
            style: self.style.clone(),
            assets: self.assets.clone(),
            cache: Mutex::new(HashMap::new()),
        }
    }
}
impl Element for CapturedElement {
    fn kind(&self) -> &'static str {
        "html_capture"
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn layout_style(&self) -> taffy::Style {
        self.style.clone()
    }
    fn update(&mut self, time: f64) -> bool {
        let index =
            ((time.max(0.0) * self.sample_rate + 1e-7).floor() as usize).min(self.frames.len() - 1);
        self.index = index;
        false
    }
    fn render(
        &self,
        canvas: &skia_safe::Canvas,
        _: skia_safe::Rect,
        opacity: f32,
        _: &mut dyn FnMut(&skia_safe::Canvas),
    ) -> Result<(), RenderError> {
        let (scene, root) = &self.frames[self.index];
        let scene = scene.lock().unwrap();
        director_core::systems::renderer::render_recursive(
            &scene,
            &self.assets,
            &mut self.cache.lock().unwrap(),
            *root,
            canvas,
            opacity,
            0,
            None,
        )
    }
    fn animate_property(&mut self, _: &str, _: f32, _: f32, _: f64, _: &str) -> Result<(), String> {
        Err("captured timelines are immutable".into())
    }
}
