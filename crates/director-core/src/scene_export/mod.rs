//! Export a live [`Director`] into a [`director_scene::SceneDocument`].
//!
//! The document stores channels, layout inputs, kind-specific values and an
//! asset table. It does not store Skia handles, layout results, caches, or
//! audio-smoothing state. A value the document cannot hold is an
//! [`ExportError`].

mod audio;
pub(crate) mod caption;
pub(crate) mod effect;
mod layout;
pub mod nodes;

pub use layout::{export_layout, export_length};
mod timeline;

use crate::animation::{Animated, EasingType, TweenableVector};
use crate::director::Director;
use crate::entity::EntityRegistry;
use crate::scene::{SceneGraph, SceneNode};
use crate::types::{Color, GradientConfig, GradientType, NodeId};
use base64::Engine as _;
use director_scene::{
    AssetId, AssetRecord, AssetSource, AudioBinding, Channel, Easing, Entity, EntityKind,
    EntitySample, Filter, FontFace, Gradient, GradientKind, Keyframe, MotionBlur, NodeBody, NodeRecord,
    NodeRef, PathAnimation, PathMorph, Rgba, SceneDocument, TransformChannels, VERSION,
};
use std::collections::HashMap;

/// Why a scene cannot become a document.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// A node failed. `source` says why.
    #[error("node {index}:{generation} ({kind}): {source}")]
    Node {
        index: u32,
        generation: u32,
        kind: &'static str,
        #[source]
        source: Box<ExportError>,
    },
    /// The element kind has no document form.
    #[error("`{kind}` elements have no scene document form")]
    Unsupported { kind: &'static str },
    /// A value the document cannot hold.
    #[error("{0}")]
    Unrepresentable(String),
    /// A media file could not be read for embedding.
    #[error("asset {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// What an element contributes to its node record.
pub struct ElementExport {
    /// The element's own opacity channel.
    pub opacity: Channel,
    pub body: NodeBody,
}

/// Shared state for one export: the asset table.
#[derive(Default)]
pub struct ExportContext {
    assets: Vec<AssetRecord>,
    by_path: HashMap<String, AssetId>,
}

impl ExportContext {
    /// Register a file by path. The same path returns the same id.
    pub fn path_asset(&mut self, prefix: &str, media_type: &str, path: &str) -> AssetId {
        if let Some(id) = self.by_path.get(path) {
            return id.clone();
        }
        let id = self.push(prefix, media_type, AssetSource::Path { path: path.to_string() });
        self.by_path.insert(path.to_string(), id.clone());
        id
    }

    /// Register bytes that have no file.
    pub fn embedded_asset(&mut self, prefix: &str, media_type: &str, bytes: &[u8]) -> AssetId {
        let base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        self.push(prefix, media_type, AssetSource::Embedded { base64 })
    }

    fn push(&mut self, prefix: &str, media_type: &str, source: AssetSource) -> AssetId {
        let id = format!("{prefix}-{}", self.assets.len());
        self.assets.push(AssetRecord {
            id: id.clone(),
            media_type: media_type.to_string(),
            source,
        });
        id
    }
}

/// Copy the scene graph, timeline, audio, entities and assets into a document.
pub fn export_scene(director: &Director) -> Result<SceneDocument, ExportError> {
    let mut document = export_nested(director)?;
    let faces = director
        .assets
        .font_faces
        .lock()
        .map_err(|_| ExportError::Unrepresentable("poisoned font faces".to_string()))?
        .clone();
    let mut cx = ExportContext {
        assets: std::mem::take(&mut document.assets),
        ..Default::default()
    };
    document.fonts = faces
        .iter()
        .map(|face| FontFace {
            family: face.family.clone(),
            asset: cx.embedded_asset("font", "font/ttf", &face.bytes),
        })
        .collect();
    document.assets = cx.assets;
    Ok(document)
}

/// Export a director that shares its parent's fonts, leaving `fonts` empty.
pub(crate) fn export_nested(director: &Director) -> Result<SceneDocument, ExportError> {
    let mut cx = ExportContext::default();
    let nodes = export_graph(&director.scene, &mut cx)?;
    let audio = audio::export_audio(&director.audio_mixer, &mut cx)?;
    Ok(SceneDocument {
        version: VERSION,
        width: director.width,
        height: director.height,
        fps: director.fps,
        output_scale_x: director.output_scale_x,
        output_scale_y: director.output_scale_y,
        transparent_background: director.transparent_background,
        motion_blur: MotionBlur {
            samples_per_frame: director.samples_per_frame,
            shutter_angle: director.shutter_angle,
        },
        nodes,
        timeline: timeline::export_timeline(director),
        audio,
        entities: export_entities(&director.entities),
        assets: cx.assets,
        fonts: Vec::new(),
    })
}

/// Copy every live node of `graph`, registering its media in `cx`.
pub fn export_graph(graph: &SceneGraph, cx: &mut ExportContext) -> Result<Vec<NodeRecord>, ExportError> {
    let mut nodes = Vec::new();
    for index in 0..graph.nodes.len() {
        let Some(node) = graph.nodes[index].as_ref() else {
            continue;
        };
        let id = NodeId::new(index as u32, graph.generation_at(index));
        let record = export_node(id, node, cx).map_err(|source| ExportError::Node {
            index: id.index,
            generation: id.generation,
            kind: node.element.kind(),
            source: Box::new(source),
        })?;
        nodes.push(record);
    }
    Ok(nodes)
}

fn export_node(id: NodeId, node: &SceneNode, cx: &mut ExportContext) -> Result<NodeRecord, ExportError> {
    let ElementExport { opacity, body } = node.element.export_body(cx)?;
    Ok(NodeRecord {
        id: node_ref(id),
        parent: node.parent.map(node_ref),
        children: node.children.iter().copied().map(node_ref).collect(),
        kind: node.element.kind().to_string(),
        z_index: node.z_index,
        blend_mode: blend_mode_name(node.blend_mode).to_string(),
        is_static: node.is_static,
        layout: export_layout(&node.element.export_layout_style())?,
        transform: TransformChannels {
            translate_x: channel(&node.transform.translate_x),
            translate_y: channel(&node.transform.translate_y),
            scale_x: channel(&node.transform.scale_x),
            scale_y: channel(&node.transform.scale_y),
            rotation: channel(&node.transform.rotation),
            skew_x: channel(&node.transform.skew_x),
            skew_y: channel(&node.transform.skew_y),
            pivot_x: node.transform.pivot_x,
            pivot_y: node.transform.pivot_y,
        },
        opacity,
        mask: node.mask_node.map(node_ref),
        filter: Filter {
            blur: node.filter.blur,
            brightness: node.filter.brightness,
            contrast: node.filter.contrast,
            saturation: node.filter.saturation,
            hue_rotate: node.filter.hue_rotate,
            invert: node.filter.invert,
            sepia: node.filter.sepia,
            opacity: node.filter.opacity,
        },
        path_animation: node.path_animation.as_ref().map(|state| PathAnimation {
            path: state.path.to_svg(),
            progress: channel(&state.progress),
            follow_tangent: state.follow_tangent,
            speed: state.speed,
            loop_count: state.loop_count,
            duration: state.duration,
        }),
        path_morph: node.path_morph.as_ref().map(|state| PathMorph {
            from_path: state.from_path.to_svg(),
            to_path: state.to_path.to_svg(),
            progress: channel(&state.progress),
            follow_tangent: state.follow_tangent,
            speed: state.speed,
            loop_count: state.loop_count,
            duration: state.duration,
        }),
        audio_bindings: node
            .audio_bindings
            .iter()
            .map(|binding| AudioBinding {
                track: binding.track_id,
                band: binding.band.clone(),
                property: binding.property.clone(),
                min_value: binding.min_value,
                max_value: binding.max_value,
                smoothing: binding.smoothing,
            })
            .collect(),
        entity_refs: node.entity_refs.iter().map(|id| id.to_string()).collect(),
        body,
    })
}

fn export_entities(registry: &EntityRegistry) -> Vec<Entity> {
    let mut entities: Vec<Entity> = registry
        .iter()
        .map(|track| Entity {
            id: track.id.to_string(),
            kind: match track.kind {
                crate::entity::EntityKind::Face => EntityKind::Face,
                crate::entity::EntityKind::Speaker => EntityKind::Speaker,
                crate::entity::EntityKind::Object => EntityKind::Object,
                crate::entity::EntityKind::CaptionPhrase => EntityKind::CaptionPhrase,
            },
            label: track.label.clone(),
            samples: track
                .samples
                .iter()
                .map(|sample| EntitySample {
                    time: sample.time,
                    bbox: director_scene::BoundingBox {
                        x: sample.bbox.x,
                        y: sample.bbox.y,
                        width: sample.bbox.width,
                        height: sample.bbox.height,
                    },
                    confidence: sample.confidence,
                })
                .collect(),
        })
        .collect();
    // The registry is a hash map. Sort so equal scenes write equal documents.
    entities.sort_by(|a, b| a.id.cmp(&b.id));
    entities
}

/// The node record name of a blend mode.
pub fn blend_mode_name(mode: skia_safe::BlendMode) -> &'static str {
    use skia_safe::BlendMode as B;
    match mode {
        B::SrcOver => "src_over",
        B::Multiply => "multiply",
        B::Screen => "screen",
        B::Overlay => "overlay",
        B::Darken => "darken",
        B::Lighten => "lighten",
        B::ColorDodge => "color_dodge",
        B::ColorBurn => "color_burn",
        B::HardLight => "hard_light",
        B::SoftLight => "soft_light",
        B::Difference => "difference",
        B::Exclusion => "exclusion",
        B::Hue => "hue",
        B::Saturation => "saturation",
        B::Color => "color",
        B::Luminosity => "luminosity",
        B::Plus => "plus",
        B::Clear => "clear",
        B::Src => "src",
        B::Dst => "dst",
        B::DstOver => "dst_over",
        B::SrcIn => "src_in",
        B::DstIn => "dst_in",
        B::SrcOut => "src_out",
        B::DstOut => "dst_out",
        B::SrcATop => "src_atop",
        B::DstATop => "dst_atop",
        B::Xor => "xor",
        B::Modulate => "modulate",
    }
}

