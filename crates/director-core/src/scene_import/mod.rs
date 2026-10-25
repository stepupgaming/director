//! Build a live [`Director`] from a [`director_scene::SceneDocument`].
//!
//! This is the inverse of [`crate::scene_export`]. An [`ImportRegistry`] maps a
//! node record `kind` to a decoder that rebuilds its element. Core registers
//! its own kinds; `director-html` registers the HTML kinds. A kind with no
//! decoder is an [`ImportError::Unsupported`].

mod caption;
mod effect;
mod layout;
pub mod nodes;
mod timeline;

pub use caption::{caption_node, smart_caption_node};
pub use effect::effect_type;
pub use layout::{import_layout, length};

use crate::animation::{Animated, EasingType};
use crate::director::{Director, DirectorContext};
use crate::element::Element;
use crate::systems::assets::AssetManager;
use crate::types::{Color, FilterConfig, GradientConfig, GradientType, NodeId, PathAnimationState, PathMorphState};
use crate::video_wrapper::RenderMode;
use crate::AssetLoader;
use base64::Engine as _;
use director_scene::{
    AssetSource, Channel, Easing, Gradient, GradientKind, NodeBody, NodeRecord, NodeRef, Rgba, SceneDocument,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Why a document cannot become a `Director`.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// A node failed. `source` says why.
    #[error("node {index}:{generation} ({kind}): {source}")]
    Node {
        index: u32,
        generation: u32,
        kind: String,
        #[source]
        source: Box<ImportError>,
    },
    /// No decoder is registered for this kind or feature.
    #[error("`{kind}` has no scene import yet")]
    Unsupported { kind: String },
    /// The document contradicts itself or holds a value the engine rejects.
    #[error("{0}")]
    Invalid(String),
    /// An asset could not be resolved.
    #[error("asset {id}: {reason}")]
    Asset { id: String, reason: String },
}

impl ImportError {
    /// Whether this error, or the node error wrapping it, names a missing decoder.
    pub fn is_unsupported(&self) -> bool {
        match self {
            ImportError::Unsupported { .. } => true,
            ImportError::Node { source, .. } => source.is_unsupported(),
            _ => false,
        }
    }
}

/// Rebuilds one element from its node record.
pub type Decoder = fn(&NodeRecord, &mut ImportContext) -> Result<Box<dyn Element>, ImportError>;

/// Decoders by node record `kind`.
#[derive(Clone, Default)]
pub struct ImportRegistry {
    decoders: HashMap<String, Decoder>,
}

impl ImportRegistry {
    /// A registry with every core kind that has a decoder.
    pub fn core() -> Self {
        let mut registry = Self::default();
        registry.register("box", nodes::decode_box);
        registry.register("text", nodes::decode_text);
        registry.register("image", nodes::decode_image);
        registry.register("video", nodes::decode_video);
        registry.register("vector", nodes::decode_vector);
        registry.register("shape", nodes::decode_shape);
        registry.register("composition", nodes::decode_composition);
        registry.register("effect", effect::decode_effect);
        registry.register("caption", caption::decode_caption);
        registry.register("smart_caption", caption::decode_smart_caption);
        registry
    }

    /// Add or replace the decoder for `kind`.
    pub fn register(&mut self, kind: &str, decoder: Decoder) {
        self.decoders.insert(kind.to_string(), decoder);
    }

    /// The decoder for `kind`.
    pub fn decoder(&self, kind: &str) -> Option<Decoder> {
        self.decoders.get(kind).copied()
    }
}

/// What a decoder may read: the document's assets, the font collection and
/// the registry for nested documents.
pub struct ImportContext<'a> {
    pub document: &'a SceneDocument,
    pub registry: &'a ImportRegistry,
    pub assets: AssetManager,
    pub render_mode: RenderMode,
    shared: HashMap<String, Box<dyn std::any::Any>>,
}

