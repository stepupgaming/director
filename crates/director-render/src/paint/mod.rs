//! Node traversal and the static layer cache.
//!
//! [`Painter::root`] paints one scene root: transforms, blend and filter
//! layers, masks, z-index order and preserve-3d depth sorting. Static nodes
//! rasterize once into a [`LayerCache`] and reuse that layer while its key
//! holds. `kinds` owns per-kind knowledge, and `depth` owns the 3D sort.
//!
//! The cache is `Director::static_cache`, so `Director::note_mutation`
//! evicts layers that a live edit invalidates.

mod bsp;
mod depth;
mod kinds;

use director_core::director::{StaticCacheEntry, StaticCacheKey};
use director_core::scene::{SceneGraph, SceneNode};
use director_core::types::NodeId;
use director_core::RenderError;
use skia_safe::{AlphaType, BlendMode, Canvas, ClipOp, Color, ColorSpace, ColorType, Font, Image, ImageInfo, Matrix, M44, Paint, Point, Rect, TextBlob};
use std::collections::{HashMap, HashSet};

/// Deepest node nesting a traversal accepts.
const MAX_DEPTH: usize = 100;

/// Static layers of one scene graph.
///
/// A full pass reuses a layer only when its key matches layout size, the baked
/// local transform, inherited opacity and time. Time is part of the key only
/// when the subtree has time-varying paint. An incremental pass reuses layers
/// outside the dirty set and never serves a dirty node.
pub(crate) type LayerCache = HashMap<NodeId, StaticCacheEntry>;

fn lookup(cache: &mut LayerCache, node_id: NodeId, key: &StaticCacheKey, full_pass: bool, is_dirty: bool) -> Option<Image> {
    if is_dirty {
        cache.remove(&node_id);
        return None;
    }
    let entry = cache.get(&node_id)?;
    if !full_pass || &entry.key == key {
        return Some(entry.image.clone());
    }
    cache.remove(&node_id);
    None
}

/// Paint descendants, or only this element's own plane.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChildDraw {
    /// Normal child paint. Preserve-3d parents hoist sharing descendants.
    Normal,
    /// This preserve-3d element was hoisted. Its descendants are other planes.
    Skip,
}

/// One traversal of a scene graph.
pub(crate) struct Painter<'a> {
    pub scene: &'a SceneGraph,
    /// Nodes repainted by an incremental frame; `None` repaints everything.
    pub dirty: Option<&'a HashSet<NodeId>>,
}

/// True when any node under `root`, masks included, paints differently over time.
fn subtree_varies(scene: &SceneGraph, root: NodeId) -> bool {
    subtree_any(scene, root, kinds::node_varies)
}

/// True when any node under `root`, masks included, may paint differently
/// somewhere in local time `[t0, t1]`.
/// Children ask over the span of times their parent's clock maps the range to.
pub(crate) fn subtree_moves(scene: &SceneGraph, root: NodeId, t0: f64, t1: f64) -> bool {
    subtree_changes(scene, root, t0, t1, kinds::node_moves)
}

/// Every node under `root`, masks included, whose paint may move inside its
/// box over local time `[t0, t1]`, with that travel in local pixels as
/// [`kinds::node_paint_travel`] reports it.
pub(crate) fn paint_travels(scene: &SceneGraph, root: NodeId, t0: f64, t1: f64, out: &mut Vec<(NodeId, Option<f32>)>) {
    let mut stack = vec![(root, t0, t1)];
    let mut seen = HashSet::new();
    while let Some((id, t0, t1)) = stack.pop() {
        let Some(node) = scene.get_node(id).filter(|_| seen.insert(id)) else {
            continue;
        };
        match kinds::node_paint_travel(node, t0, t1) {
            Some(travel) if travel <= 0.0 => {}
            travel => out.push((id, travel)),
        }
        let (c0, c1) = node.element.child_time_range(t0, t1);
        stack.extend(node.children.iter().map(|&child| (child, c0, c1)));
        stack.extend(node.mask_node.map(|mask| (mask, t0, t1)));
    }
}

