//! Preserve-3d depth sorting.
//!
//! A preserve-3d element sorts one shared list: ascending summed `view_depth`,
//! then z-index, then source order. A direct child that `shares_3d` puts its
//! descendants in that list; a child that does not stays one plane. A list
//! whose Z changes across a quad is occluded per pixel inside the bounds
//! where its planes overlap. Constant Z stays on the sort, so equal Z still
//! follows z-index and source order.

use super::bsp::{self, Eye, Point3};
use super::{ChildDraw, LayerCache, Painter};
use director_core::scene::{SceneGraph, SceneNode};
use director_core::types::NodeId;
use director_core::RenderError;
use skia_safe::{
    AlphaType, Canvas, ClipOp, ColorType, IRect, ImageInfo, Picture, PictureRecorder, Rect, RoundOut, M44,
};

const Z_VARY_EPSILON: f32 = 1.0e-3;
/// Smallest homogeneous W a projected corner keeps.
const W_EPSILON: f32 = 1.0e-4;

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
pub(super) fn scene_view_matrix(node: &SceneNode) -> M44 {
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

/// One preserve-3d plane recorded in device space.
struct RecordedPlane {
    picture: Picture,
    bounds: IRect,
    chain: M44,
    varies: bool,
    /// Corners of the leaf's paint bounds in the sorting parent's space, when
    /// the leaf drew under its chain matrix with every corner before the eye.
    quad: Option<Vec<Point3>>,
}

/// Paint bounds of a plane's leaf: `rect` in its chain space, from `near` to
/// `far` in Z where 3D children it flattens stand out of the plane, and
/// `matrix` from that space to the device.
struct LeafGeometry {
    rect: Rect,
    near: f32,
    far: f32,
    matrix: M44,
}

/// `matrix` maps `(x, y, z)` in front of the eye. `None` behind it.
fn project(matrix: &M44, x: f32, y: f32, z: f32) -> Option<skia_safe::V4> {
    let point = matrix.map(x, y, z, 1.0);
    let finite = point.x.is_finite() && point.y.is_finite() && point.z.is_finite();
    (finite && point.w > W_EPSILON).then(|| skia_safe::V4::new(point.x / point.w, point.y / point.w, point.z / point.w, 1.0))
}

/// The part of `polygon` in front of the eye under `matrix`, keeping each
/// edge's flag. Edges the near plane cuts in carry `false`.
fn clip_near(polygon: &[(Point3, bool)], matrix: &M44) -> Vec<(Point3, bool)> {
    let w = |p: Point3| matrix.rc(3, 0) * p[0] + matrix.rc(3, 1) * p[1] + matrix.rc(3, 2) * p[2] + matrix.rc(3, 3) - W_EPSILON * 2.0;
    let mut out = Vec::with_capacity(polygon.len() + 1);
    for (i, &(a, flag)) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()].0;
        let (wa, wb) = (w(a), w(b));
        let cross = || {
            let t = wa / (wa - wb);
            [0, 1, 2].map(|axis| a[axis] + (b[axis] - a[axis]) * t)
        };
        match (wa > 0.0, wb > 0.0) {
            (true, true) => out.push((a, flag)),
            (true, false) => {
                out.push((a, flag));
                out.push((cross(), false));
            }
            (false, true) => out.push((cross(), flag)),
            (false, false) => {}
        }
    }
    out
}

/// Device points of the part of `polygon` in front of the eye.
fn project_visible(polygon: &[Point3], matrix: &M44) -> Vec<skia_safe::Point> {
    let flagged: Vec<_> = polygon.iter().map(|&p| (p, false)).collect();
    clip_near(&flagged, matrix)
        .iter()
        .filter_map(|(p, _)| project(matrix, p[0], p[1], p[2]).map(|v| skia_safe::Point::new(v.x, v.y)))
        .collect()
}

fn corners(rect: Rect) -> [(f32, f32); 4] {
    [(rect.left, rect.top), (rect.right, rect.top), (rect.right, rect.bottom), (rect.left, rect.bottom)]
}