impl ImportContext<'_> {
    /// The value stored under `key` during this import, made by `make` on
    /// first use. Decoders use it to share one resource, such as a video
    /// decoder, between equal elements.
    pub fn shared<T: Clone + 'static>(
        &mut self,
        key: String,
        make: impl FnOnce(&mut Self) -> Result<T, ImportError>,
    ) -> Result<T, ImportError> {
        if let Some(value) = self.shared.get(&key).and_then(|value| value.downcast_ref::<T>()) {
            return Ok(value.clone());
        }
        let value = make(self)?;
        self.shared.insert(key, Box::new(value.clone()));
        Ok(value)
    }

    /// The bytes of asset `id`: decoded base64, or the file through the loader.
    pub fn asset_bytes(&self, id: &str) -> Result<Vec<u8>, ImportError> {
        let record = self.document.asset(id).ok_or_else(|| ImportError::Asset {
            id: id.to_string(),
            reason: "missing from the asset table".to_string(),
        })?;
        match &record.source {
            AssetSource::Path { path } => self.assets.loader.load_bytes(path).map_err(|error| ImportError::Asset {
                id: id.to_string(),
                reason: format!("{path}: {error}"),
            }),
            AssetSource::Embedded { base64 } => base64::engine::general_purpose::STANDARD
                .decode(base64)
                .map_err(|error| ImportError::Asset {
                    id: id.to_string(),
                    reason: format!("base64: {error}"),
                }),
        }
    }

    /// The loader path of asset `id`, when it is a file.
    pub fn asset_path(&self, id: &str) -> Option<&str> {
        match &self.document.asset(id)?.source {
            AssetSource::Path { path } => Some(path),
            AssetSource::Embedded { .. } => None,
        }
    }

    /// Decode a body that lives inside another element, such as the content
    /// of an HTML box, through the decoder registered for its kind.
    pub fn element(&mut self, body: &NodeBody, opacity: &Channel) -> Result<Box<dyn Element>, ImportError> {
        let kind = body.kind();
        let decoder = self.registry.decoder(kind).ok_or_else(|| ImportError::Unsupported {
            kind: kind.to_string(),
        })?;
        let record = NodeRecord {
            id: NodeRef { index: 0, generation: 0 },
            parent: None,
            children: Vec::new(),
            kind: kind.to_string(),
            z_index: 0,
            blend_mode: "src_over".to_string(),
            is_static: false,
            layout: Default::default(),
            transform: Default::default(),
            opacity: opacity.clone(),
            mask: None,
            filter: Default::default(),
            path_animation: None,
            path_morph: None,
            audio_bindings: Vec::new(),
            entity_refs: Vec::new(),
            body: body.clone(),
        };
        decoder(&record, self)
    }

    /// Import a nested document into a director that shares these fonts.
    pub fn nested(&self, document: &SceneDocument) -> Result<Director, ImportError> {
        import_into(
            document,
            self.assets.loader.clone(),
            self.registry,
            self.render_mode,
            Some(DirectorContext {
                assets: self.assets.clone(),
            }),
        )
    }
}

/// Build a `Director` that renders `document`.
pub fn import_scene(
    document: &SceneDocument,
    loader: Arc<dyn AssetLoader>,
    registry: &ImportRegistry,
    render_mode: RenderMode,
) -> Result<Director, ImportError> {
    import_into(document, loader, registry, render_mode, None)
}

fn import_into(
    document: &SceneDocument,
    loader: Arc<dyn AssetLoader>,
    registry: &ImportRegistry,
    render_mode: RenderMode,
    context: Option<DirectorContext>,
) -> Result<Director, ImportError> {
    if !document.audio.tracks.is_empty() {
        return Err(ImportError::Unsupported {
            kind: "audio".to_string(),
        });
    }
    let mut director = Director::new(document.width, document.height, document.fps, loader, render_mode, context);
    director.output_scale_x = document.output_scale_x;
    director.output_scale_y = document.output_scale_y;
    director.transparent_background = document.transparent_background;
    director.samples_per_frame = document.motion_blur.samples_per_frame;
    director.shutter_angle = document.motion_blur.shutter_angle;

    let mut cx = ImportContext {
        document,
        registry,
        assets: director.assets.clone(),
        render_mode,
        shared: HashMap::new(),
    };
    let ids = import_graph(&document.nodes, &mut director.scene, &mut cx)?;
    timeline::import_timeline(&mut director, &document.timeline, &ids)?;
    Ok(director)
}

/// Document node ids mapped to the scene ids they imported as.
pub type NodeIds = HashMap<(u32, u32), NodeId>;

/// Add `records` to `scene` with their node state and parent links. Frontends
/// use this for graphs nested inside an element, such as sampled captures.
pub fn import_graph(
    records: &[NodeRecord],
    scene: &mut crate::scene::SceneGraph,
    cx: &mut ImportContext,
) -> Result<NodeIds, ImportError> {
    let mut ids = HashMap::new();
    for record in records {
        let element = import_element(record, cx).map_err(|source| node_error(record, source))?;
        ids.insert(key(record.id), scene.add_node(element));
    }
    for record in records {
        let id = ids[&key(record.id)];
        let state = node_state(record, &ids).map_err(|source| node_error(record, source))?;
        let node = scene
            .get_node_mut(id)
            .ok_or_else(|| ImportError::Invalid("imported node vanished".to_string()))?;
        state.apply(node);
    }
    for record in records {
        let parent = ids[&key(record.id)];
        for child in &record.children {
            let child = resolve(&ids, *child)?;
            if !scene.try_add_child(parent, child) {
                return Err(node_error(
                    record,
                    ImportError::Invalid(format!("child {}:{} is not attachable", child.index, child.generation)),
                ));
            }
        }
    }
    Ok(ids)
}

