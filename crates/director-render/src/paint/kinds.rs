//! Per-kind paint knowledge: which element kinds vary with time, and the
//! kinds this crate paints itself. Every other kind paints through its own
//! `Element::render`.

use super::{LayerCache, Painter};
use director_core::element::Element;
use director_core::node::{BoxNode, CompositionNode, ImageNode, ShapeNode, SpectrumVisualizerNode, TextNode, VectorNode};
use director_core::scene::SceneNode;
use director_core::types::Transform;
use director_core::RenderError;
use skia_safe::{AlphaType, Canvas, Color, ColorSpace, ColorType, ImageInfo, Paint, Rect};
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

/// The whole timeline, for questions about any time at all.
const ALL_TIME: (f64, f64) = (f64::NEG_INFINITY, f64::INFINITY);

fn transform_moves(transform: &Transform, (t0, t1): (f64, f64)) -> bool {
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
    .any(|channel| channel.moves_between(t0, t1))
}

/// Path animations and morphs loop their own clock, so any motion counts.
fn path_moves(node: &SceneNode) -> bool {
    node.path_animation.as_ref().is_some_and(|path| path.progress.moves_between(ALL_TIME.0, ALL_TIME.1))
        || node.path_morph.as_ref().is_some_and(|morph| morph.progress.moves_between(ALL_TIME.0, ALL_TIME.1))
}

/// Animated paint channels of the core leaf kinds, which list their own
/// channels in `Element::moves_between`.
fn element_varies(element: &dyn Element) -> bool {
    let any = element.as_any();
    if any.is::<BoxNode>() || any.is::<TextNode>() || any.is::<ImageNode>() || any.is::<ShapeNode>() || any.is::<VectorNode>() {
        return element.moves_between(ALL_TIME.0, ALL_TIME.1);
    }
    any.is::<SpectrumVisualizerNode>()
}

/// True when this node's own paint may differ between two times.
pub(super) fn node_varies(node: &SceneNode) -> bool {
    node.element.varies_with_time()
        || CLOCKED_KINDS.contains(&node.element.kind())
        || !node.audio_bindings.is_empty()
        || transform_moves(&node.transform, ALL_TIME)
        || path_moves(node)
        || element_varies(node.element.as_ref())
}

/// Local pixels this node's paint may move inside its box over local time
/// `[t0, t1]` beyond what the box and its transforms show. Clocked kinds,
/// audio-driven nodes and path animations answer `None`.
pub(super) fn node_paint_travel(node: &SceneNode, t0: f64, t1: f64) -> Option<f32> {
    if CLOCKED_KINDS.contains(&node.element.kind()) || !node.audio_bindings.is_empty() || path_moves(node) {
        return None;
    }
    node.element.paint_travel(t0, t1, (node.layout_rect.width(), node.layout_rect.height()))
}

/// True when this node's own paint may differ somewhere in local time
/// `[t0, t1]`. Kinds this crate does not know answer through
/// `Element::moves_between`, which assumes motion by default.
pub(super) fn node_moves(node: &SceneNode, t0: f64, t1: f64) -> bool {
    CLOCKED_KINDS.contains(&node.element.kind())
        || !node.audio_bindings.is_empty()
        || transform_moves(&node.transform, (t0, t1))
        || path_moves(node)
        || node.element.moves_between(t0, t1)
}

fn lock<T>(mutex: &std::sync::Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Paint `node`'s element. Compositions paint their nested timeline and
/// sampled captures paint their sampled graph through this crate's traversal.
pub(super) fn paint_element(
    node: &SceneNode,
    canvas: &Canvas,
    rect: Rect,
    opacity: f32,
    draw_children: &mut dyn FnMut(&Canvas),
) -> Result<(), RenderError> {
    if let Some(composition) = node.element.as_any().downcast_ref::<CompositionNode>() {
        paint_composition(composition, canvas, rect, opacity)?;
        draw_children(canvas);
        return Ok(());
    }
    if let Some((scene, root)) = node.element.sampled_graph() {
        // A sampled graph changes every sample, so its layers never outlive the frame.
        return Painter { scene: &scene, dirty: None }.root(&mut LayerCache::new(), root, canvas, opacity);
    }
    node.element.render(canvas, rect, opacity, draw_children)
}

/// Paint the active clips of a composition's timeline at its own size, then
/// stretch that frame over `rect`. The nested `Director` keeps the static
/// layers and the composition keeps the surface across frames.
fn paint_composition(composition: &CompositionNode, canvas: &Canvas, rect: Rect, opacity: f32) -> Result<(), RenderError> {
    let mut guard = lock(&composition.internal_director);
    let director = &mut *guard;
    let (width, height) = (director.width, director.height);
    let mut surface = lock(&composition.surface_cache);
    if surface.as_ref().is_none_or(|surface| surface.width() != width || surface.height() != height) {
        let info = ImageInfo::new(
            (width, height),
            ColorType::RGBA8888,
            AlphaType::Premul,
            Some(ColorSpace::new_srgb()),
        );
        *surface = canvas.new_surface(&info, None).or_else(|| skia_safe::surfaces::raster(&info, None, None));
    }
    let Some(surface) = surface.as_mut() else {
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
        painter.root(&mut director.static_cache, clip.scene_root, inner, track_opacity * clip.fade_opacity(time))?;
    }
    let image = surface.image_snapshot();
    let mut paint = Paint::default();
    paint.set_alpha_f(opacity);
    canvas.draw_image_rect(&image, None, rect, &paint);
    Ok(())
}