fn same_matrix(a: &M44, b: &M44) -> bool {
    (0..4).all(|row| {
        (0..4).all(|col| {
            let (x, y) = (a.rc(row, col), b.rc(row, col));
            (x - y).abs() <= 1.0e-3 * x.abs().max(y.abs()).max(1.0)
        })
    })
}

/// The eye of a projection: the point every device ray passes through, which
/// `matrix` sends to zero X, Y and W.
fn eye(matrix: &M44) -> Eye {
    let rows = [0, 1, 3].map(|row| [0, 1, 2, 3].map(|col| matrix.rc(row, col)));
    let minor = |skip: usize| {
        let cols: Vec<usize> = (0..4).filter(|&col| col != skip).collect();
        let m = |r: usize, c: usize| rows[r][cols[c]];
        m(0, 0) * (m(1, 1) * m(2, 2) - m(1, 2) * m(2, 1)) - m(0, 1) * (m(1, 0) * m(2, 2) - m(1, 2) * m(2, 0))
            + m(0, 2) * (m(1, 0) * m(2, 1) - m(1, 1) * m(2, 0))
    };
    let null = [minor(0), -minor(1), minor(2), -minor(3)];
    let reach = null[0].abs().max(null[1].abs()).max(null[2].abs());
    if null[3].abs() > reach * 1.0e-7 {
        return Eye::Point([null[0] / null[3], null[1] / null[3], null[2] / null[3]]);
    }
    // An orthographic view looks down -Z, so the viewer lies toward +Z.
    let sign = if null[2] < 0.0 { -1.0 } else { 1.0 };
    match reach > 0.0 {
        true => Eye::Direction([null[0] * sign, null[1] * sign, null[2] * sign]),
        false => Eye::Direction([0.0, 0.0, 1.0]),
    }
}

/// Plane fragments of `cluster` back to front, each with the device clip its
/// cut edges need. `None` when a plane has no quad or a cut falls behind the eye.
fn split_cluster(
    planes: &[RecordedPlane],
    cluster: &[usize],
    parent_ctm: &M44,
    frame: IRect,
) -> Option<Vec<(usize, Option<skia_safe::Path>)>> {
    let quads: Vec<Vec<Point3>> = cluster.iter().map(|&index| planes[index].quad.clone()).collect::<Option<_>>()?;
    let fragments = bsp::back_to_front(&quads, eye(parent_ctm))?;
    let reach = (frame.width().max(frame.height()) * 4) as f32;
    let outer = Rect::from_irect(frame).with_outset((reach, reach));
    fragments
        .into_iter()
        .map(|fragment| {
            let plane = cluster[fragment.plane];
            if !fragment.split.contains(&true) {
                return Some((plane, None));
            }
            let flagged: Vec<_> = fragment.points.iter().copied().zip(fragment.split.iter().copied()).collect();
            let visible = clip_near(&flagged, parent_ctm);
            if visible.len() < 3 {
                // Wholly behind the eye: nothing of it shows.
                return Some((plane, Some(skia_safe::Path::new())));
            }
            let device: Vec<skia_safe::Point> = visible
                .iter()
                .map(|(p, _)| project(parent_ctm, p[0], p[1], p[2]).map(|v| skia_safe::Point::new(v.x, v.y)))
                .collect::<Option<_>>()?;
            let cuts: Vec<bool> = visible.iter().map(|vertex| vertex.1).collect();
            let count = device.len() as f32;
            let center = device.iter().fold(skia_safe::Point::default(), |sum, p| sum + *p) * (1.0 / count);
            // Only cut edges clip; the quad's own edges lie outside what the plane paints.
            let mut clip = corners(outer).map(|(x, y)| skia_safe::Point::new(x, y)).to_vec();
            for (i, &cut) in cuts.iter().enumerate() {
                let (a, b) = (device[i], device[(i + 1) % device.len()]);
                let normal = skia_safe::Point::new(a.y - b.y, b.x - a.x);
                let inside = normal.dot(center - a);
                if !cut || normal.length() < 1.0e-4 || inside.abs() < 1.0e-6 {
                    continue;
                }
                clip = clip_half_plane(&clip, a, normal * inside.signum());
            }
            Some((plane, Some(skia_safe::Path::polygon(&clip, true, None, None))))
        })
        .collect()
}