fn import_element(record: &NodeRecord, cx: &mut ImportContext) -> Result<Box<dyn Element>, ImportError> {
    if !record.audio_bindings.is_empty() {
        return Err(ImportError::Unsupported {
            kind: "audio_bindings".to_string(),
        });
    }
    if !record.entity_refs.is_empty() {
        return Err(ImportError::Unsupported {
            kind: "entity_refs".to_string(),
        });
    }
    let decoder = cx.registry.decoder(&record.kind).ok_or_else(|| ImportError::Unsupported {
        kind: record.kind.clone(),
    })?;
    let mut element = decoder(record, cx)?;
    element.set_layout_style(import_layout(&record.layout)?);
    Ok(element)
}

fn node_error(record: &NodeRecord, source: ImportError) -> ImportError {
    ImportError::Node {
        index: record.id.index,
        generation: record.id.generation,
        kind: record.kind.clone(),
        source: Box::new(source),
    }
}

/// Scene-graph fields a node record sets beside its element.
struct NodeState {
    transform: crate::types::Transform,
    z_index: i32,
    blend_mode: skia_safe::BlendMode,
    is_static: bool,
    mask: Option<NodeId>,
    filter: FilterConfig,
    path_animation: Option<PathAnimationState>,
    path_morph: Option<PathMorphState>,
}

impl NodeState {
    fn apply(self, node: &mut crate::scene::SceneNode) {
        node.transform = self.transform;
        node.z_index = self.z_index;
        node.blend_mode = self.blend_mode;
        node.is_static = self.is_static;
        node.mask_node = self.mask;
        node.filter = self.filter;
        node.path_animation = self.path_animation;
        node.path_morph = self.path_morph;
    }
}

fn node_state(record: &NodeRecord, ids: &HashMap<(u32, u32), NodeId>) -> Result<NodeState, ImportError> {
    let t = &record.transform;
    let mut transform = crate::types::Transform::new();
    transform.translate_x = animated(&t.translate_x)?;
    transform.translate_y = animated(&t.translate_y)?;
    transform.scale_x = animated(&t.scale_x)?;
    transform.scale_y = animated(&t.scale_y)?;
    transform.rotation = animated(&t.rotation)?;
    transform.skew_x = animated(&t.skew_x)?;
    transform.skew_y = animated(&t.skew_y)?;
    transform.pivot_x = t.pivot_x;
    transform.pivot_y = t.pivot_y;
    let f = &record.filter;
    Ok(NodeState {
        transform,
        z_index: record.z_index,
        blend_mode: blend_mode(&record.blend_mode)?,
        is_static: record.is_static,
        mask: record.mask.map(|mask| resolve(ids, mask)).transpose()?,
        filter: FilterConfig {
            blur: f.blur,
            brightness: f.brightness,
            contrast: f.contrast,
            saturation: f.saturation,
            hue_rotate: f.hue_rotate,
            invert: f.invert,
            sepia: f.sepia,
            opacity: f.opacity,
        },
        path_animation: record
            .path_animation
            .as_ref()
            .map(|path| -> Result<_, ImportError> {
                let mut state = PathAnimationState::new(svg_path(&path.path)?, path.follow_tangent, path.duration);
                state.progress = animated(&path.progress)?;
                state.speed = path.speed;
                state.loop_count = path.loop_count;
                Ok(state)
            })
            .transpose()?,
        path_morph: record
            .path_morph
            .as_ref()
            .map(|morph| -> Result<_, ImportError> {
                let mut state = PathMorphState::new(
                    svg_path(&morph.from_path)?,
                    svg_path(&morph.to_path)?,
                    morph.follow_tangent,
                    morph.duration,
                );
                state.progress = animated(&morph.progress)?;
                state.speed = morph.speed;
                state.loop_count = morph.loop_count;
                Ok(state)
            })
            .transpose()?,
    })
}

fn svg_path(svg: &str) -> Result<skia_safe::Path, ImportError> {
    skia_safe::Path::from_svg(svg).ok_or_else(|| ImportError::Invalid(format!("SVG path data `{svg}`")))
}

fn key(id: NodeRef) -> (u32, u32) {
    (id.index, id.generation)
}

/// The imported id of a document node.
pub fn resolve(ids: &NodeIds, id: NodeRef) -> Result<NodeId, ImportError> {
    ids.get(&key(id))
        .copied()
        .ok_or_else(|| ImportError::Invalid(format!("node {}:{} is not in the document", id.index, id.generation)))
}

