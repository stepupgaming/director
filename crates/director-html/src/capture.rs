//! Load an explicit compile-time browser extraction. No browser is used by this module.
use crate::{
    assets::AssetStore,
    compiler::{
        cached_svg_fonts, fit, load_fonts, looped_media_time, media_duration, parse_svg, span,
        text_content, validate_options,
    },
    css::{diagnostic, Values},
    element::HtmlElement,
    style::{self, Metrics},
    CompileError, CompileOptions, CompiledHtml,
};
use director_core::{
    node::{ImageNode, VectorNode, VideoNode, VideoSource},
    scene::SceneGraph,
    scene_export::{channel, export_graph, node_ref, nodes, ElementExport, ExportContext, ExportError},
    types::{NodeId, ObjectFit, ObjectPosition},
    video_wrapper::RenderMode,
    Director, Element, RenderError,
};
use director_core::scene_import::{import_graph, nodes as import_nodes, resolve, ImportContext, ImportError};
use director_scene::{NodeBody, NodeRecord};
use serde::{Deserialize, Serialize};
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet, HashMap},
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shader: Option<CapturedShader>,
    /// Present on captured `video` and `audio`. `src`, timing, mute, loop, and volume
    /// stay fixed for an id. Per-sample fit and rect may change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<CapturedMedia>,
    #[serde(default)]
    pub children: Vec<CapturedNode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedMedia {
    pub muted: bool,
    pub looped: bool,
    pub hidden: bool,
    pub volume: f64,
    pub media_start: f64,
    pub playback_rate: f64,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedShader {
    pub fragment: String,
    #[serde(default)]
    pub uniforms: BTreeMap<String, Vec<f32>>,
    #[serde(default)]
    pub images: BTreeMap<String, CapturedShaderImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedShaderImage {
    pub width: i32,
    pub height: i32,
    pub rgba: String,
    /// True when WebGL uploaded the image with UNPACK_FLIP_Y_WEBGL.
    #[serde(default)]
    pub flip_y: bool,
}

pub fn compile_capture_file(path: &Path) -> Result<CompiledHtml, CompileError> {
    let bytes = std::fs::read(path)
        .map_err(|e| CompileError::Input(format!("capture {}: {e}", path.display())))?;
    let capture: Capture = serde_json::from_slice(&bytes)
        .map_err(|e| CompileError::Input(format!("capture JSON: {e}")))?;
    compile_capture(capture, path.parent().unwrap_or(Path::new(".")))
}
pub fn compile_capture(capture: Capture, base: &Path) -> Result<CompiledHtml, CompileError> {
    compile_capture_with_assets(capture, Arc::new(AssetStore::local(base, false)))
}

pub(crate) fn compile_capture_with_assets(
    mut capture: Capture,
    assets: Arc<AssetStore>,
) -> Result<CompiledHtml, CompileError> {
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
    for frame in &capture.frames {
        let mut seen = BTreeSet::new();
        reject_duplicate_media(&frame.root, &mut seen)?;
    }
    normalize_ids(&mut capture);
    let mut video_specs: BTreeMap<String, MediaSpec> = BTreeMap::new();
    let mut audio_specs: BTreeMap<String, MediaSpec> = BTreeMap::new();
    for frame in &capture.frames {
        let mut found = Vec::new();
        collect_media(&frame.root, &mut found);
        for node in found {
            if node.tag == "iframe" {
                return Err(CompileError::Diagnostics(vec![diagnostic(
                    &node.id,
                    "captured iframe requires native media translation; use the static compiler for media",
                )]));
            }
            let spec = media_spec(node)?;
            let slot = if node.tag == "audio" {
                &mut audio_specs
            } else {
                &mut video_specs
            };
            if let Some(previous) = slot.get(&node.id) {
                if previous != &spec {
                    return Err(CompileError::Input(format!(
                        "captured media '{}' changes across samples",
                        node.id
                    )));
                }
            } else {
                slot.insert(node.id.clone(), spec);
            }
        }
    }
    let mut director = Director::new(
        capture.width,
        capture.height,
        capture.fps,
        assets.clone(),
        director_core::video_wrapper::RenderMode::Export,
        None,
    );
    load_fonts(&mut director, &capture.fonts, &assets)?;
    for spec in audio_specs.values() {
        let packed = capture.assets.get(&spec.src).ok_or_else(|| {
            CompileError::Input(format!("captured media asset missing: {}", spec.src))
        })?;
        let bytes = assets.read(packed)?;
        let mut samples = director_core::audio::load_audio_bytes(&bytes, 48000)
            .map_err(|e| CompileError::Input(format!("audio '{}': {e}", spec.src)))?;
        samples.drain(..((spec.media.media_start * 96000.0) as usize).min(samples.len()));
        let track_id =
            director.add_scene_audio(samples, spec.media.start, spec.media.end - spec.media.start);
        if let Some(track) = director
            .audio_mixer
            .tracks
            .get_mut(track_id)
            .and_then(|track| track.as_mut())
        {
            track.volume = director_core::animation::Animated::new(if spec.media.hidden {
                0.0
            } else {
                spec.media.volume as f32
            });
            track.loop_audio = spec.media.looped;
            if !packed.starts_with("data:") {
                track.source_path = Some(packed.clone());
                track.source_start = spec.media.media_start;
            }
        }
    }
    let metrics = Metrics {
        width: capture.width as f32,
        height: capture.height as f32,
        font_size: 16.0,
    };
    let mut svg_fonts = None;
    let mut frame_cache: HashMap<String, Arc<Mutex<SceneGraph>>> = HashMap::new();
    let mut image_cache: HashMap<String, skia_safe::Image> = HashMap::new();
    let mut videos: BTreeMap<String, Arc<Mutex<VideoNode>>> = BTreeMap::new();
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
                &mut videos,
                &assets,
                &mut svg_fonts,
                &capture.fonts,
                metrics,
                style::ContainingBlock::from_axes(capture.width as f32, capture.height as f32),
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
    let source_inspection = crate::SourceInspection {
        sample_rate: capture.sample_rate,
        samples: capture
            .frames
            .iter()
            .map(|frame| {
                let mut nodes = Vec::new();
                inspect_node(&frame.root, &mut nodes);
                nodes
            })
            .collect(),
    };
    Ok(CompiledHtml {
        director,
        element_ids: BTreeMap::new(),
        source_kind: "sampled-dom".into(),
        sample_rate: Some(capture.sample_rate),
        node_count,
        source_inspection: Some(Arc::new(source_inspection)),
        assets,
    })
}

/// Anonymous/ambiguous captured elements use structural paths. Authored unique
/// IDs remain intact; paths retain identity while the sampled topology is stable.
pub(crate) fn normalize_ids(capture: &mut Capture) {
    fn count(node: &CapturedNode, counts: &mut BTreeMap<String, usize>) {
        *counts.entry(node.id.clone()).or_default() += 1;
        for child in &node.children {
            count(child, counts);
        }
    }
    fn rename(node: &mut CapturedNode, ambiguous: &BTreeSet<String>, prefix: &str, path: &str) {
        if node.id.is_empty() || ambiguous.contains(&node.id) {
            node.id = format!("{prefix}{path}");
        }
        for (index, child) in node.children.iter_mut().enumerate() {
            rename(child, ambiguous, prefix, &format!("{path}/{index}"));
        }
    }
    let mut observed = BTreeSet::new();
    let mut ambiguous = BTreeSet::new();
    for frame in &capture.frames {
        let mut counts = BTreeMap::new();
        count(&frame.root, &mut counts);
        for (id, count) in counts {
            if count > 1 {
                ambiguous.insert(id.clone());
            }
            observed.insert(id);
        }
    }
    let mut prefix = "@capture".to_owned();
    while observed.iter().any(|id| id.starts_with(&prefix)) {
        prefix.push('@');
    }
    for frame in &mut capture.frames {
        rename(&mut frame.root, &ambiguous, &prefix, "/root");
    }
}

fn inspect_node(node: &CapturedNode, nodes: &mut Vec<crate::InspectedElement>) {
    nodes.push(crate::InspectedElement {
        id: node.id.clone(),
        tag: node.tag.clone(),
        bounds: node.rect,
        text: node.text.clone(),
        children: node.children.len(),
    });
    for child in &node.children {
        inspect_node(child, nodes);
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    n: &CapturedNode,
    scene: &mut SceneGraph,
    director: &Director,
    assets: &BTreeMap<String, String>,
    images: &mut HashMap<String, skia_safe::Image>,
    videos: &mut BTreeMap<String, Arc<Mutex<VideoNode>>>,
    store: &AssetStore,
    svg_fonts: &mut Option<Arc<usvg::fontdb::Database>>,
    font_faces: &[(String, String)],
    m: Metrics,
    block: style::ContainingBlock,
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
    let (mut style, mut paint, _grid_scope) = style::translate(&n.styles, m, block, store).map_err(|e| {
        CompileError::Diagnostics(e.into_iter().map(|e| diagnostic(&n.id, e)).collect())
    })?;
    // Sampled px stays px. A leftover calc uses this node's used size when the style
    // does not already name a definite content box.
    let derived = style::content_box(&style, block, false);
    let own = style::ContainingBlock {
        width: derived.width.or_else(|| style::axis_basis(n.rect[2])),
        height: derived.height.or_else(|| style::axis_basis(n.rect[3])),
    };
    let gap_metrics = Metrics {
        font_size: paint.font_size,
        ..m
    };
    style::resolve_gaps(&mut style, &n.styles, gap_metrics, own).map_err(|errors| {
        CompileError::Diagnostics(
            errors
                .into_iter()
                .map(|error| diagnostic(&n.id, error))
                .collect(),
        )
    })?;
    paint.box_node.style = style.clone();
    let mut paper = None;
    let content: Option<Box<dyn Element>> = if let Some(svg) = &n.svg {
        crate::compiler::validate_captured_svg(svg)?;
        let fonts = cached_svg_fonts(svg_fonts, store, font_faces)?;
        Some(Box::new(VectorNode::from_tree(parse_svg(svg, &n.id, &fonts)?)))
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
            let bytes = store.read(data)?;
            let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&bytes))
                .ok_or_else(|| CompileError::Input("invalid captured image".into()))?;
            images.insert(src.clone(), image.clone());
            image
        };
        let mut image = ImageNode::from_image(Some(image));
        image.object_fit = fit(&n.styles);
        image.object_position = paint.object_position;
        Some(Box::new(image))
    } else if n.tag == "canvas" {
        let shader = n.shader.as_ref().ok_or_else(|| {
            CompileError::Diagnostics(vec![diagnostic(
                &n.id,
                "captured canvas requires a native shader translation".to_string(),
            )])
        })?;
        paper = Some(
            crate::shader::prepare(shader)
                .map_err(|message| CompileError::Diagnostics(vec![diagnostic(&n.id, message)]))?,
        );
        None
    } else if n.tag == "video" {
        let src = n
            .src
            .as_deref()
            .filter(|src| !src.is_empty())
            .ok_or_else(|| CompileError::Input("video without src".into()))?;
        let shared = if let Some(shared) = videos.get(&n.id) {
            Arc::clone(shared)
        } else {
            let packed = assets.get(src).ok_or_else(|| {
                CompileError::Input(format!("captured media asset missing: {src}"))
            })?;
            let path = store.media_path(packed)?;
            let mut video = VideoNode::new(VideoSource::Path(path), RenderMode::Export);
            if !packed.starts_with("data:") {
                video = video.with_source_path(packed);
            }
            // Cloning VideoNode opens a second decoder. Samples share this one.
            // Style's raw pointer keeps VideoNode `!Send`; it is a niche discriminant,
            // the same claim SceneNode already makes.
            #[allow(clippy::arc_with_non_send_sync)]
            let shared = Arc::new(Mutex::new(video));
            videos.insert(n.id.clone(), Arc::clone(&shared));
            shared
        };
        let looped = n.media.as_ref().is_some_and(|media| media.looped);
        let duration = if looped {
            let path = shared.lock().unwrap().path().clone();
            media_duration(&path)?
        } else {
            0.0
        };
        Some(Box::new(SharedVideo {
            node: shared,
            object_fit: fit(&n.styles),
            object_position: paint.object_position,
            looped,
            duration,
        }))
    } else if n.tag == "audio" {
        None
    } else if n.tag == "iframe" {
        return Err(CompileError::Diagnostics(vec![diagnostic(
            &n.id,
            "captured iframe requires native media translation; use the static compiler for media",
        )]));
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
    el.paper = paper;
    if n.tag == "video" {
        if let Some(media) = &n.media {
            el.start = media.start;
            el.end = media.end;
            el.media_start = media.media_start;
            el.playback_rate = media.playback_rate;
            el.hidden = media.hidden;
        }
    }
    if n.tag == "audio" {
        el.hidden = true;
    }
    el.post_layout(skia_safe::Rect::from_wh(n.rect[2], n.rect[3]));
    let id = scene.add_node(Box::new(el));
    let node = scene.get_node_mut(id).unwrap();
    node.layout_rect = skia_safe::Rect::from_xywh(n.rect[0], n.rect[1], n.rect[2], n.rect[3]);
    node.z_index = paint.z_index;
    // Audio is already one mixer track. Video children are not visual content.
    if !matches!(n.tag.as_str(), "audio" | "video") {
        for child in &n.children {
            if child.tag == "audio" {
                continue;
            }
            let child = build(
                child,
                scene,
                director,
                assets,
                images,
                videos,
                store,
                svg_fonts,
                font_faces,
                m,
                own,
                depth + 1,
            )?;
            scene.add_child(id, child);
        }
    }
    Ok(id)
}

#[derive(PartialEq)]
struct MediaSpec {
    src: String,
    media: CapturedMedia,
}

fn reject_duplicate_media(
    node: &CapturedNode,
    seen: &mut BTreeSet<String>,
) -> Result<(), CompileError> {
    if matches!(node.tag.as_str(), "video" | "audio")
        && !node.id.is_empty()
        && !seen.insert(node.id.clone())
    {
        return Err(CompileError::Input(format!(
            "duplicate captured media id '{}'",
            node.id
        )));
    }
    for child in &node.children {
        reject_duplicate_media(child, seen)?;
    }
    Ok(())
}

fn collect_media<'a>(node: &'a CapturedNode, found: &mut Vec<&'a CapturedNode>) {
    if matches!(node.tag.as_str(), "video" | "audio" | "iframe") {
        found.push(node);
    }
    for child in &node.children {
        collect_media(child, found);
    }
}