/// The plane's picture rendered once into an image covering its bounds, on
/// the backend of `canvas`.
fn rasterize(plane: &RecordedPlane, canvas: &Canvas) -> Option<skia_safe::Image> {
    let info = ImageInfo::new_n32_premul((plane.bounds.width(), plane.bounds.height()), None);
    let mut surface = canvas.new_surface(&info, None)?;
    let target = surface.canvas();
    target.translate((-plane.bounds.left as f32, -plane.bounds.top as f32));
    target.draw_picture(&plane.picture, None, None);
    Some(surface.image_snapshot())
}

/// Sutherland-Hodgman: keeps the part of convex `polygon` where
/// `normal . (p - origin) >= 0`.
fn clip_half_plane(polygon: &[skia_safe::Point], origin: skia_safe::Point, normal: skia_safe::Point) -> Vec<skia_safe::Point> {
    let side = |p: skia_safe::Point| normal.dot(p - origin);
    let mut out = Vec::with_capacity(polygon.len() + 1);
    for (i, &a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        let (da, db) = (side(a), side(b));
        if da >= 0.0 {
            out.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            out.push(a + (b - a) * (da / (da - db)));
        }
    }
    out
}

/// Groups planes whose device bounds overlap, directly or through another
/// plane. Each cluster lists plane indices in sorted order.
fn clusters(planes: &[RecordedPlane]) -> Vec<Vec<usize>> {
    fn root(parent: &mut [usize], mut index: usize) -> usize {
        while parent[index] != index {
            parent[index] = parent[parent[index]];
            index = parent[index];
        }
        index
    }
    let mut parent: Vec<usize> = (0..planes.len()).collect();
    for a in 0..planes.len() {
        for b in a + 1..planes.len() {
            if IRect::intersects(&planes[a].bounds, &planes[b].bounds) {
                let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut slot = vec![usize::MAX; planes.len()];
    for index in 0..planes.len() {
        let group = root(&mut parent, index);
        if slot[group] == usize::MAX {
            slot[group] = groups.len();
            groups.push(Vec::new());
        }
        groups[slot[group]].push(index);
    }
    groups
}

/// Per-pixel depth composite of one cluster inside the union of its bounds.
/// `canvas` draws in device space.
fn composite_cluster(planes: &[RecordedPlane], cluster: &[usize], parent_ctm: &M44, canvas: &Canvas) -> Result<(), RenderError> {
    let mut area = planes[cluster[0]].bounds;
    for &index in &cluster[1..] {
        area = IRect::join(&area, &planes[index].bounds);
    }
    let (width, height) = (area.width() as usize, area.height() as usize);
    let mut color = vec![0u8; width * height * 4];
    let mut stored = vec![f32::NEG_INFINITY; width * height];
    let mut pixels = Vec::new();
    for &index in cluster {
        let plane = &planes[index];
        let bounds = plane.bounds;
        let (plane_width, plane_height) = (bounds.width() as usize, bounds.height() as usize);
        let info = ImageInfo::new((bounds.width(), bounds.height()), ColorType::RGBA8888, AlphaType::Premul, None);
        let mut surface = skia_safe::surfaces::raster(&info, None, None).ok_or(RenderError::SurfaceFailure)?;
        let offscreen = surface.canvas();
        offscreen.clear(skia_safe::Color::TRANSPARENT);
        offscreen.translate((-bounds.left as f32, -bounds.top as f32));
        offscreen.draw_picture(&plane.picture, None, None);
        pixels.resize(plane_width * plane_height * 4, 0);
        if !surface.read_pixels(&info, &mut pixels, plane_width * 4, (0, 0)) {
            return Err(RenderError::SurfaceFailure);
        }
        let device = M44::concat(parent_ctm, &plane.chain);
        let (left, top) = ((bounds.left - area.left) as usize, (bounds.top - area.top) as usize);
        for y in 0..plane_height {
            let v = (bounds.top + y as i32) as f32 + 0.5;
            for x in 0..plane_width {
                let source = (y * plane_width + x) * 4;
                if pixels[source + 3] == 0 {
                    continue;
                }
                let u = (bounds.left + x as i32) as f32 + 0.5;
                let Some(view_z) = plane_depth_at(&device, &plane.chain, u, v) else {
                    continue;
                };
                let index = (y + top) * width + x + left;
                let offset = index * 4;
                let src: [u8; 4] = pixels[source..source + 4].try_into().unwrap();
                let dst: [u8; 4] = color[offset..offset + 4].try_into().unwrap();
                let (front, back) = if view_z >= stored[index] {
                    stored[index] = view_z;
                    (src, dst)
                } else {
                    (dst, src)
                };
                // Premultiplied source-over.
                for c in 0..4 {
                    let rest = back[c] as u32 * (255 - front[3] as u32);
                    color[offset + c] = (front[c] as u32 + (rest + 127) / 255).min(255) as u8;
                }
            }
        }
    }
    let info = ImageInfo::new((area.width(), area.height()), ColorType::RGBA8888, AlphaType::Premul, None);
    let image = skia_safe::images::raster_from_data(&info, skia_safe::Data::new_copy(&color), width * 4)
        .ok_or(RenderError::SurfaceFailure)?;
    canvas.draw_image(&image, (area.left, area.top), None);
    Ok(())
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
            self.depth_chain(cache, &plane.chain, canvas, opacity, depth, &mut None)?;
        }
        Ok(())
    }

    /// Per-pixel occlusion for planes whose Z changes across the quad.
    ///
    /// Each plane records in device space. Its bounds are the leaf's paint
    /// bounds projected through the matrix it drew under, intersected with the
    /// picture's cull rect, which ignores clips. Planes whose bounds overlap
    /// form a cluster. A lone plane or a cluster of constant-Z planes draws in
    /// sorted order straight onto `canvas`. Any other cluster splits along a
    /// BSP tree of the planes' quads and draws its fragments back to front,
    /// each clipped to its cut edges, so the work stays on the canvas backend.
    /// A cluster with a plane of unknown geometry composites per pixel inside
    /// its own bounds.
    ///
    /// Planes stay in center-Z order. A fragment whose view Z is greater or
    /// equal to the nearest one stored so far composites over the pixel and
    /// becomes the nearest, so an equal-Z tie still follows z-index and source
    /// order; a farther fragment composites under it. Translucent and
    /// antialiased fragments keep their coverage instead of being dropped.
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
        let frame = IRect::from_wh(layer.width, layer.height);
        let parent_ctm = canvas.local_to_device();
        let mut recorded = Vec::with_capacity(planes.len());
        for plane in planes {
            let mut recorder = PictureRecorder::new();
            let recording = recorder.begin_recording(Rect::from_irect(frame), true);
            recording.set_matrix(&parent_ctm);
            let mut leaf_parent = None;
            self.depth_chain(cache, &plane.chain, recording, opacity, depth, &mut leaf_parent)?;
            let Some(picture) = recorder.finish_recording_as_picture(None) else {
                continue;
            };
            // A bounding hierarchy trims the cull rect to what the plane draws.
            let cull = picture.cull_rect();
            if cull.is_empty() {
                continue;
            }
            let chain = chain_local_matrix(self.scene, &plane.chain);
            let geometry = leaf_parent.and_then(|parent| self.leaf_geometry(&plane.chain, &parent));
            // The faces of the leaf's box, clipped to the part before the eye, bound it.
            let projected = geometry.as_ref().map(|leaf| {
                let [a, b, c, d] = corners(leaf.rect);
                let box_faces = [[a, b, c, d], [a, b, b, a], [b, c, c, b], [c, d, d, c], [d, a, a, d]];
                let points: Vec<_> = box_faces
                    .iter()
                    .enumerate()
                    .filter(|(face, _)| *face == 0 || leaf.near != leaf.far)
                    .flat_map(|(face, quad)| {
                        // Side faces join the near and far corners.
                        let polygon: Vec<Point3> = match face {
                            0 => quad.iter().map(|&(x, y)| [x, y, leaf.near]).collect(),
                            _ => vec![[quad[0].0, quad[0].1, leaf.near], [quad[1].0, quad[1].1, leaf.near], [quad[2].0, quad[2].1, leaf.far], [quad[3].0, quad[3].1, leaf.far]],
                        };
                        let mut points = project_visible(&polygon, &leaf.matrix);
                        if face == 0 && leaf.near != leaf.far {
                            let far: Vec<Point3> = polygon.iter().map(|p| [p[0], p[1], leaf.far]).collect();
                            points.extend(project_visible(&far, &leaf.matrix));
                        }
                        points
                    })
                    .collect();
                Rect::bounds(&points)
            });
            let mut bounds = match projected {
                Some(Some(mut projected)) => {
                    if !projected.intersect(cull) {
                        continue;
                    }
                    projected
                }
                // Wholly behind the eye.
                Some(None) => continue,
                None => cull,
            };
            let quad = geometry.as_ref()
                .filter(|leaf| same_matrix(&leaf.matrix, &M44::concat(&parent_ctm, &chain)))
                .and_then(|leaf| corners(leaf.rect).iter().map(|&(x, y)| project(&chain, x, y, 0.0).map(|p| [p.x, p.y, p.z])).collect());
            // One pixel of margin covers antialiased edges.
            bounds.outset((1.0, 1.0));
            let Some(bounds) = IRect::intersect(&bounds.round_out(), &frame) else {
                continue;
            };
            let varies = chain.rc(2, 0).abs() > Z_VARY_EPSILON || chain.rc(2, 1).abs() > Z_VARY_EPSILON;
            recorded.push(RecordedPlane { picture, bounds, chain, varies, quad });
        }
        canvas.save();
        canvas.reset_matrix();
        let result = clusters(&recorded).into_iter().try_for_each(|cluster| {
            if cluster.len() == 1 || cluster.iter().all(|&index| !recorded[index].varies) {
                for &index in &cluster {
                    canvas.draw_picture(&recorded[index].picture, None, None);
                }
                return Ok(());
            }
            match split_cluster(&recorded, &cluster, &parent_ctm, frame) {
                Some(fragments) => {
                    // A plane cut into several fragments rasterizes once and each fragment blits it.
                    let mut pieces = vec![0usize; recorded.len()];
                    for (index, _) in &fragments {
                        pieces[*index] += 1;
                    }
                    let mut images: Vec<Option<skia_safe::Image>> = vec![None; recorded.len()];
                    for (index, clip) in fragments {
                        let plane = &recorded[index];
                        if pieces[index] > 1 && images[index].is_none() {
                            images[index] = rasterize(plane, canvas);
                        }
                        canvas.save();
                        if let Some(clip) = clip {
                            canvas.clip_path(&clip, ClipOp::Intersect, false);
                        }
                        match &images[index] {
                            Some(image) => {
                                canvas.draw_image(image, (plane.bounds.left as f32, plane.bounds.top as f32), None);
                            }
                            None => {
                                canvas.draw_picture(&plane.picture, None, None);
                            }
                        }
                        canvas.restore();
                    }
                    Ok(())
                }
                None => composite_cluster(&recorded, &cluster, &parent_ctm, canvas),
            }
        });
        canvas.restore();
        result
    }

    fn depth_chain(
        &self,
        cache: &mut LayerCache,
        chain: &[NodeId],
        canvas: &Canvas,
        opacity: f32,
        depth: usize,
        leaf_parent: &mut Option<M44>,
    ) -> Result<(), RenderError> {
        if depth > super::MAX_DEPTH {
            return Err(RenderError::RecursionLimit);
        }
        let Some((&node_id, rest)) = chain.split_first() else {
            return Ok(());
        };
        if rest.is_empty() {
            // A sharing leaf was hoisted: its descendants are planes of their own.
            let skip = self.scene.get_node(node_id).is_some_and(|node| node.element.shares_3d());
            let draw = if skip { ChildDraw::Skip } else { ChildDraw::Normal };
            *leaf_parent = Some(canvas.local_to_device());
            return self.node(cache, node_id, canvas, opacity, depth, draw);
        }
        self.enter_preserve_3d(node_id, canvas, opacity, |child_canvas| {
            self.depth_chain(cache, rest, child_canvas, opacity, depth + 1, leaf_parent)
        })
    }

    /// Paint bounds of a plane's leaf drawn under `parent`, the matrix before
    /// its placement. `None` when the leaf's paint bounds are unknown.
    fn leaf_geometry(&self, chain: &[NodeId], parent: &M44) -> Option<LeafGeometry> {
        let node = self.scene.get_node(*chain.last()?)?;
        if node.filter.has_effects() {
            return None;
        }
        let (width, height) = (node.layout_rect.width(), node.layout_rect.height());
        let (mut children, mut near, mut far) = (Rect::new_empty(), 0.0f32, 0.0f32);
        // A hoisted sharing leaf paints none of its children.
        if !node.element.shares_3d() {
            for &child_id in &node.children {
                let (rect, z) = self.child_box(child_id)?;
                children.join(rect);
                near = near.min(z.0);
                far = far.max(z.1);
            }
        }
        let mut rect = node.element.paint_bounds(Rect::from_wh(width, height), Some(children))?;
        let mut matrix = M44::concat(parent, &scene_view_matrix(node));
        match node.element.local_view_matrix(width, height) {
            Some(view) => matrix = M44::concat(&matrix, &view),
            None => {
                if let Some(transform) = node.element.local_transform(width, height) {
                    if near != 0.0 || far != 0.0 {
                        return None;
                    }
                    rect = transform.map_rect(rect).0;
                }
            }
        }
        Some(LeafGeometry { rect, near, far, matrix })
    }

    /// Bounds of a child subtree in its parent's space with the Z range it
    /// reaches. Skia draws a 3D child of a flat parent with its full matrix, so
    /// the child stands out of the parent's plane by that range.
    fn child_box(&self, id: NodeId) -> Option<(Rect, (f32, f32))> {
        let node = self.scene.get_node(id)?;
        let (width, height) = (node.layout_rect.width(), node.layout_rect.height());
        let Some(view) = node.element.local_view_matrix(width, height) else {
            return Some((super::placement(node).map_rect(self.subtree_bounds(id, 1)?).0, (0.0, 0.0)));
        };
        if node.element.shares_3d() || node.filter.has_effects() {
            return None;
        }
        let mut children = Rect::new_empty();
        for &child_id in &node.children {
            let child = self.scene.get_node(child_id)?;
            children.join(super::placement(child).map_rect(self.subtree_bounds(child_id, 2)?).0);
        }
        let inner = node.element.paint_bounds(Rect::from_wh(width, height), Some(children))?;
        let matrix = M44::concat(&scene_view_matrix(node), &view);
        let mut points = Vec::with_capacity(4);
        let (mut near, mut far) = (0.0f32, 0.0f32);
        for (x, y) in corners(inner) {
            let point = matrix.map(x, y, 0.0, 1.0);
            // A projective child has no box in its parent's space.
            if (point.w - 1.0).abs() > 1.0e-4 || !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
                return None;
            }
            points.push(skia_safe::Point::new(point.x, point.y));
            near = near.min(point.z);
            far = far.max(point.z);
        }
        Some((Rect::bounds(&points)?, (near, far)))
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
