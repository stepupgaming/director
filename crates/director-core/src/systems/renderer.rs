//! # Renderer System
//!
//! Handles visual output via Skia.
//!
// TODO: Re-enable Rayon for parallel node updates and rendering.
//!
//! ## Responsibilities
//! - **Scene Traversal**: Recursively paints `SceneNode`s to Canvas (`render_recursive`).
//! - **Layer Composition**: Manages canvas save/restore for transforms.
//! - **Debug Rendering**: Single-frame rendering for previews (`render_frame`).
//!
//! ## Key Functions
//! - `render_recursive`: Draws a single node hierarchy.
//! - `render_frame`: Debug helper for single-frame rendering.
//!
//! ## See Also
//! - `export::video` for the full export loop (`render_export`).
//! - `systems::transitions` for transition shaders.

use crate::animation::{Animated, EasingType};
use crate::director::{Director, StaticCacheEntry, StaticCacheKey, TimelineItem};
use crate::errors::RenderError;
use crate::node::{BoxNode, ImageNode, ShapeNode, SpectrumVisualizerNode, TextNode, VectorNode};
use crate::scene::{SceneGraph, SceneNode};
use crate::systems::assets::AssetManager;
use crate::systems::layout::LayoutEngine;
use crate::systems::transitions::draw_transition;
use crate::timeline::TrackClip;
use crate::types::{NodeId, Transform};
use std::collections::{HashMap, HashSet};
use tracing::debug;

#[cfg(feature = "vulkan")]
use skia_safe::gpu::DirectContext;

#[cfg(feature = "vulkan")]
/// Type alias for the GPU context (Vulkan backend).
pub type GpuContext = DirectContext;
#[cfg(not(feature = "vulkan"))]
/// Placeholder for GPU context when Vulkan feature is disabled.
pub type GpuContext = ();

/// Static-subtree cache consulted by [`render_recursive`].
///
/// Keyed entries reuse a full pass only when the recorded bake key matches.
/// Image-only maps have no key, so a full pass always misses.
pub trait StaticSubtreeCache {
    fn lookup(
        &mut self,
        node_id: NodeId,
        key: &StaticCacheKey,
        full_pass: bool,
        is_dirty: bool,
    ) -> Option<skia_safe::Image>;

    fn store(&mut self, node_id: NodeId, image: skia_safe::Image, key: StaticCacheKey);
}

impl StaticSubtreeCache for HashMap<NodeId, StaticCacheEntry> {
    fn lookup(
        &mut self,
        node_id: NodeId,
        key: &StaticCacheKey,
        full_pass: bool,
        is_dirty: bool,
    ) -> Option<skia_safe::Image> {
        if is_dirty {
            self.remove(&node_id);
            return None;
        }
        let Some(entry) = self.get(&node_id) else {
            return None;
        };
        if !full_pass || &entry.key == key {
            return Some(entry.image.clone());
        }
        self.remove(&node_id);
        None
    }

    fn store(&mut self, node_id: NodeId, image: skia_safe::Image, key: StaticCacheKey) {
        self.insert(node_id, StaticCacheEntry { image, key });
    }
}

impl StaticSubtreeCache for HashMap<NodeId, skia_safe::Image> {
    fn lookup(
        &mut self,
        node_id: NodeId,
        _key: &StaticCacheKey,
        full_pass: bool,
        is_dirty: bool,
    ) -> Option<skia_safe::Image> {
        if is_dirty || full_pass {
            self.remove(&node_id);
            return None;
        }
        self.get(&node_id).cloned()
    }

    fn store(&mut self, node_id: NodeId, image: skia_safe::Image, _key: StaticCacheKey) {
        self.insert(node_id, image);
    }
}

impl StaticSubtreeCache for std::sync::MutexGuard<'_, HashMap<NodeId, skia_safe::Image>> {
    fn lookup(
        &mut self,
        node_id: NodeId,
        key: &StaticCacheKey,
        full_pass: bool,
        is_dirty: bool,
    ) -> Option<skia_safe::Image> {
        <HashMap<NodeId, skia_safe::Image> as StaticSubtreeCache>::lookup(
            &mut **self,
            node_id,
            key,
            full_pass,
            is_dirty,
        )
    }

    fn store(&mut self, node_id: NodeId, image: skia_safe::Image, key: StaticCacheKey) {
        <HashMap<NodeId, skia_safe::Image> as StaticSubtreeCache>::store(
            &mut **self,
            node_id,
            image,
            key,
        );
    }
}

fn channel_depends_on_time<T>(anim: &Animated<T>) -> bool
where
    T: Clone + keyframe::CanTween + Default,
{
    anim.raw_keyframes.len() > 1
        || anim
            .raw_keyframes
            .iter()
            .any(|(_, _, ease)| matches!(ease, EasingType::Expression { .. }))
}

fn opt_channel_depends_on_time<T>(anim: &Option<Animated<T>>) -> bool
where
    T: Clone + keyframe::CanTween + Default,
{
    anim.as_ref().is_some_and(channel_depends_on_time)
}

fn transform_depends_on_time(transform: &Transform) -> bool {
    channel_depends_on_time(&transform.scale_x)
        || channel_depends_on_time(&transform.scale_y)
        || channel_depends_on_time(&transform.rotation)
        || channel_depends_on_time(&transform.skew_x)
        || channel_depends_on_time(&transform.skew_y)
        || channel_depends_on_time(&transform.translate_x)
        || channel_depends_on_time(&transform.translate_y)
}

fn element_depends_on_time(element: &dyn crate::element::Element) -> bool {
    if let Some(box_node) = element.as_any().downcast_ref::<BoxNode>() {
        return opt_channel_depends_on_time(&box_node.bg_color)
            || channel_depends_on_time(&box_node.opacity)
            || channel_depends_on_time(&box_node.blur)
            || opt_channel_depends_on_time(&box_node.shadow_color)
            || channel_depends_on_time(&box_node.shadow_blur)
            || channel_depends_on_time(&box_node.shadow_offset_x)
            || channel_depends_on_time(&box_node.shadow_offset_y)
            || channel_depends_on_time(&box_node.border_radius)
            || channel_depends_on_time(&box_node.border_width)
            || opt_channel_depends_on_time(&box_node.border_color)
            || channel_depends_on_time(&box_node.backdrop_blur)
            || box_node.extra_shadows.iter().any(|shadow| {
                channel_depends_on_time(&shadow.color)
                    || channel_depends_on_time(&shadow.blur)
                    || channel_depends_on_time(&shadow.offset_x)
                    || channel_depends_on_time(&shadow.offset_y)
            });
    }
    if let Some(text) = element.as_any().downcast_ref::<TextNode>() {
        return channel_depends_on_time(&text.default_font_size)
            || channel_depends_on_time(&text.default_color)
            || channel_depends_on_time(&text.opacity)
            || channel_depends_on_time(&text.counter_value)
            || text
                .animators
                .iter()
                .any(|animator| channel_depends_on_time(&animator.animation));
    }
    if let Some(image) = element.as_any().downcast_ref::<ImageNode>() {
        return channel_depends_on_time(&image.opacity)
            || channel_depends_on_time(&image.border_radius);
    }
    if let Some(shape) = element.as_any().downcast_ref::<ShapeNode>() {
        return opt_channel_depends_on_time(&shape.fill_color)
            || opt_channel_depends_on_time(&shape.stroke_color)
            || channel_depends_on_time(&shape.stroke_width)
            || channel_depends_on_time(&shape.opacity);
    }
    if let Some(vector) = element.as_any().downcast_ref::<VectorNode>() {
        return channel_depends_on_time(&vector.opacity);
    }
    element
        .as_any()
        .downcast_ref::<SpectrumVisualizerNode>()
        .is_some()
}