/// An `f32` channel from its keyframes.
pub fn channel(animated: &Animated<f32>) -> Channel {
    channel_with(animated, |value| *value)
}

/// A color channel from its keyframes.
pub fn color_channel(animated: &Animated<Color>) -> Channel<Rgba> {
    channel_with(animated, |value| rgba(*value))
}

/// A vector channel from its keyframes.
pub(crate) fn vector_channel(animated: &Animated<TweenableVector>) -> Channel<Vec<f32>> {
    channel_with(animated, |value| value.0.clone())
}

/// A channel of any tweenable value through `convert`.
pub fn channel_with<T, U>(animated: &Animated<T>, convert: impl Fn(&T) -> U) -> Channel<U>
where
    T: Clone + keyframe::CanTween + Default,
{
    if animated.raw_keyframes.is_empty() {
        return Channel::still(convert(&animated.current_value));
    }
    Channel {
        keyframes: animated
            .raw_keyframes
            .iter()
            .map(|(value, time, easing)| Keyframe {
                time: *time,
                value: convert(value),
                easing: export_easing(easing),
            })
            .collect(),
    }
}

pub fn export_easing(easing: &EasingType) -> Easing {
    match easing {
        EasingType::Linear => Easing::Linear,
        EasingType::EaseIn => Easing::EaseIn,
        EasingType::EaseOut => Easing::EaseOut,
        EasingType::EaseInOut => Easing::EaseInOut,
        EasingType::BounceOut => Easing::BounceOut,
        EasingType::BounceIn => Easing::BounceIn,
        EasingType::BounceInOut => Easing::BounceInOut,
        EasingType::ElasticOut => Easing::ElasticOut,
        EasingType::ElasticIn => Easing::ElasticIn,
        EasingType::ElasticInOut => Easing::ElasticInOut,
        EasingType::BackOut => Easing::BackOut,
        EasingType::BackIn => Easing::BackIn,
        EasingType::BackInOut => Easing::BackInOut,
        EasingType::Hold => Easing::Hold,
        EasingType::CubicBezier { x1, y1, x2, y2 } => Easing::CubicBezier {
            x1: *x1,
            y1: *y1,
            x2: *x2,
            y2: *y2,
        },
        EasingType::Expression { expr } => Easing::Expression { expr: expr.clone() },
    }
}

pub fn rgba(color: Color) -> Rgba {
    Rgba {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

pub fn gradient(config: &GradientConfig) -> Gradient {
    Gradient {
        kind: match config.gradient_type {
            GradientType::Linear => GradientKind::Linear,
            GradientType::Radial => GradientKind::Radial,
            GradientType::Conic => GradientKind::Conic,
        },
        colors: config.colors.iter().copied().map(rgba).collect(),
        positions: config.positions.clone(),
        start: [config.start.0, config.start.1],
        end: [config.end.0, config.end.1],
        center: [config.center.0, config.center.1],
        radius: config.radius,
        angle: config.angle,
    }
}

pub fn node_ref(id: NodeId) -> NodeRef {
    NodeRef {
        index: id.index,
        generation: id.generation,
    }
}