fn subtree_changes(scene: &SceneGraph, root: NodeId, t0: f64, t1: f64, test: fn(&SceneNode, f64, f64) -> bool) -> bool {
    let mut stack = vec![(root, t0, t1)];
    let mut seen = HashSet::new();
    while let Some((id, t0, t1)) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = scene.get_node(id) else {
            continue;
        };
        if test(node, t0, t1) {
            return true;
        }
        let (c0, c1) = node.element.child_time_range(t0, t1);
        stack.extend(node.children.iter().map(|&child| (child, c0, c1)));
        stack.extend(node.mask_node.map(|mask| (mask, t0, t1)));
    }
    false
}

/// A node box and the matrix that maps it to the device.
pub(crate) struct BoxPlacement {
    pub matrix: M44,
    pub size: (f32, f32),
}

/// Placement of every node box under `root`, keyed by node, where `matrix`
/// maps the root's parent space to the device.
pub(crate) fn box_placements(scene: &SceneGraph, root: NodeId, matrix: M44, out: &mut HashMap<NodeId, BoxPlacement>) {
    let mut stack = vec![(root, matrix, 0)];
    while let Some((id, parent, level)) = stack.pop() {
        let Some(node) = scene.get_node(id).filter(|_| level <= MAX_DEPTH) else {
            continue;
        };
        let (width, height) = (node.layout_rect.width(), node.layout_rect.height());
        let mut matrix = M44::concat(&parent, &depth::scene_view_matrix(node));
        match node.element.local_view_matrix(width, height) {
            Some(view) => matrix = M44::concat(&matrix, &view),
            None => {
                if let Some(transform) = node.element.local_transform(width, height) {
                    matrix = M44::concat(&matrix, &M44::from(transform));
                }
            }
        }
        let inner = node.element.child_view_matrix(width, height).map_or_else(|| matrix.clone(), |view| M44::concat(&matrix, &view));
        stack.extend(node.children.iter().map(|&child| (child, inner.clone(), level + 1)));
        out.insert(id, BoxPlacement { matrix, size: (width, height) });
    }
}

fn subtree_any(scene: &SceneGraph, root: NodeId, test: impl Fn(&SceneNode) -> bool) -> bool {
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = scene.get_node(id) else {
            continue;
        };
        if test(node) {
            return true;
        }
        stack.extend(node.children.iter().copied());
        stack.extend(node.mask_node);
    }
    false
}

/// The matrix `Painter::node` applies before a node's subtree: layout position,
/// translation, then rotation, scale and skew about the pivot.
fn placement(node: &SceneNode) -> Matrix {
    let transform = &node.transform;
    let pivot_x = node.layout_rect.width() * transform.pivot_x;
    let pivot_y = node.layout_rect.height() * transform.pivot_y;
    let mut matrix = Matrix::translate((
        node.layout_rect.left + transform.translate_x.current_value + pivot_x,
        node.layout_rect.top + transform.translate_y.current_value + pivot_y,
    ));
    matrix.pre_rotate(transform.rotation.current_value, None);
    matrix.pre_scale((transform.scale_x.current_value, transform.scale_y.current_value), None);
    matrix.pre_skew(
        (transform.skew_x.current_value.to_radians().tan(), transform.skew_y.current_value.to_radians().tan()),
        None,
    );
    matrix.pre_translate((-pivot_x, -pivot_y));
    matrix
}

/// Clip `canvas` to the device pixels covering local `bounds` inside a save.
/// Returns false and leaves the canvas alone when a corner maps behind the eye.
fn clip_device(canvas: &Canvas, bounds: Rect, view: Option<&M44>) -> bool {
    let matrix = canvas.local_to_device();
    let through = view.map_or_else(|| matrix.clone(), |view| M44::concat(&matrix, view));
    let mut corners = [Point::default(); 4];
    let points = [(bounds.left, bounds.top), (bounds.right, bounds.top), (bounds.right, bounds.bottom), (bounds.left, bounds.bottom)];
    for (corner, (x, y)) in corners.iter_mut().zip(points) {
        let mapped = through.map(x, y, 0.0, 1.0);
        if mapped.w <= f32::EPSILON {
            return false;
        }
        *corner = Point::new(mapped.x / mapped.w, mapped.y / mapped.w);
    }
    let Some(device) = Rect::bounds(&corners) else {
        return false;
    };
    canvas.save();
    canvas.reset_matrix();
    let device: skia_safe::IRect = skia_safe::RoundOut::round_out(&device);
    canvas.clip_irect(device.with_outset((1, 1)), ClipOp::Intersect);
    canvas.set_matrix(&matrix);
    true
}

