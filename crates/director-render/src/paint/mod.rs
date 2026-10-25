//! Node traversal and the static layer cache.
//!
//! [`Painter::root`] paints one scene root: transforms, blend and filter
//! layers, masks, z-index order and preserve-3d depth sorting. Static nodes
//! rasterize once into a [`LayerCache`] and reuse that layer while its key
//! holds. `kinds` owns per-kind knowledge, and `depth` owns the 3D sort.

mod depth;
mod kinds;

use director_core::director::StaticCacheKey;
use director_core::scene::SceneGraph;
use director_core::types::NodeId;
use director_core::RenderError;
use kinds::Nested;
use skia_safe::{AlphaType, BlendMode, Canvas, ColorSpace, ColorType, Image, ImageInfo, Paint, Rect};
use std::collections::{HashMap, HashSet};

/// Deepest node nesting a traversal accepts.
const MAX_DEPTH: usize = 100;

/// A rasterized static subtree and the key baked into it.
struct Layer {
    image: Image,
    key: StaticCacheKey,
}

/// Static layers of one scene graph, plus the nested state of its compositions.
///
/// A full pass reuses a layer only when its key matches layout size, the baked
/// local transform, inherited opacity and time. Time is part of the key only
/// when the subtree has time-varying paint. An incremental pass reuses layers
/// outside the dirty set and never serves a dirty node.
#[derive(Default)]
pub(crate) struct LayerCache {
    layers: HashMap<NodeId, Layer>,
    nested: HashMap<NodeId, Nested>,
}

impl LayerCache {
    fn lookup(&mut self, node_id: NodeId, key: &StaticCacheKey, full_pass: bool, is_dirty: bool) -> Option<Image> {
        if is_dirty {
            self.layers.remove(&node_id);
            return None;
        }
        let layer = self.layers.get(&node_id)?;
        if !full_pass || &layer.key == key {
            return Some(layer.image.clone());
        }
        self.layers.remove(&node_id);
        None
    }

    fn store(&mut self, node_id: NodeId, image: Image, key: StaticCacheKey) {
        self.layers.insert(node_id, Layer { image, key });
    }
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
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = scene.get_node(id) else {
            continue;
        };
        if kinds::node_varies(node) {
            return true;
        }
        stack.extend(node.children.iter().copied());
        stack.extend(node.mask_node);
    }
    false
}

impl Painter<'_> {
    /// Paint the subtree at `root` onto `canvas` under the canvas's current matrix.
    pub(crate) fn root(&self, cache: &mut LayerCache, root: NodeId, canvas: &Canvas, opacity: f32) -> Result<(), RenderError> {
        self.node(cache, root, canvas, opacity, 0, ChildDraw::Normal)
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
        let image = match cache.lookup(node_id, &key, full_pass, is_dirty) {
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
                cache.store(node_id, image.clone(), key);
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
        // Nested state leaves the cache while the children closure borrows it.
        let mut nested = kinds::has_nested(node).then(|| cache.nested.remove(&node_id).unwrap_or_default());

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
        let result = if need_layer {
            let mut paint = Paint::default();
            paint.set_blend_mode(node.blend_mode);
            if let Some(filter) = node.filter.to_image_filter() {
                paint.set_image_filter(filter);
            }
            canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&paint));
            let painted = kinds::paint_element(node, nested.as_mut(), canvas, local_rect, opacity, &mut draw_children);
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
            kinds::paint_element(node, nested.as_mut(), canvas, local_rect, opacity, &mut draw_children)
        };
        if let Some(nested) = nested {
            cache.nested.insert(node_id, nested);
        }
        result?;
        last_error
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
        let mut cache = LayerCache::default();
        paint(&scene, &mut cache, id, 1.0, None);
        paint(&scene, &mut cache, id, 1.0, None);
        assert_eq!(paints.load(Ordering::SeqCst), 1);
        paint(&scene, &mut cache, id, 0.5, None);
        assert_eq!(paints.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_clocked_static_layer_repaints_when_local_time_moves() {
        let (mut scene, id, paints) = static_scene(true);
        let mut cache = LayerCache::default();
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
        let mut cache = LayerCache::default();
        paint(&scene, &mut cache, id, 1.0, None);
        // Clean nodes hit without comparing the key.
        paint(&scene, &mut cache, id, 0.5, Some(&HashSet::new()));
        assert_eq!(paints.load(Ordering::SeqCst), 1);
        paint(&scene, &mut cache, id, 1.0, Some(&HashSet::from([id])));
        assert_eq!(paints.load(Ordering::SeqCst), 2);
    }
}
