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

use crate::director::{Director, TimelineItem};
use crate::errors::RenderError;
use crate::scene::SceneGraph;
use crate::systems::assets::AssetManager;
use crate::systems::layout::LayoutEngine;
use crate::systems::transitions::draw_transition;
use crate::timeline::TrackClip;
use crate::types::NodeId;
use std::collections::HashMap;
use tracing::debug;

#[cfg(feature = "vulkan")]
use skia_safe::gpu::DirectContext;

#[cfg(feature = "vulkan")]
/// Type alias for the GPU context (Vulkan backend).
pub type GpuContext = DirectContext;
#[cfg(not(feature = "vulkan"))]
/// Placeholder for GPU context when Vulkan feature is disabled.
pub type GpuContext = ();

/// Renders a node's element and children to the given canvas. The caller is
/// responsible for setting up the canvas transform.
///
/// If `dirty_nodes` is `Some`, only nodes in the set are rendered; this is
/// used after `Director::invalidate()` to incrementally re-render changed nodes.
fn render_node_subtree(
    scene: &SceneGraph,
    assets: &AssetManager,
    static_cache: &mut HashMap<NodeId, skia_safe::Image>,
    node_id: NodeId,
    canvas: &skia_safe::Canvas,
    parent_opacity: f32,
    depth: usize,
    dirty_nodes: Option<&std::collections::HashSet<NodeId>>,
) -> Result<(), RenderError> {
    let node = match scene.get_node(node_id) {
        Some(n) => n,
        None => return Ok(()),
    };

    let local_rect = skia_safe::Rect::from_wh(node.layout_rect.width(), node.layout_rect.height());

    let mut last_error = Ok(());
    let mut draw_children = |canvas: &skia_safe::Canvas| {
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
/// Handles transformation stack, blending modes, masking, and static-subtree
/// caching. When a node is marked `is_static`, its rasterized output is looked
/// up in `static_cache`; on a miss the subtree is rendered to an offscreen
/// surface, stored, and then drawn.
///
/// If `dirty_nodes` is `Some`, only nodes in the set are rendered. Nodes not
/// in the set are skipped entirely, including their children.
pub fn render_recursive(
    scene: &SceneGraph,
    assets: &AssetManager,
    static_cache: &mut HashMap<NodeId, skia_safe::Image>,
    node_id: NodeId,
    canvas: &skia_safe::Canvas,
    parent_opacity: f32,
    depth: usize,
    dirty_nodes: Option<&std::collections::HashSet<NodeId>>,
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
        )
    };

    let tan_skew_x = skew_x.to_radians().tan();
    let tan_skew_y = skew_y.to_radians().tan();

    if is_static {
        let is_dirty = dirty_nodes.map(|d| d.contains(&node_id)).unwrap_or(true);

        // Cache hit only when the node is not dirty. Dirty static nodes must
        // re-rasterize so mutations never serve stale pixels (plan 03).
        if !is_dirty {
            if let Some(image) = static_cache.get(&node_id) {
                canvas.save();
                canvas.translate((layout_x + tx, layout_y + ty));
                canvas.draw_image(image, (0, 0), None);
                canvas.restore();
                return Ok(());
            }
            // No cache entry and not dirty: nothing to draw this pass (parent
            // full frame or prior full pass should have filled the cache).
            // Fall through to rasterize so the first full frame still works.
        } else {
            // Drop any stale entry before re-rasterizing.
            static_cache.remove(&node_id);
        }

        // Cache miss or dirty: render the subtree to an offscreen surface and cache it.
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
        )?;

        offscreen.restore();

        let image = surface.image_snapshot();
        static_cache.insert(node_id, image.clone());

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

                            draw_transition(
                                canvas,
                                &img_a,
                                &img_b,
                                val,
                                &trans.kind,
                                director.width,
                                director.height,
                            );
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
        ) {
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