fn media_spec(node: &CapturedNode) -> Result<MediaSpec, CompileError> {
    let media = node.media.clone().ok_or_else(|| {
        CompileError::Input(format!(
            "{}: captured {} requires media timing",
            node.id, node.tag
        ))
    })?;
    let src = node
        .src
        .clone()
        .filter(|src| !src.is_empty())
        .ok_or_else(|| CompileError::Input(format!("{} without src", node.tag)))?;
    if !media.media_start.is_finite()
        || media.media_start < 0.0
        || !media.playback_rate.is_finite()
        || media.playback_rate <= 0.0
    {
        return Err(CompileError::Input(
            "media start must be nonnegative and rate positive".into(),
        ));
    }
    if !media.start.is_finite()
        || !media.end.is_finite()
        || media.start < 0.0
        || media.end <= media.start
    {
        return Err(CompileError::Input(format!(
            "{}: invalid clip window [{}, {})",
            node.id, media.start, media.end
        )));
    }
    if node.tag == "video" && !media.muted {
        return Err(CompileError::Input(
            "video audio needs an explicit audio element; author muted video".into(),
        ));
    }
    if node.tag == "audio" && media.playback_rate != 1.0 {
        return Err(CompileError::Input(
            "audio playback-rate translation unsupported".into(),
        ));
    }
    if node.tag == "audio"
        && (!media.volume.is_finite() || media.volume < 0.0 || media.volume > f32::MAX as f64)
    {
        return Err(CompileError::Input(
            "audio volume must be finite and nonnegative".into(),
        ));
    }
    Ok(MediaSpec { src, media })
}

