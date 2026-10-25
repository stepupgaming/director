//! Per-kind paint knowledge: which element kinds vary with time, and the
//! kinds this crate paints itself. Every other kind paints through its own
//! `Element::render`.

use super::{LayerCache, Painter};
use director_core::animation::{Animated, EasingType};
use director_core::element::Element;
use director_core::node::{BoxNode, CompositionNode, ImageNode, ShapeNode, SpectrumVisualizerNode, TextNode, VectorNode};
use director_core::scene::SceneNode;
use director_core::types::Transform;
use director_core::RenderError;
use skia_safe::{AlphaType, Canvas, Color, ColorSpace, ColorType, ImageInfo, Paint, Rect, Surface};
use std::sync::MutexGuard;

/// Kinds whose paint follows the clock whatever their channels say.
const CLOCKED_KINDS: &[&str] = &[
    "video",
    "lottie",
    "caption",
    "smart_caption",
    "active_speaker_camera",
    "composition",
    "effect",
    "html_capture",
];

fn channel_varies<T>(anim: &Animated<T>) -> bool
where
    T: Clone + keyframe::CanTween + Default,
{
    anim.raw_keyframes.len() > 1
        || anim.raw_keyframes.iter().any(|(_, _, ease)| matches!(ease, EasingType::Expression { .. }))
}

fn optional_channel_varies<T>(anim: &Option<Animated<T>>) -> bool
where
    T: Clone + keyframe::CanTween + Default,
{
    anim.as_ref().is_some_and(channel_varies)
}

fn transform_varies(transform: &Transform) -> bool {
    [
        &transform.scale_x,
        &transform.scale_y,
        &transform.rotation,
        &transform.skew_x,
        &transform.skew_y,
        &transform.translate_x,
        &transform.translate_y,
    ]
    .into_iter()
    .any(channel_varies)
}

/// Animated paint channels of the core leaf kinds.
fn element_varies(element: &dyn Element) -> bool {
    let any = element.as_any();
    if let Some(node) = any.downcast_ref::<BoxNode>() {
        return optional_channel_varies(&node.bg_color)
            || channel_varies(&node.opacity)
            || channel_varies(&node.blur)
            || optional_channel_varies(&node.shadow_color)
            || channel_varies(&node.shadow_blur)
            || channel_varies(&node.shadow_offset_x)
            || channel_varies(&node.shadow_offset_y)
            || channel_varies(&node.border_radius)
            || channel_varies(&node.border_width)
            || optional_channel_varies(&node.border_color)
            || channel_varies(&node.backdrop_blur)
            || node.extra_shadows.iter().any(|shadow| {
                channel_varies(&shadow.color)
                    || channel_varies(&shadow.blur)
                    || channel_varies(&shadow.offset_x)
                    || channel_varies(&shadow.offset_y)
            });
    }
    if let Some(node) = any.downcast_ref::<TextNode>() {
        return channel_varies(&node.default_font_size)
            || channel_varies(&node.default_color)
            || channel_varies(&node.opacity)
            || channel_varies(&node.counter_value)
            || node.animators.iter().any(|animator| channel_varies(&animator.animation));
    }
    if let Some(node) = any.downcast_ref::<ImageNode>() {
        return channel_varies(&node.opacity) || channel_varies(&node.border_radius);
    }
    if let Some(node) = any.downcast_ref::<ShapeNode>() {
        return optional_channel_varies(&node.fill_color)
            || optional_channel_varies(&node.stroke_color)
            || channel_varies(&node.stroke_width)
            || channel_varies(&node.opacity);
    }
    if let Some(node) = any.downcast_ref::<VectorNode>() {
        return channel_varies(&node.opacity);
    }
    any.is::<SpectrumVisualizerNode>()
}

/// True when this node's own paint may differ between two times.
pub(super) fn node_varies(node: &SceneNode) -> bool {
    node.element.varies_with_time()
        || CLOCKED_KINDS.contains(&node.element.kind())
        || !node.audio_bindings.is_empty()
        || transform_varies(&node.transform)
        || node.path_animation.as_ref().is_some_and(|path| channel_varies(&path.progress))
        || node.path_morph.as_ref().is_some_and(|morph| channel_varies(&morph.progress))
        || element_varies(node.element.as_ref())
}

/// State a composition keeps across frames: the layer cache of its nested
/// scene and the surface that scene paints into.
#[derive(Default)]
pub(crate) struct Nested {
    cache: LayerCache,
    surface: Option<Surface>,
}

fn lock<T>(mutex: &std::sync::Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Paint `node`'s element. Compositions paint their nested timeline through
/// this crate's traversal; `nested` holds their state.
pub(super) fn paint_element(
    node: &SceneNode,
    nested: Option<&mut Nested>,
    canvas: &Canvas,
    rect: Rect,
    opacity: f32,
    draw_children: &mut dyn FnMut(&Canvas),
) -> Result<(), RenderError> {
    match (node.element.as_any().downcast_ref::<CompositionNode>(), nested) {
        (Some(composition), Some(nested)) => {
            paint_composition(composition, nested, canvas, rect, opacity)?;
            draw_children(canvas);
            Ok(())
        }
        _ => node.element.render(canvas, rect, opacity, draw_children),
    }
}

/// True when [`paint_element`] needs [`Nested`] state for this node.
pub(super) fn has_nested(node: &SceneNode) -> bool {
    node.element.as_any().is::<CompositionNode>()
}

/// Paint the active clips of a composition's timeline at its own size, then
/// stretch that frame over `rect`.
fn paint_composition(
    composition: &CompositionNode,
    nested: &mut Nested,
    canvas: &Canvas,
    rect: Rect,
    opacity: f32,
) -> Result<(), RenderError> {
    let director = lock(&composition.internal_director);
    let (width, height) = (director.width, director.height);
    if nested.surface.as_ref().is_none_or(|surface| surface.width() != width || surface.height() != height) {
        let info = ImageInfo::new(
            (width, height),
            ColorType::RGBA8888,
            AlphaType::Premul,
            Some(ColorSpace::new_srgb()),
        );
        nested.surface = canvas.new_surface(&info, None).or_else(|| skia_safe::surfaces::raster(&info, None, None));
    }
    let Some(surface) = nested.surface.as_mut() else {
        return Ok(());
    };
    let inner = surface.canvas();
    inner.clear(Color::TRANSPARENT);
    // `CompositionNode::update` advanced every inner node to the composition's time.
    let time = director.scene.nodes.iter().flatten().next().map_or(0.0, |node| node.last_visit_time);
    let mut clips: Vec<_> = director
        .tracks
        .iter()
        .filter(|track| track.enabled)
        .flat_map(|track| {
            track
                .clips
                .iter()
                .filter(|clip| time >= clip.start_time && time < clip.start_time + clip.duration)
                .map(move |clip| (track.opacity, clip))
        })
        .collect();
    clips.sort_by_key(|(_, clip)| clip.z_index);
    let painter = Painter { scene: &director.scene, dirty: None };
    for (track_opacity, clip) in clips {
        painter.root(&mut nested.cache, clip.scene_root, inner, track_opacity * clip.fade_opacity(time))?;
    }
    let image = surface.image_snapshot();
    let mut paint = Paint::default();
    paint.set_alpha_f(opacity);
    canvas.draw_image_rect(&image, None, rect, &paint);
    Ok(())
}
