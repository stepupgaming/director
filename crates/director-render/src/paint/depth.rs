//! Preserve-3d depth sorting.
//!
//! A preserve-3d element sorts one shared list: ascending summed `view_depth`,
//! then z-index, then source order. A direct child that `shares_3d` puts its
//! descendants in that list; a child that does not stays one plane. A list
//! whose Z changes across a quad is occluded per pixel. Constant Z stays on
//! the sort, so equal Z still follows z-index and source order.

use super::{ChildDraw, LayerCache, Painter};
use director_core::scene::{SceneGraph, SceneNode};
use director_core::types::NodeId;
use director_core::RenderError;
use skia_safe::{AlphaType, Canvas, ClipOp, ColorType, ImageInfo, Rect, M44};

const Z_VARY_EPSILON: f32 = 1.0e-3;

/// One sorted plane in a preserve-3d context.
///
/// `chain` starts at a direct child of the sorting element and ends at the
/// plane. Sharing elements along that chain are not flattened groups.
pub(super) struct DepthPlane {
    depth: f32,
    z_index: i32,
    source: u32,
    chain: Vec<NodeId>,
}

/// Direct children of a preserve-3d element, with nested sharing descendants
/// lifted into the same list. Depth is the sum of `view_depth` down the chain.
pub(super) fn collect_planes(scene: &SceneGraph, parent_id: NodeId) -> Vec<DepthPlane> {
    let mut planes = Vec::new();
    let mut source = 0u32;
    let mut prefix = Vec::new();
    let children = scene.get_node(parent_id).map(|node| node.children.clone()).unwrap_or_default();
    for child_id in children {
        push_plane(scene, child_id, 0.0, &mut prefix, &mut source, &mut planes);
    }
    planes.sort_by(|a, b| {
        a.depth
            .total_cmp(&b.depth)
            .then(a.z_index.cmp(&b.z_index))
            .then(a.source.cmp(&b.source))
    });
    planes
}

fn push_plane(
    scene: &SceneGraph,
    node_id: NodeId,
    ancestor_depth: f32,
    prefix: &mut Vec<NodeId>,
    source: &mut u32,
    planes: &mut Vec<DepthPlane>,
) {
    let Some(node) = scene.get_node(node_id) else {
        return;
    };
    let depth = ancestor_depth + node.element.view_depth(node.layout_rect.width(), node.layout_rect.height());
    let order = *source;
    *source += 1;
    prefix.push(node_id);
    planes.push(DepthPlane { depth, z_index: node.z_index, source: order, chain: prefix.clone() });
    if node.element.shares_3d() {
        for &child_id in &node.children {
            push_plane(scene, child_id, depth, prefix, source, planes);
        }
    }
    prefix.pop();
}