/// Forwards seek and draw to one shared decoder. Fit and position belong to this sample.
#[derive(Debug)]
struct SharedVideo {
    node: Arc<Mutex<VideoNode>>,
    object_fit: ObjectFit,
    object_position: ObjectPosition,
    looped: bool,
    duration: f64,
}

impl Clone for SharedVideo {
    fn clone(&self) -> Self {
        Self {
            node: Arc::clone(&self.node),
            object_fit: self.object_fit,
            object_position: self.object_position,
            looped: self.looped,
            duration: self.duration,
        }
    }
}

impl Element for SharedVideo {
    fn kind(&self) -> &'static str {
        "html_video"
    }
    fn export_body(&self, cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
        let node = self
            .node
            .lock()
            .map_err(|_| ExportError::Unrepresentable("poisoned shared video".to_string()))?;
        let mut video = nodes::video_media(&node, cx)?;
        video.fit = nodes::object_fit(self.object_fit);
        video.position = nodes::object_position(self.object_position);
        Ok(ElementExport {
            opacity: channel(&node.opacity),
            body: NodeBody::HtmlVideo(director_scene::HtmlVideoBody {
                video,
                looped: self.looped,
                duration: self.duration,
            }),
        })
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn layout_style(&self) -> taffy::Style {
        taffy::Style::default()
    }
    fn animate_property(&mut self, _: &str, _: f32, _: f32, _: f64, _: &str) -> Result<(), String> {
        Err("captured timelines are immutable".into())
    }
    fn update(&mut self, time: f64) -> bool {
        let mut node = self.node.lock().unwrap();
        node.object_fit = self.object_fit;
        node.object_position = self.object_position;
        let seek = if self.looped {
            looped_media_time(time, self.duration)
        } else {
            time
        };
        node.update(seek)
    }
    fn render(
        &self,
        canvas: &skia_safe::Canvas,
        rect: skia_safe::Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&skia_safe::Canvas),
    ) -> Result<(), RenderError> {
        let mut node = self.node.lock().unwrap();
        node.object_fit = self.object_fit;
        node.object_position = self.object_position;
        node.render(canvas, rect, opacity, draw_children)
    }
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
    fn export_body(&self, cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
        // Equal samples share one graph, so the document stores each graph once.
        let mut graphs: Vec<(&Arc<Mutex<SceneGraph>>, NodeId)> = Vec::new();
        let mut body = director_scene::HtmlCaptureBody {
            sample_rate: self.sample_rate,
            graphs: Vec::new(),
            frames: Vec::with_capacity(self.frames.len()),
        };
        for (graph, root) in self.frames.iter() {
            let index = match graphs
                .iter()
                .position(|(seen, seen_root)| Arc::ptr_eq(seen, graph) && seen_root == root)
            {
                Some(index) => index,
                None => {
                    let scene = graph
                        .lock()
                        .map_err(|_| ExportError::Unrepresentable("poisoned capture graph".to_string()))?;
                    body.graphs.push(director_scene::html::CaptureGraph {
                        nodes: export_graph(&scene, cx)?,
                        root: node_ref(*root),
                        // Sampled rects stand in for layout, so they travel with the graph.
                        rects: scene
                            .iter_alive()
                            .map(|(_, node)| {
                                let r = node.layout_rect;
                                [r.left, r.top, r.width(), r.height()]
                            })
                            .collect(),
                    });
                    graphs.push((graph, *root));
                    graphs.len() - 1
                }
            };
            body.frames.push(index);
        }
        Ok(ElementExport {
            opacity: director_scene::Channel::still(1.0),
            body: NodeBody::HtmlCapture(body),
        })
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
    fn set_layout_style(&mut self, style: taffy::Style) {
        self.style = style;
    }
    fn update(&mut self, time: f64) -> bool {
        let index =
            ((time.max(0.0) * self.sample_rate + 1e-7).floor() as usize).min(self.frames.len() - 1);
        self.index = index;
        // The sampled scene is not a Director child, so its video nodes seek here.
        let mut scene = self.frames[index].0.lock().unwrap();
        let mut changed = false;
        for (_, node) in scene.iter_alive_mut() {
            changed |= node.element.update(time);
        }
        changed
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

/// Rebuild an `html_video` element with a decoder of its own.
pub(crate) fn decode_video(record: &NodeRecord, cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::HtmlVideo(body) = &record.body else {
        return Err(crate::import::mismatch(record));
    };
    let node = import_nodes::video_node(&body.video, &record.opacity, cx)?;
    // Style's raw pointer keeps VideoNode `!Send`, as in `build`.
    #[allow(clippy::arc_with_non_send_sync)]
    let node = Arc::new(Mutex::new(node));
    Ok(shared_video(body, node))
}

/// Rebuild the `html_video` content of a box. Boxes that play the same video
/// with the same `timing` seek it identically, so they share one decoder, as
/// equal capture samples do after compile.
pub(crate) fn decode_box_video(
    body: &director_scene::HtmlVideoBody,
    opacity: &director_scene::Channel,
    timing: String,
    cx: &mut ImportContext,
) -> Result<Box<dyn Element>, ImportError> {
    let key = serde_json::to_string(&(&body.video.asset, &body.video.border_radius, body.looped, body.duration, opacity))
        .map_err(|error| ImportError::Invalid(error.to_string()))?;
    let node = cx.shared(format!("html_video:{timing}:{key}"), |cx| {
        let node = import_nodes::video_node(&body.video, opacity, cx)?;
        #[allow(clippy::arc_with_non_send_sync)]
        let node = Arc::new(Mutex::new(node));
        Ok(node)
    })?;
    Ok(shared_video(body, node))
}

fn shared_video(body: &director_scene::HtmlVideoBody, node: Arc<Mutex<VideoNode>>) -> Box<dyn Element> {
    Box::new(SharedVideo {
        node,
        object_fit: import_nodes::object_fit(body.video.fit),
        object_position: import_nodes::object_position(body.video.position),
        looped: body.looped,
        duration: body.duration,
    })
}

/// Rebuild an `html_capture` element. Equal sampled graphs stay shared and
/// each node gets its sampled rectangle back.
pub(crate) fn decode_capture(record: &NodeRecord, cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    let NodeBody::HtmlCapture(body) = &record.body else {
        return Err(crate::import::mismatch(record));
    };
    if !body.sample_rate.is_finite() || body.sample_rate <= 0.0 || body.frames.is_empty() {
        return Err(ImportError::Invalid(
            "capture needs frames and a positive sample rate".to_string(),
        ));
    }
    let mut graphs = Vec::with_capacity(body.graphs.len());
    for graph in &body.graphs {
        if graph.rects.len() != graph.nodes.len() {
            return Err(ImportError::Invalid(format!(
                "capture graph has {} nodes and {} rects",
                graph.nodes.len(),
                graph.rects.len()
            )));
        }
        let mut scene = SceneGraph::new();
        let ids = import_graph(&graph.nodes, &mut scene, cx)?;
        for (record, rect) in graph.nodes.iter().zip(&graph.rects) {
            let node = scene
                .get_node_mut(resolve(&ids, record.id)?)
                .ok_or_else(|| ImportError::Invalid("imported capture node vanished".to_string()))?;
            node.layout_rect = skia_safe::Rect::from_xywh(rect[0], rect[1], rect[2], rect[3]);
            node.element.post_layout(skia_safe::Rect::from_wh(rect[2], rect[3]));
        }
        let root = resolve(&ids, graph.root)?;
        graphs.push((Arc::new(Mutex::new(scene)), root));
    }
    let frames = body
        .frames
        .iter()
        .map(|&index| {
            graphs
                .get(index)
                .cloned()
                .ok_or_else(|| ImportError::Invalid(format!("capture frame names missing graph {index}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut element = CapturedElement {
        frames: Arc::new(frames),
        sample_rate: body.sample_rate,
        index: 0,
        style: taffy::Style::default(),
        assets: cx.assets.clone(),
        cache: Mutex::new(HashMap::new()),
    };
    element.update(0.0);
    Ok(Box::new(element))
}