/// An `f32` animation from its channel.
pub fn animated(channel: &Channel) -> Result<Animated<f32>, ImportError> {
    animated_with(channel, |value| *value)
}

/// A color animation from its channel.
pub fn color_animated(channel: &Channel<Rgba>) -> Result<Animated<Color>, ImportError> {
    animated_with(channel, |value| color(*value))
}

/// An animation of any tweenable value through `convert`.
pub fn animated_with<T, U>(channel: &Channel<U>, convert: impl Fn(&U) -> T) -> Result<Animated<T>, ImportError>
where
    T: Clone + keyframe::CanTween + Default,
{
    let first = channel
        .keyframes
        .first()
        .ok_or_else(|| ImportError::Invalid("a channel needs at least one keyframe".to_string()))?;
    let mut animation = Animated::new(convert(&first.value));
    animation.raw_keyframes = channel
        .keyframes
        .iter()
        .map(|keyframe| (convert(&keyframe.value), keyframe.time, easing(&keyframe.easing)))
        .collect();
    animation.sequence = keyframe::AnimationSequence::from(
        animation
            .raw_keyframes
            .iter()
            .map(|(value, time, easing)| keyframe::Keyframe::new(value.clone(), *time, easing.clone()))
            .collect::<Vec<_>>(),
    );
    Ok(animation)
}

pub fn easing(easing: &Easing) -> EasingType {
    match easing {
        Easing::Linear => EasingType::Linear,
        Easing::EaseIn => EasingType::EaseIn,
        Easing::EaseOut => EasingType::EaseOut,
        Easing::EaseInOut => EasingType::EaseInOut,
        Easing::BounceOut => EasingType::BounceOut,
        Easing::BounceIn => EasingType::BounceIn,
        Easing::BounceInOut => EasingType::BounceInOut,
        Easing::ElasticOut => EasingType::ElasticOut,
        Easing::ElasticIn => EasingType::ElasticIn,
        Easing::ElasticInOut => EasingType::ElasticInOut,
        Easing::BackOut => EasingType::BackOut,
        Easing::BackIn => EasingType::BackIn,
        Easing::BackInOut => EasingType::BackInOut,
        Easing::Hold => EasingType::Hold,
        Easing::CubicBezier { x1, y1, x2, y2 } => EasingType::CubicBezier {
            x1: *x1,
            y1: *y1,
            x2: *x2,
            y2: *y2,
        },
        Easing::Expression { expr } => EasingType::Expression { expr: expr.clone() },
    }
}

pub fn color(value: Rgba) -> Color {
    Color {
        r: value.r,
        g: value.g,
        b: value.b,
        a: value.a,
    }
}

pub fn gradient(value: &Gradient) -> GradientConfig {
    GradientConfig {
        colors: value.colors.iter().copied().map(color).collect(),
        positions: value.positions.clone(),
        start: (value.start[0], value.start[1]),
        end: (value.end[0], value.end[1]),
        gradient_type: match value.kind {
            GradientKind::Linear => GradientType::Linear,
            GradientKind::Radial => GradientType::Radial,
            GradientKind::Conic => GradientType::Conic,
        },
        center: (value.center[0], value.center[1]),
        radius: value.radius,
        angle: value.angle,
    }
}

/// The blend mode a node record names.
pub fn blend_mode(name: &str) -> Result<skia_safe::BlendMode, ImportError> {
    use skia_safe::BlendMode as B;
    Ok(match name {
        "src_over" => B::SrcOver,
        "multiply" => B::Multiply,
        "screen" => B::Screen,
        "overlay" => B::Overlay,
        "darken" => B::Darken,
        "lighten" => B::Lighten,
        "color_dodge" => B::ColorDodge,
        "color_burn" => B::ColorBurn,
        "hard_light" => B::HardLight,
        "soft_light" => B::SoftLight,
        "difference" => B::Difference,
        "exclusion" => B::Exclusion,
        "hue" => B::Hue,
        "saturation" => B::Saturation,
        "color" => B::Color,
        "luminosity" => B::Luminosity,
        "plus" => B::Plus,
        "clear" => B::Clear,
        "src" => B::Src,
        "dst" => B::Dst,
        "dst_over" => B::DstOver,
        "src_in" => B::SrcIn,
        "dst_in" => B::DstIn,
        "src_out" => B::SrcOut,
        "dst_out" => B::DstOut,
        "src_atop" => B::SrcATop,
        "dst_atop" => B::DstATop,
        "xor" => B::Xor,
        "modulate" => B::Modulate,
        other => return Err(ImportError::Invalid(format!("unknown blend mode `{other}`"))),
    })
}