fn node_depends_on_time(node: &SceneNode) -> bool {
    if node.element.varies_with_time() {
        return true;
    }
    if matches!(
        node.element.kind(),
        "video"
            | "lottie"
            | "caption"
            | "smart_caption"
            | "active_speaker_camera"
            | "composition"
            | "wgpu"
            | "effect"
            | "html_capture"
    ) {
        return true;
    }
    if !node.audio_bindings.is_empty()
        || transform_depends_on_time(&node.transform)
        || node
            .path_animation
            .as_ref()
            .is_some_and(|path| channel_depends_on_time(&path.progress))
        || node
            .path_morph
            .as_ref()
            .is_some_and(|morph| channel_depends_on_time(&morph.progress))
    {
        return true;
    }
    element_depends_on_time(node.element.as_ref())
}

fn subtree_depends_on_time(scene: &SceneGraph, root: NodeId) -> bool {
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = scene.get_node(id) else {
            continue;
        };
        if node_depends_on_time(node) {
            return true;
        }
        stack.extend(node.children.iter().copied());
        if let Some(mask) = node.mask_node {
            stack.push(mask);
        }
    }
    false
}

fn static_cache_key(
    scene: &SceneGraph,
    node_id: NodeId,
    local_time: f64,
    spatial: StaticCacheKey,
) -> StaticCacheKey {
    let time = if subtree_depends_on_time(scene, node_id) {
        Some(local_time)
    } else {
        None
    };
    StaticCacheKey { time, ..spatial }
}

/// Paint descendants, or only this element's own plane.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChildDraw {
    /// Normal child paint. Preserve-3d parents hoist sharing descendants.
    Normal,
    /// This preserve-3d element was hoisted. Its descendants are other planes.
    Skip,
}

/// One sorted plane in a preserve-3d context.
///
/// `chain` starts at a direct child of the sorting element and ends at the
/// plane. Sharing elements along that chain are not flattened groups.
struct DepthPlane {
    depth: f32,
    z_index: i32,
    source: u32,
    chain: Vec<NodeId>,
}

/// Direct children of a preserve-3d element, with nested sharing descendants
/// lifted into the same list. Depth is the sum of `view_depth` down the
/// preserve-3d chain. A child that does not `shares_3d` is one plane.
fn collect_3d_planes(scene: &SceneGraph, parent_id: NodeId) -> Vec<DepthPlane> {
    let mut planes = Vec::new();
    let mut source = 0u32;
    let mut prefix = Vec::new();
    let children = scene
        .get_node(parent_id)
        .map(|node| node.children.clone())
        .unwrap_or_default();
    for child_id in children {
        push_3d_plane(
            scene,
            child_id,
            0.0,
            &mut prefix,
            &mut source,
            &mut planes,
        );
    }
    planes.sort_by(|a, b| {
        a.depth
            .total_cmp(&b.depth)
            .then(a.z_index.cmp(&b.z_index))
            .then(a.source.cmp(&b.source))
    });
    planes
}

fn push_3d_plane(
    scene: &SceneGraph,
    node_id: NodeId,
    ancestor_depth: f32,
    prefix: &mut Vec<NodeId>,
    source: &mut u32,
    planes: &mut Vec<DepthPlane>,
) {
    let (depth, z_index, shares, children) = {
        let Some(node) = scene.get_node(node_id) else {
            return;
        };
        (
            ancestor_depth
                + node
                    .element
                    .view_depth(node.layout_rect.width(), node.layout_rect.height()),
            node.z_index,
            node.element.shares_3d(),
            node.children.clone(),
        )
    };
    let order = *source;
    *source += 1;
    prefix.push(node_id);
    planes.push(DepthPlane {
        depth,
        z_index,
        source: order,
        chain: prefix.clone(),
    });
    if shares {
        for child_id in children {
            push_3d_plane(scene, child_id, depth, prefix, source, planes);
        }
    }
    prefix.pop();
}

const Z_VARY_EPSILON: f32 = 1.0e-3;

