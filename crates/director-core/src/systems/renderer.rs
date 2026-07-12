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
fn render_node_subtree(
    scene: &SceneGraph,
    assets: &AssetManager,
    static_cache: &mut HashMap<NodeId, skia_safe::Image>,
    node_id: NodeId,
    canvas: &skia_safe::Canvas,
    parent_opacity: f32,
    depth: usize,
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
                    render_recursive(scene, assets, static_cache, mask_id, canvas, 1.0, depth + 1)
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
pub fn render_recursive(
    scene: &SceneGraph,
    assets: &AssetManager,
    static_cache: &mut HashMap<NodeId, skia_safe::Image>,
    node_id: NodeId,
    canvas: &skia_safe::Canvas,
    parent_opacity: f32,
    depth: usize,
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
        // Static cache hit: draw the cached image at the node's position.
        if let Some(image) = static_cache.get(&node_id) {
            canvas.save();
            canvas.translate((layout_x + tx, layout_y + ty));
            canvas.draw_image(image, (0, 0), None);
            canvas.restore();
            return Ok(());
        }

        // Cache miss: render the subtree to an offscreen surface and cache it.
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

    for (id, node_opt) in scene.nodes.iter().enumerate() {
        let Some(node) = node_opt else { continue };
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
fn render_scene_to_canvas(
    director: &mut Director,
    time: f64,
    canvas: &skia_safe::Canvas,
) -> Result<(), RenderError> {
    let assets = &director.assets;
    canvas.clear(skia_safe::Color::BLACK);

    let has_tracks = director.tracks.iter().any(|t| !t.clips.is_empty());
    if has_tracks {
        let mut clips: Vec<(usize, usize, &TrackClip)> = Vec::new();
        for (ti, track) in director.tracks.iter().enumerate() {
            for (ci, clip) in track.clips.iter().enumerate() {
                if track.enabled
                    && time >= clip.start_time
                    && time < clip.start_time + clip.duration
                {
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
            )?;
        }
    } else {
        let mut items: Vec<(usize, TimelineItem)> = director
            .timeline
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, item)| time >= item.start_time && time < item.start_time + item.duration)
            .collect();
        items.sort_by_key(|(_, item)| item.z_index);

        for (_, item) in items {
            render_recursive(
                &director.scene,
                assets,
                &mut director.static_cache,
                item.scene_root,
                canvas,
                1.0,
                0,
            )?;
        }
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
        director.update(time);
        layout_engine.compute_layout(&mut director.scene, director.width, director.height, time);
        director.run_post_layout(time);
        render_scene_to_canvas(director, time, canvas)?;
        return Ok(());
    }

    // Motion blur: sample across the shutter duration and accumulate.
    let fps = director.fps as f64;
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

        director.update(sample_time);
        layout_engine.compute_layout(
            &mut director.scene,
            director.width,
            director.height,
            sample_time,
        );
        director.run_post_layout(sample_time);

        render_scene_to_canvas(director, sample_time, scratch_surface.canvas())?;

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
    director.update(time);
    layout_engine.compute_layout(&mut director.scene, director.width, director.height, time);
    director.run_post_layout(time);

    let assets_ref = &director.assets;

    canvas.clear(skia_safe::Color::BLACK);

    // Check transition
    let transition = director
        .transitions
        .iter()
        .find(|t| time >= t.start_time && time < t.start_time + t.duration)
        .cloned();

    // Collect items. Need indices to match with transition.
    let mut items: Vec<(usize, TimelineItem)> = director
        .timeline
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
                            director.timeline.get(trans.from_scene_idx),
                            director.timeline.get(trans.to_scene_idx),
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
                            )?;
                            render_recursive(
                                &director.scene,
                                assets_ref,
                                &mut director.static_cache,
                                item_b.scene_root,
                                surf_b.canvas(),
                                1.0,
                                0,
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
                )?;
            }
        }
    } else {
        for (_, item) in items {
            render_recursive(
                &director.scene,
                assets_ref,
                &mut director.static_cache,
                item.scene_root,
                canvas,
                1.0,
                0,
            )?;
        }
    }
    if time < 0.1 {
        debug!("[Frame] render complete");
    }
    Ok(())
}