/// Scene-graph placement for one node. Matches [`apply_scene_transform`].
fn scene_view_matrix(node: &SceneNode) -> M44 {
    let tx = node.layout_rect.left + node.transform.translate_x.current_value;
    let ty = node.layout_rect.top + node.transform.translate_y.current_value;
    let rotation = node.transform.rotation.current_value;
    let scale_x = node.transform.scale_x.current_value;
    let scale_y = node.transform.scale_y.current_value;
    let skew_x = node.transform.skew_x.current_value;
    let skew_y = node.transform.skew_y.current_value;
    if rotation == 0.0 && scale_x == 1.0 && scale_y == 1.0 && skew_x == 0.0 && skew_y == 0.0 {
        return M44::translate(tx, ty, 0.0);
    }
    let pivot_x = node.layout_rect.width() * node.transform.pivot_x;
    let pivot_y = node.layout_rect.height() * node.transform.pivot_y;
    let (sin, cos) = rotation.to_radians().sin_cos();
    let rotation_z = M44::row_major(&[
        cos, -sin, 0.0, 0.0, sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    let kx = skew_x.to_radians().tan();
    let ky = skew_y.to_radians().tan();
    let skew = M44::row_major(&[1.0, kx, 0.0, 0.0, ky, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    let mut matrix = M44::translate(tx, ty, 0.0);
    matrix = M44::concat(&matrix, &M44::translate(pivot_x, pivot_y, 0.0));
    matrix = M44::concat(&matrix, &rotation_z);
    matrix = M44::concat(&matrix, &M44::scale(scale_x, scale_y, 1.0));
    matrix = M44::concat(&matrix, &skew);
    M44::concat(&matrix, &M44::translate(-pivot_x, -pivot_y, 0.0))
}

/// Leaf-local point to the sorting parent's space, before that parent's perspective.
fn chain_local_matrix(scene: &SceneGraph, chain: &[NodeId]) -> M44 {
    let mut matrix = M44::new_identity();
    for node in chain.iter().filter_map(|id| scene.get_node(*id)) {
        matrix = M44::concat(&matrix, &scene_view_matrix(node));
        if let Some(local) = node.element.local_view_matrix(node.layout_rect.width(), node.layout_rect.height()) {
            matrix = M44::concat(&matrix, &local);
        }
    }
    matrix
}

/// True when a plane's Z changes across its quad, so the list needs per-pixel occlusion.
pub(super) fn z_varies(scene: &SceneGraph, planes: &[DepthPlane]) -> bool {
    planes.iter().any(|plane| {
        let matrix = chain_local_matrix(scene, &plane.chain);
        matrix.rc(2, 0).abs() > Z_VARY_EPSILON || matrix.rc(2, 1).abs() > Z_VARY_EPSILON
    })
}

/// View Z of the plane point that projects to device `(u, v)`. Larger Z is closer.
fn plane_depth_at(device: &M44, chain: &M44, u: f32, v: f32) -> Option<f32> {
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
    Some(if point.w.abs() < 1.0e-8 { point.z } else { point.z / point.w })
}

/// Same local transform order as the direct paint path of [`Painter::node`].
fn apply_scene_transform(canvas: &Canvas, node: &SceneNode) {
    let pivot_x = node.layout_rect.width() * node.transform.pivot_x;
    let pivot_y = node.layout_rect.height() * node.transform.pivot_y;
    canvas.translate((
        node.layout_rect.left + node.transform.translate_x.current_value,
        node.layout_rect.top + node.transform.translate_y.current_value,
    ));
    canvas.translate((pivot_x, pivot_y));
    canvas.rotate(node.transform.rotation.current_value, None);
    canvas.scale((node.transform.scale_x.current_value, node.transform.scale_y.current_value));
    canvas.skew((
        node.transform.skew_x.current_value.to_radians().tan(),
        node.transform.skew_y.current_value.to_radians().tan(),
    ));
    canvas.translate((-pivot_x, -pivot_y));
}

impl Painter<'_> {
    /// Paint sorted planes in order.
    pub(super) fn sorted_planes(
        &self,
        cache: &mut LayerCache,
        planes: &[DepthPlane],
        canvas: &Canvas,
        opacity: f32,
        depth: usize,
    ) -> Result<(), RenderError> {
        for plane in planes {
            self.depth_chain(cache, &plane.chain, 0, canvas, opacity, depth)?;
        }
        Ok(())
    }

    /// Per-pixel occlusion for planes whose Z changes across the quad.
    ///
    /// Planes stay in center-Z order. A covered fragment replaces the stored one
    /// when its view Z is greater or equal, so an equal-Z tie still follows
    /// z-index and source order.
    pub(super) fn occluded_planes(
        &self,
        cache: &mut LayerCache,
        planes: &[DepthPlane],
        canvas: &Canvas,
        opacity: f32,
        depth: usize,
    ) -> Result<(), RenderError> {
        let layer = canvas.base_layer_size();
        if layer.width <= 0 || layer.height <= 0 {
            return self.sorted_planes(cache, planes, canvas, opacity, depth);
        }
        let (width, height) = (layer.width as usize, layer.height as usize);
        let info = ImageInfo::new((layer.width, layer.height), ColorType::RGBA8888, AlphaType::Unpremul, None);
        let parent_ctm = canvas.local_to_device();
        let mut color = vec![0u8; width * height * 4];
        let mut stored = vec![f32::NEG_INFINITY; width * height];
        let mut pixels = vec![0u8; width * height * 4];
        for plane in planes {
            let chain = chain_local_matrix(self.scene, &plane.chain);
            let device = M44::concat(&parent_ctm, &chain);
            let mut surface = skia_safe::surfaces::raster_n32_premul((layer.width, layer.height))
                .ok_or(RenderError::SurfaceFailure)?;
            let offscreen = surface.canvas();
            offscreen.clear(skia_safe::Color::TRANSPARENT);
            offscreen.set_matrix(&parent_ctm);
            self.depth_chain(cache, &plane.chain, 0, offscreen, opacity, depth)?;
            if !surface.read_pixels(&info, &mut pixels, width * 4, (0, 0)) {
                return Err(RenderError::SurfaceFailure);
            }
            for y in 0..height {
                for x in 0..width {
                    let offset = (y * width + x) * 4;
                    if pixels[offset + 3] < 128 {
                        continue;
                    }
                    let Some(view_z) = plane_depth_at(&device, &chain, x as f32 + 0.5, y as f32 + 0.5) else {
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
        let image = skia_safe::images::raster_from_data(&info, skia_safe::Data::new_copy(&color), width * 4)
            .ok_or(RenderError::SurfaceFailure)?;
        canvas.save();
        canvas.reset_matrix();
        canvas.draw_image(&image, (0, 0), None);
        canvas.restore();
        Ok(())
    }

    fn depth_chain(
        &self,
        cache: &mut LayerCache,
        chain: &[NodeId],
        index: usize,
        canvas: &Canvas,
        opacity: f32,
        depth: usize,
    ) -> Result<(), RenderError> {
        if depth > super::MAX_DEPTH {
            return Err(RenderError::RecursionLimit);
        }
        let Some(node_id) = chain.get(index).copied() else {
            return Ok(());
        };
        if index + 1 == chain.len() {
            // A sharing leaf was hoisted: its descendants are planes of their own.
            let skip = self.scene.get_node(node_id).is_some_and(|node| node.element.shares_3d());
            let draw = if skip { ChildDraw::Skip } else { ChildDraw::Normal };
            return self.node(cache, node_id, canvas, opacity, depth, draw);
        }
        self.enter_preserve_3d(node_id, canvas, opacity, |child_canvas| {
            self.depth_chain(cache, chain, index + 1, child_canvas, opacity, depth + 1)
        })
    }

    /// Run `paint_descendants` in this element's child space without keeping its pixels.
    ///
    /// Descendant planes are sorted outside this element, so its background must
    /// not be redrawn on their pass. `Element::render` still applies the local
    /// matrix, perspective and backface skip. An empty clip drops that pass's paint.
    fn enter_preserve_3d<F>(&self, node_id: NodeId, canvas: &Canvas, opacity: f32, paint_descendants: F) -> Result<(), RenderError>
    where
        F: FnOnce(&Canvas) -> Result<(), RenderError>,
    {
        let Some(node) = self.scene.get_node(node_id) else {
            return Ok(());
        };
        canvas.save();
        apply_scene_transform(canvas, node);
        let local_rect = Rect::from_wh(node.layout_rect.width(), node.layout_rect.height());
        let mut child_matrix = None;
        canvas.save();
        canvas.clip_rect(Rect::new_empty(), ClipOp::Intersect, false);
        let rendered = node.element.render(canvas, local_rect, opacity, &mut |child_canvas| {
            child_matrix = Some(child_canvas.local_to_device());
        });
        canvas.restore();
        let result = rendered.and_then(|()| match child_matrix {
            Some(matrix) => {
                canvas.set_matrix(&matrix);
                paint_descendants(canvas)
            }
            None => Ok(()),
        });
        canvas.restore();
        result
    }
}