/// Scene-graph placement for one node. Matches `apply_node_scene_transform`.
fn scene_view_matrix(node: &SceneNode) -> skia_safe::M44 {
    let tx = node.layout_rect.left + node.transform.translate_x.current_value;
    let ty = node.layout_rect.top + node.transform.translate_y.current_value;
    let rotation = node.transform.rotation.current_value;
    let scale_x = node.transform.scale_x.current_value;
    let scale_y = node.transform.scale_y.current_value;
    let skew_x = node.transform.skew_x.current_value;
    let skew_y = node.transform.skew_y.current_value;
    if rotation == 0.0 && scale_x == 1.0 && scale_y == 1.0 && skew_x == 0.0 && skew_y == 0.0 {
        return skia_safe::M44::translate(tx, ty, 0.0);
    }
    let pivot_x = node.layout_rect.width() * node.transform.pivot_x;
    let pivot_y = node.layout_rect.height() * node.transform.pivot_y;
    let radians = rotation.to_radians();
    let (sin, cos) = (radians.sin(), radians.cos());
    let rotation_z = skia_safe::M44::row_major(&[
        cos, -sin, 0.0, 0.0, sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    let kx = skew_x.to_radians().tan();
    let ky = skew_y.to_radians().tan();
    let skew = skia_safe::M44::row_major(&[
        1.0, kx, 0.0, 0.0, ky, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    let mut matrix = skia_safe::M44::translate(tx, ty, 0.0);
    matrix = skia_safe::M44::concat(&matrix, &skia_safe::M44::translate(pivot_x, pivot_y, 0.0));
    matrix = skia_safe::M44::concat(&matrix, &rotation_z);
    matrix = skia_safe::M44::concat(&matrix, &skia_safe::M44::scale(scale_x, scale_y, 1.0));
    matrix = skia_safe::M44::concat(&matrix, &skew);
    skia_safe::M44::concat(&matrix, &skia_safe::M44::translate(-pivot_x, -pivot_y, 0.0))
}

/// Leaf-local point to the sorting parent's space, before that parent's perspective.
fn chain_local_matrix(scene: &SceneGraph, chain: &[NodeId]) -> skia_safe::M44 {
    let mut matrix = skia_safe::M44::new_identity();
    for node_id in chain {
        let Some(node) = scene.get_node(*node_id) else {
            continue;
        };
        matrix = skia_safe::M44::concat(&matrix, &scene_view_matrix(node));
        if let Some(local) = node
            .element
            .local_view_matrix(node.layout_rect.width(), node.layout_rect.height())
        {
            matrix = skia_safe::M44::concat(&matrix, &local);
        }
    }
    matrix
}

fn plane_z_varies(scene: &SceneGraph, chain: &[NodeId]) -> bool {
    let matrix = chain_local_matrix(scene, chain);
    matrix.rc(2, 0).abs() > Z_VARY_EPSILON || matrix.rc(2, 1).abs() > Z_VARY_EPSILON
}

/// View Z of the plane point that projects to device `(u, v)`. Larger Z is closer.
fn plane_depth_at(device: &skia_safe::M44, chain: &skia_safe::M44, u: f32, v: f32) -> Option<f32> {
    let a = u * device.rc(3, 0) - device.rc(0, 0);
    let b = u * device.rc(3, 1) - device.rc(0, 1);
    let c = device.rc(0, 3) - u * device.rc(3, 3);
    let d = v * device.rc(3, 0) - device.rc(1, 0);
    let e = v * device.rc(3, 1) - device.rc(1, 1);
    let f = device.rc(1, 3) - v * device.rc(3, 3);
    let det = a * e - b * d;
    if !det.is_finite() || det.abs() < 1.0e-8 {
        return None;
    }
    let x = (c * e - b * f) / det;
    let y = (a * f - c * d) / det;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let point = chain.map(x, y, 0.0, 1.0);
    if !point.w.is_finite() || !point.z.is_finite() {
        return None;
    }
    if point.w.abs() < 1.0e-8 {
        Some(point.z)
    } else {
        Some(point.z / point.w)
    }
}

fn paint_sorted_planes<C: StaticSubtreeCache>(
    scene: &SceneGraph,
    assets: &AssetManager,
    static_cache: &mut C,
    planes: &[DepthPlane],
    canvas: &skia_safe::Canvas,
    parent_opacity: f32,
    depth: usize,
    dirty_nodes: Option<&HashSet<NodeId>>,
) -> Result<(), RenderError> {
    for plane in planes {
        paint_depth_chain(
            scene,
            assets,
            static_cache,
            &plane.chain,
            0,
            canvas,
            parent_opacity,
            depth,
            dirty_nodes,
        )?;
    }
    Ok(())
}

/// Per-pixel occlusion for preserve-3d planes whose Z changes across the quad.
///
/// Planes stay in center-Z order. A covered fragment replaces the stored one
/// when its view Z is greater or equal, so an equal-Z tie still follows
/// z-index and source order. Flat and constant-Z lists do not use this path.
fn paint_occluded_planes<C: StaticSubtreeCache>(
    scene: &SceneGraph,
    assets: &AssetManager,
    static_cache: &mut C,
    planes: &[DepthPlane],
    canvas: &skia_safe::Canvas,
    parent_opacity: f32,
    depth: usize,
    dirty_nodes: Option<&HashSet<NodeId>>,
) -> Result<(), RenderError> {
    let layer = canvas.base_layer_size();
    if layer.width <= 0 || layer.height <= 0 {
        return paint_sorted_planes(
            scene,
            assets,
            static_cache,
            planes,
            canvas,
            parent_opacity,
            depth,
            dirty_nodes,
        );
    }
    let width = layer.width as usize;
    let height = layer.height as usize;
    let parent_ctm = canvas.local_to_device();
    let mut color = vec![0u8; width * height * 4];
    let mut stored = vec![f32::NEG_INFINITY; width * height];
    for plane in planes {
        let chain = chain_local_matrix(scene, &plane.chain);
        let device = skia_safe::M44::concat(&parent_ctm, &chain);
        let mut surface = skia_safe::surfaces::raster_n32_premul((layer.width, layer.height))
            .ok_or(RenderError::SurfaceFailure)?;
        let offscreen = surface.canvas();
        offscreen.clear(skia_safe::Color::TRANSPARENT);
        offscreen.set_matrix(&parent_ctm);
        paint_depth_chain(
            scene,
            assets,
            static_cache,
            &plane.chain,
            0,
            offscreen,
            parent_opacity,
            depth,
            dirty_nodes,
        )?;
        let info = skia_safe::ImageInfo::new(
            (layer.width, layer.height),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut pixels = vec![0u8; width * height * 4];
        if !surface.read_pixels(&info, &mut pixels, width * 4, (0, 0)) {
            return Err(RenderError::SurfaceFailure);
        }
        for y in 0..height {
            for x in 0..width {
                let offset = (y * width + x) * 4;
                if pixels[offset + 3] < 128 {
                    continue;
                }
                let Some(view_z) =
                    plane_depth_at(&device, &chain, x as f32 + 0.5, y as f32 + 0.5)
                else {
                    continue;
                };
                let index = y * width + x;
                if view_z >= stored[index] {
                    stored[index] = view_z;
                    color[offset..offset + 4].copy_from_slice(&pixels[offset..offset + 4]);
                }
            }
        }
    }
    let info = skia_safe::ImageInfo::new(
        (layer.width, layer.height),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let image = skia_safe::images::raster_from_data(
        &info,
        skia_safe::Data::new_copy(&color),
        width * 4,
    )
    .ok_or(RenderError::SurfaceFailure)?;
    canvas.save();
    canvas.reset_matrix();
    canvas.draw_image(&image, (0, 0), None);
    canvas.restore();
    Ok(())
}

/// Same local transform order as the non-static path of `render_recursive`.
fn apply_node_scene_transform(canvas: &skia_safe::Canvas, node: &SceneNode) {
    let width = node.layout_rect.width();
    let height = node.layout_rect.height();
    let pivot_x = width * node.transform.pivot_x;
    let pivot_y = height * node.transform.pivot_y;
    canvas.translate((
        node.layout_rect.left + node.transform.translate_x.current_value,
        node.layout_rect.top + node.transform.translate_y.current_value,
    ));
    canvas.translate((pivot_x, pivot_y));
    canvas.rotate(node.transform.rotation.current_value, None);
    canvas.scale((
        node.transform.scale_x.current_value,
        node.transform.scale_y.current_value,
    ));
    canvas.skew((
        node.transform.skew_x.current_value.to_radians().tan(),
        node.transform.skew_y.current_value.to_radians().tan(),
    ));
    canvas.translate((-pivot_x, -pivot_y));
}

/// Run `paint_descendants` in this element's child space without keeping its pixels.
///
/// Descendant planes are sorted outside this element, so its background must
/// not be redrawn on their pass. `Element::render` still applies the local
/// matrix, perspective, and backface skip. An empty clip drops that pass's paint.
fn enter_preserve_3d_child<F>(
    scene: &SceneGraph,
    node_id: NodeId,
    canvas: &skia_safe::Canvas,
    parent_opacity: f32,
    paint_descendants: F,
) -> Result<(), RenderError>
where
    F: FnOnce(&skia_safe::Canvas) -> Result<(), RenderError>,
{
    canvas.save();
    let mut child_matrix = None;
    let rendered = if let Some(node) = scene.get_node(node_id) {
        apply_node_scene_transform(canvas, node);
        let local_rect =
            skia_safe::Rect::from_wh(node.layout_rect.width(), node.layout_rect.height());
        canvas.save();
        canvas.clip_rect(
            skia_safe::Rect::new_empty(),
            skia_safe::ClipOp::Intersect,
            false,
        );
        let rendered = node.element.render(
            canvas,
            local_rect,
            parent_opacity,
            &mut |child_canvas| {
                child_matrix = Some(child_canvas.local_to_device());
            },
        );
        canvas.restore();
        rendered
    } else {
        canvas.restore();
        return Ok(());
    };
    let result = if let Ok(()) = rendered {
        if let Some(matrix) = child_matrix {
            canvas.set_matrix(&matrix);
            paint_descendants(canvas)
        } else {
            Ok(())
        }
    } else {
        rendered
    };
    canvas.restore();
    result
}

fn paint_depth_chain<C: StaticSubtreeCache>(
    scene: &SceneGraph,
    assets: &AssetManager,
    static_cache: &mut C,
    chain: &[NodeId],
    index: usize,
    canvas: &skia_safe::Canvas,
    parent_opacity: f32,
    depth: usize,
    dirty_nodes: Option<&HashSet<NodeId>>,
) -> Result<(), RenderError> {
    if depth > 100 {
        return Err(RenderError::RecursionLimit);
    }
    let Some(node_id) = chain.get(index).copied() else {
        return Ok(());
    };
    if index + 1 == chain.len() {
        let skip = scene
            .get_node(node_id)
            .is_some_and(|node| node.element.shares_3d());
        return render_recursive_inner(
            scene,
            assets,
            static_cache,
            node_id,
            canvas,
            parent_opacity,
            depth,
            dirty_nodes,
            if skip {
                ChildDraw::Skip
            } else {
                ChildDraw::Normal
            },
        );
    }
    enter_preserve_3d_child(scene, node_id, canvas, parent_opacity, |child_canvas| {
        paint_depth_chain(
            scene,
            assets,
            static_cache,
            chain,
            index + 1,
            child_canvas,
            parent_opacity,
            depth + 1,
            dirty_nodes,
        )
    })
}

/// Renders a node's element and children to the given canvas. The caller is
/// responsible for setting up the canvas transform.
///
/// If `dirty_nodes` is `Some`, only nodes in the set are rendered; this is
/// used after `Director::invalidate()` to incrementally re-render changed nodes.
///
/// A preserve-3d element sorts one shared list: ascending summed `view_depth`,
/// then z-index, then source order. A direct child that `shares_3d` puts its
/// descendants in that list. A child that does not stays one plane. A list
/// whose Z changes across a quad is occluded per pixel. Constant Z stays on
/// this sort, so equal Z still follows z-index and source order.
fn render_node_subtree<C: StaticSubtreeCache>(
    scene: &SceneGraph,
    assets: &AssetManager,
    static_cache: &mut C,
    node_id: NodeId,
    canvas: &skia_safe::Canvas,
    parent_opacity: f32,
    depth: usize,
    dirty_nodes: Option<&std::collections::HashSet<NodeId>>,
    child_draw: ChildDraw,
) -> Result<(), RenderError> {
    let node = match scene.get_node(node_id) {
        Some(n) => n,
        None => return Ok(()),
    };

    let local_rect = skia_safe::Rect::from_wh(node.layout_rect.width(), node.layout_rect.height());

    let shares_3d = node.element.shares_3d();

    let mut last_error = Ok(());
    let mut draw_children = |canvas: &skia_safe::Canvas| {
        if child_draw == ChildDraw::Skip {
            return;
        }
        if shares_3d {
            let planes = collect_3d_planes(scene, node_id);
            let occlude = planes.iter().any(|plane| plane_z_varies(scene, &plane.chain));
            let painted = if occlude {
                paint_occluded_planes(
                    scene,
                    assets,
                    static_cache,
                    &planes,
                    canvas,
                    parent_opacity,
                    depth + 1,
                    dirty_nodes,
                )
            } else {
                paint_sorted_planes(
                    scene,
                    assets,
                    static_cache,
                    &planes,
                    canvas,
                    parent_opacity,
                    depth + 1,
                    dirty_nodes,
                )
            };
            if let Err(error) = painted {
                last_error = Err(error);
            }
            return;
        }
        // Z-Index Sorting
        let mut sorted_children: Vec<(NodeId, i32)> = Vec::with_capacity(node.children.len());
        for &child_id in &node.children {
            if let Some(child) = scene.get_node(child_id) {
                sorted_children.push((child_id, child.z_index));
            }
        }
        sorted_children.sort_by_key(|k| k.1);

        for (child_id, _) in sorted_children {
            if let Err(e) = render_recursive(
                scene,
                assets,
                static_cache,
                child_id,
                canvas,
                parent_opacity,
                depth + 1,
                dirty_nodes,
            ) {
                last_error = Err(e);
            }
        }
    };

    // Check if we need a save layer for blending, masking, or filters
    let need_layer = node.mask_node.is_some()
        || node.blend_mode != skia_safe::BlendMode::SrcOver
        || node.filter.has_effects();

    let result = if need_layer {
        let mut paint = skia_safe::Paint::default();
        paint.set_blend_mode(node.blend_mode);

        // Apply CSS-like image filters (blur, brightness, contrast, saturation, hue-rotate)
        if let Some(filter) = node.filter.to_image_filter() {
            paint.set_image_filter(filter);
        }

        // Create an isolated layer.
        canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&paint));

        let r = node
            .element
            .render(canvas, local_rect, parent_opacity, &mut draw_children);

        if r.is_ok() && last_error.is_ok() {
            if let Some(mask_id) = node.mask_node {
                let mut mask_paint = skia_safe::Paint::default();
                mask_paint.set_blend_mode(skia_safe::BlendMode::DstIn);

                canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&mask_paint));
                if let Err(e) =
                    render_recursive(scene, assets, static_cache, mask_id, canvas, 1.0, depth + 1, dirty_nodes)
                {
                    last_error = Err(e);
                }
                canvas.restore();
            }
        }

        canvas.restore();
        r
    } else {
        node.element
            .render(canvas, local_rect, parent_opacity, &mut draw_children)
    };

    result?;
    last_error?;
    Ok(())
}

/// Recursively renders a node and its children to the canvas.
///
/// Static nodes rasterize into `static_cache`. A full pass (`dirty_nodes ==
/// None`) reuses that image only when the recorded key matches layout size,
/// the baked local transform, inherited opacity, and time. Time is part of
/// the key only when the subtree has time-varying paint, a video, or an
/// expression. A mismatch drops the entry and re-rasters. An incremental
/// pass reuses entries for nodes outside the dirty set and never serves a
/// hit for a dirty static node.
pub fn render_recursive<C: StaticSubtreeCache>(
    scene: &SceneGraph,
    assets: &AssetManager,
    static_cache: &mut C,
    node_id: NodeId,
    canvas: &skia_safe::Canvas,
    parent_opacity: f32,
    depth: usize,
    dirty_nodes: Option<&std::collections::HashSet<NodeId>>,
) -> Result<(), RenderError> {
    render_recursive_inner(
        scene,
        assets,
        static_cache,
        node_id,
        canvas,
        parent_opacity,
        depth,
        dirty_nodes,
        ChildDraw::Normal,
    )
}

fn render_recursive_inner<C: StaticSubtreeCache>(
    scene: &SceneGraph,
    assets: &AssetManager,
    static_cache: &mut C,
    node_id: NodeId,
    canvas: &skia_safe::Canvas,
    parent_opacity: f32,
    depth: usize,
    dirty_nodes: Option<&std::collections::HashSet<NodeId>>,
    child_draw: ChildDraw,
) -> Result<(), RenderError> {
    if depth > 100 {
        return Err(RenderError::RecursionLimit);
    }

    // Extract the values we need so we can release the borrow on `scene` before
    // recursing or rendering to an offscreen surface.
    let (
        is_static,
        layout_x,
        layout_y,
        width,
        height,
        tx,
        ty,
        scale_x,
        scale_y,
        rotation,
        skew_x,
        skew_y,
        pivot_x,
        pivot_y,
        local_time,
    ) = {
        let node = match scene.get_node(node_id) {
            Some(n) => n,
            None => return Ok(()),
        };
        (
            node.is_static,
            node.layout_rect.left,
            node.layout_rect.top,
            node.layout_rect.width(),
            node.layout_rect.height(),
            node.transform.translate_x.current_value,
            node.transform.translate_y.current_value,
            node.transform.scale_x.current_value,
            node.transform.scale_y.current_value,
            node.transform.rotation.current_value,
            node.transform.skew_x.current_value,
            node.transform.skew_y.current_value,
            node.layout_rect.width() * node.transform.pivot_x,
            node.layout_rect.height() * node.transform.pivot_y,
            node.local_time,
        )
    };

    let tan_skew_x = skew_x.to_radians().tan();
    let tan_skew_y = skew_y.to_radians().tan();

    // A skipped preserve-3d shell must not reuse or store a flattened subtree.
    if is_static && child_draw == ChildDraw::Normal {
        let full_pass = dirty_nodes.is_none();
        let is_dirty = dirty_nodes.is_some_and(|set| set.contains(&node_id));
        let key = static_cache_key(
            scene,
            node_id,
            local_time,
            StaticCacheKey {
                width,
                height,
                translate_x: tx,
                translate_y: ty,
                scale_x,
                scale_y,
                rotation,
                skew_x,
                skew_y,
                pivot_x,
                pivot_y,
                parent_opacity,
                time: None,
            },
        );
        if let Some(image) = static_cache.lookup(node_id, &key, full_pass, is_dirty) {
            canvas.save();
            canvas.translate((layout_x + tx, layout_y + ty));
            canvas.draw_image(&image, (0, 0), None);
            canvas.restore();
            return Ok(());
        }

        let info = skia_safe::ImageInfo::new(
            (width as i32, height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            Some(skia_safe::ColorSpace::new_srgb()),
        );
        let mut surface =
            skia_safe::surfaces::raster(&info, None, None).ok_or(RenderError::SurfaceFailure)?;
        let offscreen = surface.canvas();

        offscreen.save();
        // Apply local transforms only (position is applied when drawing the image).
        offscreen.translate((pivot_x, pivot_y));
        offscreen.rotate(rotation, None);
        offscreen.scale((scale_x, scale_y));
        offscreen.skew((tan_skew_x, tan_skew_y));
        offscreen.translate((-pivot_x, -pivot_y));

        render_node_subtree(
            scene,
            assets,
            static_cache,
            node_id,
            offscreen,
            parent_opacity,
            depth,
            dirty_nodes,
            child_draw,
        )?;

        offscreen.restore();

        let image = surface.image_snapshot();
        static_cache.store(node_id, image.clone(), key);

        canvas.save();
        canvas.translate((layout_x + tx, layout_y + ty));
        canvas.draw_image(&image, (0, 0), None);
        canvas.restore();

        return Ok(());
    }

    canvas.save();

    // Apply Transform Stack
    // 1. Move to position
    canvas.translate((layout_x + tx, layout_y + ty));

    // 2. Move to pivot
    canvas.translate((pivot_x, pivot_y));

    // 3. Rotate
    canvas.rotate(rotation, None);

    // 4. Scale
    canvas.scale((scale_x, scale_y));

    // 5. Skew (Degrees to Tangent)
    canvas.skew((tan_skew_x, tan_skew_y));

    // 6. Move back from pivot
    canvas.translate((-pivot_x, -pivot_y));

    render_node_subtree(
        scene,
        assets,
        static_cache,
        node_id,
        canvas,
        parent_opacity,
        depth,
        dirty_nodes,
        child_draw,
    )?;

    canvas.restore();

    Ok(())
}

/// Draws debug overlays (green bounding boxes and red id/z-index labels) for
/// every node in the scene graph.
pub fn draw_debug_overlays(scene: &SceneGraph, canvas: &skia_safe::Canvas) {
    let mut paint = skia_safe::Paint::default();
    paint.set_color(skia_safe::Color::GREEN);
    paint.set_style(skia_safe::paint::Style::Stroke);
    paint.set_stroke_width(2.0);

    let mut text_paint = skia_safe::Paint::default();
    text_paint.set_color(skia_safe::Color::RED);

    let font = skia_safe::Font::default();

    for (id, node) in scene.iter_alive() {
        let rect = node.layout_rect;
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            continue;
        }

        canvas.draw_rect(rect, &paint);

        let label = format!("{} z:{}", id, node.z_index);
        if let Some(blob) = skia_safe::TextBlob::new(&label, &font) {
            canvas.draw_text_blob(&blob, (rect.left + 2.0, rect.top + 12.0), &text_paint);
        }
    }
}

/// Renders the scene at the current director state to a canvas.
/// Assumes `director.update()` and layout have already been called.
///
/// If `dirty_nodes` is `Some`, only nodes in the set are rendered.
fn render_scene_to_canvas(
    director: &mut Director,
    time: f64,
    canvas: &skia_safe::Canvas,
    dirty_nodes: Option<&std::collections::HashSet<NodeId>>,
) -> Result<(), RenderError> {
    let assets = &director.assets;
    canvas.clear(skia_safe::Color::BLACK);

    let mut clips: Vec<(usize, usize, &TrackClip)> = Vec::new();
    for (ti, track) in director.tracks.iter().enumerate() {
        if !track.enabled {
            continue;
        }
        for (ci, clip) in track.clips.iter().enumerate() {
            if clip.is_active_at(time) {
                clips.push((ti, ci, clip));
            }
        }
    }
    clips.sort_by_key(|(_, _, c)| c.z_index);
    for (ti, _, clip) in clips {
        let track_opacity = director.tracks[ti].opacity;
        let fade_opacity = clip.fade_opacity(time);
        render_recursive(
            &director.scene,
            assets,
            &mut director.static_cache,
            clip.scene_root,
            canvas,
            track_opacity * fade_opacity,
            0,
            dirty_nodes,
        )?;
    }

    if director.debug_overlays {
        draw_debug_overlays(&director.scene, canvas);
    }

    Ok(())
}

/// Renders a single frame at a specific timestamp to the provided canvas.
///
/// This is helpful for debugging or generating static previews without running the full export loop.
/// Supports motion blur via sub-frame sampling when `director.samples_per_frame > 1`.
pub fn render_frame(
    director: &mut Director,
    time: f64,
    canvas: &skia_safe::Canvas,
) -> Result<(), RenderError> {
    let samples = director.samples_per_frame.max(1);
    let shutter_angle = director.shutter_angle.clamp(0.0, 360.0);

    if samples == 1 {
        let mut layout_engine = LayoutEngine::new();
        director.update(time, None);
        layout_engine.compute_layout(&mut director.scene, director.width, director.height, time);
        director.run_post_layout(time);
        render_scene_to_canvas(director, time, canvas, None)?;
        return Ok(());
    }

    // Motion blur: sample across the shutter duration and accumulate.
    let fps = director.fps.max(1e-6);
    let frame_duration = 1.0 / fps;
    let shutter_duration = frame_duration * (shutter_angle as f64 / 360.0);
    let shutter_start_time = time - (shutter_duration / 2.0);

    let info = skia_safe::ImageInfo::new(
        (director.width, director.height),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        Some(skia_safe::ColorSpace::new_srgb()),
    );

    let mut scratch_surface =
        skia_safe::surfaces::raster(&info, None, None).ok_or(RenderError::SurfaceFailure)?;

    let mut layout_engine = LayoutEngine::new();

    for s in 0..samples {
        let t_offset = if samples > 1 {
            (s as f64 / (samples - 1) as f64) * shutter_duration
        } else {
            0.0
        };
        let sample_time = shutter_start_time + t_offset;

        director.update(sample_time, None);
        layout_engine.compute_layout(
            &mut director.scene,
            director.width,
            director.height,
            sample_time,
        );
        director.run_post_layout(sample_time);

        render_scene_to_canvas(director, sample_time, scratch_surface.canvas(), None)?;

        // Equal-weight accumulation: each sample contributes 1/n via Plus blend.
        let weight = 1.0 / samples as f32;
        let mut paint = skia_safe::Paint::default();
        paint.set_alpha_f(weight);
        paint.set_blend_mode(skia_safe::BlendMode::Plus);
        let image = scratch_surface.image_snapshot();

        if s == 0 {
            canvas.clear(skia_safe::Color::BLACK);
        }
        canvas.draw_image(&image, (0, 0), Some(&paint));
    }

    Ok(())
}

/// Render an export-equivalent frame, including shutter sampling and transitions.
/// Scratch targets inherit the caller's backend and are reused across frames.
pub fn render_sampled_at_time(
    director: &mut Director,
    layout_engine: &mut LayoutEngine,
    time: f64,
    surface: &mut skia_safe::Surface,
    accumulation: &mut Option<skia_safe::Surface>,
    transitions: &mut Option<(skia_safe::Surface, skia_safe::Surface)>,
) -> Result<(), RenderError> {
    let samples = director.samples_per_frame.max(1);
    if samples == 1 {
        return render_at_time(director, layout_engine, time, surface.canvas(), transitions);
    }
    if accumulation.is_none() {
        *accumulation = Some(
            surface
                .new_surface(&surface.image_info())
                .ok_or(RenderError::SurfaceFailure)?,
        );
    }
    let scratch = accumulation.as_mut().unwrap();
    let shutter_duration =
        (1.0 / director.fps) * (director.shutter_angle.clamp(0.0, 360.0) as f64 / 360.0);
    let shutter_start = time - shutter_duration / 2.0;
    let mut paint = skia_safe::Paint::default();
    paint.set_alpha_f(1.0 / samples as f32);
    paint.set_blend_mode(skia_safe::BlendMode::Plus);
    surface.canvas().clear(skia_safe::Color::BLACK);
    for sample in 0..samples {
        let sample_time = shutter_start + (sample as f64 / (samples - 1) as f64) * shutter_duration;
        render_at_time(
            director,
            layout_engine,
            sample_time,
            scratch.canvas(),
            transitions,
        )?;
        surface
            .canvas()
            .draw_image(scratch.image_snapshot(), (0, 0), Some(&paint));
    }
    Ok(())
}

/// Map layout pixels onto a smaller export surface. Scale 1 leaves the matrix alone.
fn begin_scene_canvas(canvas: &skia_safe::Canvas, director: &Director) {
    let sx = director.output_scale_x;
    let sy = director.output_scale_y;
    if sx != 1.0 || sy != 1.0 {
        canvas.reset_matrix();
        canvas.scale((sx, sy));
    }
}

/// Renders a frame at the given time, handling transitions between scenes.
///
/// Used internally by the export pipeline.
pub fn render_at_time(
    director: &mut Director,
    layout_engine: &mut LayoutEngine,
    time: f64,
    canvas: &skia_safe::Canvas,
    surfaces: &mut Option<(skia_safe::Surface, skia_safe::Surface)>,
) -> Result<(), RenderError> {
    // Build the incremental dirty set if invalidation has been requested.
    // Safe default (plan 03): without a compiled dependency graph, always run
    // a full frame so hierarchy/property edits cannot leave stale pixels.
    let dirty: Option<std::collections::HashSet<NodeId>> =
        if director.dirty_nodes.is_empty() || !director.is_compiled() {
            None
        } else {
            Some(director.expand_dirty_set(&director.dirty_nodes))
        };

    director.update(time, dirty.as_ref());
    layout_engine.compute_layout(&mut director.scene, director.width, director.height, time);
    director.run_post_layout(time);

    // Consume the dirty set now that the incremental render has been prepared.
    let dirty = dirty.as_ref();
    director.dirty_nodes.clear();

    let assets_ref = &director.assets;

    canvas.clear(skia_safe::Color::BLACK);
    begin_scene_canvas(canvas, director);

    // Check transition (indices refer to Director::flat_timeline order).
    let transition = director
        .transitions
        .iter()
        .find(|t| time >= t.start_time && time < t.start_time + t.duration)
        .cloned();

    let flat = director.flat_timeline();
    let mut items: Vec<(usize, TimelineItem)> = flat
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, item)| time >= item.start_time && time < item.start_time + item.duration)
        .collect();
    items.sort_by_key(|(_, item)| item.z_index);

    if let Some(trans) = transition {
        let mut drawn_transition = false;

        for (idx, item) in items {
            if idx == trans.from_scene_idx || idx == trans.to_scene_idx {
                if !drawn_transition {
                    if let Some((surf_a, surf_b)) = surfaces {
                        if let (Some(item_a), Some(item_b)) = (
                            flat.get(trans.from_scene_idx),
                            flat.get(trans.to_scene_idx),
                        ) {
                            surf_a.canvas().clear(skia_safe::Color::TRANSPARENT);
                            surf_b.canvas().clear(skia_safe::Color::TRANSPARENT);
                            begin_scene_canvas(surf_a.canvas(), director);
                            begin_scene_canvas(surf_b.canvas(), director);

                            render_recursive(
                                &director.scene,
                                assets_ref,
                                &mut director.static_cache,
                                item_a.scene_root,
                                surf_a.canvas(),
                                1.0,
                                0,
                                dirty,
                            )?;
                            render_recursive(
                                &director.scene,
                                assets_ref,
                                &mut director.static_cache,
                                item_b.scene_root,
                                surf_b.canvas(),
                                1.0,
                                0,
                                dirty,
                            )?;

                            let img_a = surf_a.image_snapshot();
                            let img_b = surf_b.image_snapshot();

                            let progress =
                                ((time - trans.start_time) / trans.duration).clamp(0.0, 1.0) as f32;
                            let val = trans.easing.eval(progress);

                            let scaled =
                                director.output_scale_x != 1.0 || director.output_scale_y != 1.0;
                            let (paint_w, paint_h) = if scaled {
                                (
                                    (director.width as f32 * director.output_scale_x).round()
                                        as i32,
                                    (director.height as f32 * director.output_scale_y).round()
                                        as i32,
                                )
                            } else {
                                (director.width, director.height)
                            };
                            if scaled {
                                canvas.save();
                                canvas.reset_matrix();
                            }
                            draw_transition(
                                canvas,
                                &img_a,
                                &img_b,
                                val,
                                &trans.kind,
                                paint_w,
                                paint_h,
                            );
                            if scaled {
                                canvas.restore();
                            }
                        }
                    }
                    drawn_transition = true;
                }
            } else {
                render_recursive(
                    &director.scene,
                    assets_ref,
                    &mut director.static_cache,
                    item.scene_root,
                    canvas,
                    1.0,
                    0,
                    dirty,
                )?;
            }
        }
    } else {
        // Prefer track path so fade/track opacity apply.
        let mut clips: Vec<(usize, &TrackClip)> = Vec::new();
        for (ti, track) in director.tracks.iter().enumerate() {
            if !track.enabled {
                continue;
            }
            for clip in &track.clips {
                if clip.is_active_at(time) {
                    clips.push((ti, clip));
                }
            }
        }
        clips.sort_by_key(|(_, c)| c.z_index);
        for (ti, clip) in clips {
            let track_opacity = director.tracks[ti].opacity;
            let fade_opacity = clip.fade_opacity(time);
            render_recursive(
                &director.scene,
                assets_ref,
                &mut director.static_cache,
                clip.scene_root,
                canvas,
                track_opacity * fade_opacity,
                0,
                dirty,
            )?;
        }
    }
    if time < 0.1 {
        debug!("[Frame] render complete");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::director::{Director, TimelineItem};
    use crate::node::BoxNode;
    use crate::video_wrapper::RenderMode;
    use std::sync::Arc;

    fn make_director_with_two_boxes() -> Director {
        let mut director = Director::new(
            1080,
            1920,
            30.0,
            Arc::new(crate::DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        let root = director.scene.add_node(Box::new(BoxNode::new()));
        let child_a = director.scene.add_node(Box::new(BoxNode::new()));
        let child_b = director.scene.add_node(Box::new(BoxNode::new()));

        director.scene.try_add_child(root, child_a);
        director.scene.try_add_child(root, child_b);

        director.push_scene_root_at(root, 0.0, 5.0, Some("scene".into()));

        director
    }

    #[test]
    fn incremental_render_skips_unchanged_nodes() {
        let mut director = make_director_with_two_boxes();
        director.compile();

        let mut layout_engine = LayoutEngine::new();
        let info = skia_safe::ImageInfo::new(
            (director.width, director.height),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            Some(skia_safe::ColorSpace::new_srgb()),
        );
        let mut surface =
            skia_safe::surfaces::raster(&info, None, None).expect("failed to create test surface");

        // Full render at t=0.
        render_at_time(
            &mut director,
            &mut layout_engine,
            0.0,
            surface.canvas(),
            &mut None,
        )
        .expect("full render should succeed");

        let root_id = director.flat_timeline()[0].scene_root;
        let child_a = director.scene.get_node(root_id).unwrap().children[0];
        let child_b = director.scene.get_node(root_id).unwrap().children[1];

        let root_gen_after_full = director.scene.get_node(root_id).unwrap().update_generation;
        let a_gen_after_full = director.scene.get_node(child_a).unwrap().update_generation;
        let b_gen_after_full = director.scene.get_node(child_b).unwrap().update_generation;

        // Change child_a's structural input so the hash-based invalidation
        // actually marks it dirty.
        director.scene.get_node_mut(child_a).unwrap().z_index = 7;
        director.invalidate(vec![crate::compiler::DependencyInput::Node(child_a)]);

        render_at_time(
            &mut director,
            &mut layout_engine,
            0.5,
            surface.canvas(),
            &mut None,
        )
        .expect("incremental render should succeed");

        let root_gen_after_inc = director.scene.get_node(root_id).unwrap().update_generation;
        let a_gen_after_inc = director.scene.get_node(child_a).unwrap().update_generation;
        let b_gen_after_inc = director.scene.get_node(child_b).unwrap().update_generation;

        // Root and child_a should have been re-evaluated; child_b should not.
        assert!(root_gen_after_inc > root_gen_after_full, "root should be re-evaluated as ancestor of dirty node");
        assert!(a_gen_after_inc > a_gen_after_full, "dirty node should be re-evaluated");
        assert_eq!(b_gen_after_inc, b_gen_after_full, "unchanged sibling should not be re-evaluated");
    }

    /// A minimal element that tracks how many times it has been rendered.
    #[derive(Clone, Debug)]
    struct RenderTracker {
        render_count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl RenderTracker {
        fn new() -> (Self, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
            let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            (Self { render_count: count.clone() }, count)
        }
    }

    impl crate::element::Element for RenderTracker {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
        fn layout_style(&self) -> taffy::style::Style {
            taffy::style::Style::default()
        }
        fn needs_measure(&self) -> bool {
            true
        }
        fn measure(
            &self,
            _known_dimensions: taffy::geometry::Size<Option<f32>>,
            _available_space: taffy::geometry::Size<taffy::style::AvailableSpace>,
        ) -> taffy::geometry::Size<f32> {
            taffy::geometry::Size { width: 100.0, height: 100.0 }
        }
        fn update(&mut self, _time: f64) -> bool {
            false
        }
        fn render(
            &self,
            _canvas: &skia_safe::Canvas,
            _rect: skia_safe::Rect,
            _opacity: f32,
            _draw_children: &mut dyn FnMut(&skia_safe::Canvas),
        ) -> Result<(), crate::errors::RenderError> {
            self.render_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        fn animate_property(
            &mut self,
            _property: &str,
            _start: f32,
            _target: f32,
            _duration: f64,
            _easing: &str,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn incremental_render_skips_static_subtree() {
        let mut director = make_director_with_two_boxes();
        director.compile();

        let mut layout_engine = LayoutEngine::new();
        let info = skia_safe::ImageInfo::new(
            (director.width, director.height),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            Some(skia_safe::ColorSpace::new_srgb()),
        );
        let mut surface =
            skia_safe::surfaces::raster(&info, None, None).expect("failed to create test surface");

        // Replace child_b with a tracked static element.
        let root_id = director.flat_timeline()[0].scene_root;
        let child_b = director.scene.get_node(root_id).unwrap().children[1];
        let (tracker, count) = RenderTracker::new();
        if let Some(node) = director.scene.get_node_mut(child_b) {
            node.element = Box::new(tracker);
            node.is_static = true;
            node.parent = Some(root_id);
            node.layout_rect = skia_safe::Rect::from_wh(100.0, 100.0);
        }

        // Full render: static subtree is rasterized once.
        render_at_time(&mut director, &mut layout_engine, 0.0, surface.canvas(), &mut None)
            .expect("full render should succeed");
        let count_after_full = count.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(count_after_full, 1, "static subtree should be rendered once");

        // Change child_a's structural input and invalidate it so the
        // incremental path actually runs.
        let child_a = director.scene.get_node(root_id).unwrap().children[0];
        director.scene.get_node_mut(child_a).unwrap().z_index = 7;
        director.invalidate(vec![crate::compiler::DependencyInput::Node(child_a)]);

        render_at_time(&mut director, &mut layout_engine, 0.5, surface.canvas(), &mut None)
            .expect("incremental render should succeed");
        let count_after_inc = count.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(count_after_inc, count_after_full, "static subtree should not be re-rendered when not dirty");
    }

    #[test]
    fn full_render_re_evaluates_all_nodes() {
        let mut director = make_director_with_two_boxes();
        director.compile();

        let mut layout_engine = LayoutEngine::new();
        let info = skia_safe::ImageInfo::new(
            (director.width, director.height),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            Some(skia_safe::ColorSpace::new_srgb()),
        );
        let mut surface =
            skia_safe::surfaces::raster(&info, None, None).expect("failed to create test surface");

        render_at_time(
            &mut director,
            &mut layout_engine,
            0.0,
            surface.canvas(),
            &mut None,
        )
        .expect("full render should succeed");

        let root_id = director.flat_timeline()[0].scene_root;
        let child_a = director.scene.get_node(root_id).unwrap().children[0];
        let child_b = director.scene.get_node(root_id).unwrap().children[1];

        let a_gen_first = director.scene.get_node(child_a).unwrap().update_generation;
        let b_gen_first = director.scene.get_node(child_b).unwrap().update_generation;

        // Without invalidation, a second render should still update all nodes.
        render_at_time(
            &mut director,
            &mut layout_engine,
            0.5,
            surface.canvas(),
            &mut None,
        )
        .expect("second full render should succeed");

        let a_gen_second = director.scene.get_node(child_a).unwrap().update_generation;
        let b_gen_second = director.scene.get_node(child_b).unwrap().update_generation;

        assert!(a_gen_second > a_gen_first, "child_a should be updated during full render");
        assert!(b_gen_second > b_gen_first, "child_b should be updated during full render");
    }

    #[test]
    fn parent_invalidation_re_evaluates_descendants() {
        let mut director = make_director_with_two_boxes();
        director.compile();

        let mut layout_engine = LayoutEngine::new();
        let info = skia_safe::ImageInfo::new(
            (director.width, director.height),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            Some(skia_safe::ColorSpace::new_srgb()),
        );
        let mut surface =
            skia_safe::surfaces::raster(&info, None, None).expect("failed to create test surface");

        // Full render to establish a baseline.
        render_at_time(
            &mut director,
            &mut layout_engine,
            0.0,
            surface.canvas(),
            &mut None,
        )
        .expect("full render should succeed");

        let root_id = director.flat_timeline()[0].scene_root;
        let child_a = director.scene.get_node(root_id).unwrap().children[0];
        let child_b = director.scene.get_node(root_id).unwrap().children[1];

        let root_gen_first = director.scene.get_node(root_id).unwrap().update_generation;
        let a_gen_first = director.scene.get_node(child_a).unwrap().update_generation;
        let b_gen_first = director.scene.get_node(child_b).unwrap().update_generation;

        // Change the root's structural input so hash-based invalidation marks
        // it dirty, then invalidate the root. The dirty-set expansion should
        // include the root's descendants.
        director.scene.get_node_mut(root_id).unwrap().z_index = 7;
        director.invalidate(vec![crate::compiler::DependencyInput::Node(root_id)]);

        render_at_time(
            &mut director,
            &mut layout_engine,
            0.5,
            surface.canvas(),
            &mut None,
        )
        .expect("incremental render should succeed");

        let root_gen_second = director.scene.get_node(root_id).unwrap().update_generation;
        let a_gen_second = director.scene.get_node(child_a).unwrap().update_generation;
        let b_gen_second = director.scene.get_node(child_b).unwrap().update_generation;

        assert!(root_gen_second > root_gen_first, "root should be re-evaluated");
        assert!(a_gen_second > a_gen_first, "child_a should be re-evaluated as descendant of dirty parent");
        assert!(b_gen_second > b_gen_first, "child_b should be re-evaluated as descendant of dirty parent");
    }
}