impl Painter<'_> {
    /// Paint the subtree at `root` onto `canvas` under the canvas's current matrix.
    pub(crate) fn root(&self, cache: &mut LayerCache, root: NodeId, canvas: &Canvas, opacity: f32) -> Result<(), RenderError> {
        self.node(cache, root, canvas, opacity, 0, ChildDraw::Normal)
    }

    /// Bounds the subtree at `id` paints in the space `subtree` draws it in,
    /// after placement and before the element's own transform. `None` means
    /// unbounded: a 3D context, an element without bounds or a projective child.
    fn subtree_bounds(&self, id: NodeId, depth: usize) -> Option<Rect> {
        if depth > MAX_DEPTH {
            return None;
        }
        let node = self.scene.get_node(id)?;
        let (width, height) = (node.layout_rect.width(), node.layout_rect.height());
        if node.element.shares_3d() || node.element.local_view_matrix(width, height).is_some() {
            return None;
        }
        let mut children = Rect::new_empty();
        for &child_id in &node.children {
            let Some(child) = self.scene.get_node(child_id) else {
                continue;
            };
            children.join(placement(child).map_rect(self.subtree_bounds(child_id, depth + 1)?).0);
        }
        let inner = node.element.paint_bounds(Rect::from_wh(width, height), Some(children))?;
        let mut bounds = node.element.local_transform(width, height).map_or(inner, |matrix| matrix.map_rect(inner).0);
        if node.filter.has_effects() {
            if let Some(filter) = node.filter.to_image_filter() {
                bounds = filter.compute_fast_bounds(bounds);
            }
        }
        Some(bounds)
    }

    /// Clips `canvas` to the device pixels the subtree at `id` can reach.
    /// `false` leaves it unclipped.
    fn clip_layer(&self, id: NodeId, depth: usize, canvas: &Canvas) -> bool {
        if let Some(bounds) = self.subtree_bounds(id, depth) {
            return clip_device(canvas, bounds, None);
        }
        // An element's 3D transform carries its flat subtree in one plane.
        let Some(node) = self.scene.get_node(id) else {
            return false;
        };
        let (width, height) = (node.layout_rect.width(), node.layout_rect.height());
        if node.element.shares_3d() || node.filter.has_effects() {
            return false;
        }
        let Some(view) = node.element.local_view_matrix(width, height) else {
            return false;
        };
        let mut children = Rect::new_empty();
        for &child_id in &node.children {
            let Some(child) = self.scene.get_node(child_id) else {
                continue;
            };
            match self.subtree_bounds(child_id, depth + 1) {
                Some(bounds) => children.join(placement(child).map_rect(bounds).0),
                None => return false,
            }
        }
        node.element
            .paint_bounds(Rect::from_wh(width, height), Some(children))
            .is_some_and(|inner| clip_device(canvas, inner, Some(&view)))
    }

    fn node(
        &self,
        cache: &mut LayerCache,
        node_id: NodeId,
        canvas: &Canvas,
        opacity: f32,
        depth: usize,
        child_draw: ChildDraw,
    ) -> Result<(), RenderError> {
        if depth > MAX_DEPTH {
            return Err(RenderError::RecursionLimit);
        }
        let Some(node) = self.scene.get_node(node_id) else {
            return Ok(());
        };
        let transform = &node.transform;
        let (width, height) = (node.layout_rect.width(), node.layout_rect.height());
        let (x, y) = (node.layout_rect.left, node.layout_rect.top);
        let (tx, ty) = (transform.translate_x.current_value, transform.translate_y.current_value);
        let (scale_x, scale_y) = (transform.scale_x.current_value, transform.scale_y.current_value);
        let rotation = transform.rotation.current_value;
        let (skew_x, skew_y) = (transform.skew_x.current_value, transform.skew_y.current_value);
        let (pivot_x, pivot_y) = (width * transform.pivot_x, height * transform.pivot_y);
        let apply_local = |canvas: &Canvas| {
            canvas.translate((pivot_x, pivot_y));
            canvas.rotate(rotation, None);
            canvas.scale((scale_x, scale_y));
            canvas.skew((skew_x.to_radians().tan(), skew_y.to_radians().tan()));
            canvas.translate((-pivot_x, -pivot_y));
        };

        // A skipped preserve-3d shell must not reuse or store a flattened subtree.
        if !(node.is_static && child_draw == ChildDraw::Normal) {
            canvas.save();
            canvas.translate((x + tx, y + ty));
            apply_local(canvas);
            let painted = self.subtree(cache, node_id, canvas, opacity, depth, child_draw);
            canvas.restore();
            return painted;
        }

        let full_pass = self.dirty.is_none();
        let is_dirty = self.dirty.is_some_and(|set| set.contains(&node_id));
        let key = StaticCacheKey {
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
            parent_opacity: opacity,
            time: subtree_varies(self.scene, node_id).then_some(node.local_time),
        };
        let image = match lookup(cache, node_id, &key, full_pass, is_dirty) {
            Some(image) => image,
            None => {
                // The layer bakes the local transform; position applies when it is drawn.
                let info = ImageInfo::new(
                    (width as i32, height as i32),
                    ColorType::RGBA8888,
                    AlphaType::Premul,
                    Some(ColorSpace::new_srgb()),
                );
                let mut surface = skia_safe::surfaces::raster(&info, None, None).ok_or(RenderError::SurfaceFailure)?;
                let offscreen = surface.canvas();
                offscreen.save();
                apply_local(offscreen);
                self.subtree(cache, node_id, offscreen, opacity, depth, child_draw)?;
                offscreen.restore();
                let image = surface.image_snapshot();
                cache.insert(node_id, StaticCacheEntry { image: image.clone(), key });
                image
            }
        };
        canvas.save();
        canvas.translate((x + tx, y + ty));
        canvas.draw_image(&image, (0, 0), None);
        canvas.restore();
        Ok(())
    }

    /// Paint a node's element and children in its local space: blend, filter and
    /// mask layers, then children in z-index order or as sorted 3D planes.
    fn subtree(
        &self,
        cache: &mut LayerCache,
        node_id: NodeId,
        canvas: &Canvas,
        opacity: f32,
        depth: usize,
        child_draw: ChildDraw,
    ) -> Result<(), RenderError> {
        let Some(node) = self.scene.get_node(node_id) else {
            return Ok(());
        };
        let local_rect = Rect::from_wh(node.layout_rect.width(), node.layout_rect.height());
        let shares_3d = node.element.shares_3d();

        let mut last_error = Ok(());
        let mut draw_children = |canvas: &Canvas| {
            if child_draw == ChildDraw::Skip {
                return;
            }
            let painted = if shares_3d {
                let planes = depth::collect_planes(self.scene, node_id);
                if depth::z_varies(self.scene, &planes) {
                    self.occluded_planes(cache, &planes, canvas, opacity, depth + 1)
                } else {
                    self.sorted_planes(cache, &planes, canvas, opacity, depth + 1)
                }
            } else {
                let mut children: Vec<(NodeId, i32)> = node
                    .children
                    .iter()
                    .filter_map(|&id| self.scene.get_node(id).map(|child| (id, child.z_index)))
                    .collect();
                children.sort_by_key(|&(_, z_index)| z_index);
                // Every child paints; the last failure is reported.
                let mut painted = Ok(());
                for (child_id, _) in children {
                    if let Err(error) = self.node(cache, child_id, canvas, opacity, depth + 1, ChildDraw::Normal) {
                        painted = Err(error);
                    }
                }
                painted
            };
            if let Err(error) = painted {
                last_error = Err(error);
            }
        };

        let need_layer = node.mask_node.is_some() || node.blend_mode != BlendMode::SrcOver || node.filter.has_effects();
        // A layer without bounds covers the whole clip, so clip layered
        // subtrees to the device pixels they can reach.
        let clipped = (need_layer || node.element.opens_layer(opacity))
            && self.clip_layer(node_id, depth, canvas);
        let result = if need_layer {
            let mut paint = Paint::default();
            paint.set_blend_mode(node.blend_mode);
            if let Some(filter) = node.filter.to_image_filter() {
                paint.set_image_filter(filter);
            }
            canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&paint));
            let painted = kinds::paint_element(node, canvas, local_rect, opacity, &mut draw_children);
            let mask = node.mask_node.filter(|_| painted.is_ok() && last_error.is_ok());
            if let Some(mask_id) = mask {
                let mut mask_paint = Paint::default();
                mask_paint.set_blend_mode(BlendMode::DstIn);
                canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&mask_paint));
                if let Err(error) = self.node(cache, mask_id, canvas, 1.0, depth + 1, ChildDraw::Normal) {
                    last_error = Err(error);
                }
                canvas.restore();
            }
            canvas.restore();
            painted
        } else {
            kinds::paint_element(node, canvas, local_rect, opacity, &mut draw_children)
        };
        if clipped {
            canvas.restore();
        }
        result?;
        last_error
    }
}

/// Outline every laid-out node with its id and z-index, in layout space.
pub(crate) fn debug_overlays(scene: &SceneGraph, canvas: &Canvas) {
    let mut outline = Paint::default();
    outline.set_color(Color::GREEN);
    outline.set_style(skia_safe::paint::Style::Stroke);
    outline.set_stroke_width(2.0);
    let mut label_paint = Paint::default();
    label_paint.set_color(Color::RED);
    let font = Font::default();
    for (id, node) in scene.iter_alive() {
        let rect = node.layout_rect;
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            continue;
        }
        canvas.draw_rect(rect, &outline);
        if let Some(blob) = TextBlob::new(format!("{} z:{}", id, node.z_index), &font) {
            canvas.draw_text_blob(&blob, (rect.left + 2.0, rect.top + 12.0), &label_paint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use director_core::element::Element;
    use director_core::scene::SceneGraph;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Counts its paints; `clocked` makes its paint follow local time.
    #[derive(Clone, Debug)]
    struct Counter {
        paints: Arc<AtomicUsize>,
        clocked: bool,
    }

    impl Element for Counter {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
        fn layout_style(&self) -> taffy::style::Style {
            taffy::style::Style::default()
        }
        fn update(&mut self, _time: f64) -> bool {
            false
        }
        fn varies_with_time(&self) -> bool {
            self.clocked
        }
        fn render(&self, canvas: &Canvas, rect: Rect, _: f32, _: &mut dyn FnMut(&Canvas)) -> Result<(), RenderError> {
            self.paints.fetch_add(1, Ordering::SeqCst);
            canvas.draw_rect(rect, &Paint::default());
            Ok(())
        }
        fn animate_property(&mut self, _: &str, _: f32, _: f32, _: f64, _: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn static_scene(clocked: bool) -> (SceneGraph, NodeId, Arc<AtomicUsize>) {
        let paints = Arc::new(AtomicUsize::new(0));
        let mut scene = SceneGraph::new();
        let id = scene.add_node(Box::new(Counter { paints: paints.clone(), clocked }));
        let node = scene.get_node_mut(id).unwrap();
        node.is_static = true;
        node.layout_rect = Rect::from_xywh(10.0, 10.0, 20.0, 20.0);
        (scene, id, paints)
    }

    fn paint(scene: &SceneGraph, cache: &mut LayerCache, root: NodeId, opacity: f32, dirty: Option<&HashSet<NodeId>>) {
        let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
        Painter { scene, dirty }.root(cache, root, surface.canvas(), opacity).unwrap();
    }

    #[test]
    fn a_static_layer_paints_once_until_its_key_changes() {
        let (scene, id, paints) = static_scene(false);
        let mut cache = LayerCache::new();
        paint(&scene, &mut cache, id, 1.0, None);
        paint(&scene, &mut cache, id, 1.0, None);
        assert_eq!(paints.load(Ordering::SeqCst), 1);
        paint(&scene, &mut cache, id, 0.5, None);
        assert_eq!(paints.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_clocked_static_layer_repaints_when_local_time_moves() {
        let (mut scene, id, paints) = static_scene(true);
        let mut cache = LayerCache::new();
        paint(&scene, &mut cache, id, 1.0, None);
        paint(&scene, &mut cache, id, 1.0, None);
        assert_eq!(paints.load(Ordering::SeqCst), 1);
        scene.get_node_mut(id).unwrap().local_time = 0.5;
        paint(&scene, &mut cache, id, 1.0, None);
        assert_eq!(paints.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn an_incremental_pass_reuses_clean_layers_and_repaints_dirty_ones() {
        let (scene, id, paints) = static_scene(false);
        let mut cache = LayerCache::new();
        paint(&scene, &mut cache, id, 1.0, None);
        // Clean nodes hit without comparing the key.
        paint(&scene, &mut cache, id, 0.5, Some(&HashSet::new()));
        assert_eq!(paints.load(Ordering::SeqCst), 1);
        paint(&scene, &mut cache, id, 1.0, Some(&HashSet::from([id])));
        assert_eq!(paints.load(Ordering::SeqCst), 2);
    }
}
