//! Per-frame evaluation and paint.
//!
//! [`evaluate`] advances the `Director` to a time and resolves what the frame
//! draws into a [`FramePlan`]. [`paint`] draws a plan, and [`FrameState::render`]
//! averages evaluated sub-frames across the shutter for motion blur.

use crate::paint::{debug_overlays, Painter};
use crate::RenderError;
use director_core::systems::layout::LayoutEngine;
use director_core::systems::transitions::{draw_transition, Transition, TransitionType};
use director_core::types::NodeId;
use director_core::Director;
use skia_safe::{BlendMode, Canvas, Color, Paint, Surface};
use std::collections::HashSet;

/// What one evaluated frame draws, bottom layer first.
struct FramePlan {
    layers: Vec<Layer>,
    /// Nodes repainted by an incremental frame; `None` repaints everything.
    dirty: Option<HashSet<NodeId>>,
}

enum Layer {
    Scene { root: NodeId, opacity: f32 },
    Transition { from: NodeId, to: NodeId, progress: f32, kind: TransitionType },
}

/// Layout and scratch rasters reused across frames of one live `Director`.
/// Static layers live on the `Director` in `static_cache`.
#[derive(Default)]
pub struct FrameState {
    layout: LayoutEngine,
    accumulation: Option<Surface>,
    transitions: Option<(Surface, Surface)>,
}

/// Paint the frame of `director` at `time` seconds into `target` with fresh
/// frame state. Hold a [`FrameState`] to paint a sequence.
pub fn render_frame(director: &mut Director, time: f64, target: &mut Surface) -> Result<(), RenderError> {
    FrameState::default().render(director, time, target)
}

/// Paint the subtree at `root` of `director`'s scene onto `canvas` under its
/// current matrix, reusing and filling `director.static_cache`. The scene must
/// already be updated and laid out.
pub fn paint_root(director: &mut Director, root: NodeId, canvas: &Canvas, opacity: f32) -> Result<(), RenderError> {
    let painter = Painter { scene: &director.scene, dirty: None };
    Ok(painter.root(&mut director.static_cache, root, canvas, opacity)?)
}

/// Advance `director` to `time`, lay it out and resolve the layers it draws.
fn evaluate(director: &mut Director, layout: &mut LayoutEngine, time: f64) -> FramePlan {
    // Without a compiled dependency graph every frame repaints in full.
    let dirty = if director.dirty_nodes.is_empty() || !director.is_compiled() {
        None
    } else {
        Some(director.expand_dirty_set(&director.dirty_nodes))
    };
    director.update(time, dirty.as_ref());
    layout.compute_layout(&mut director.scene, director.width, director.height, time);
    director.run_post_layout(time);
    director.dirty_nodes.clear();

    let transition = director
        .transitions
        .iter()
        .find(|t| time >= t.start_time && time < t.start_time + t.duration)
        .cloned();
    let layers = match transition {
        Some(transition) => transition_layers(director, time, transition),
        None => track_layers(director, time),
    };
    FramePlan { layers, dirty }
}

/// Timeline items in z order, with the transition pair drawn once at the
/// position of whichever of its scenes comes first.
fn transition_layers(director: &Director, time: f64, transition: Transition) -> Vec<Layer> {
    let flat = director.flat_timeline();
    let mut items: Vec<_> = flat
        .iter()
        .enumerate()
        .filter(|(_, item)| time >= item.start_time && time < item.start_time + item.duration)
        .collect();
    items.sort_by_key(|(_, item)| item.z_index);
    let pair = flat.get(transition.from_scene_idx).zip(flat.get(transition.to_scene_idx));
    let progress = ((time - transition.start_time) / transition.duration).clamp(0.0, 1.0) as f32;
    let pair_indices = [transition.from_scene_idx, transition.to_scene_idx];
    let mut pending = Some(transition);
    let mut layers = Vec::with_capacity(items.len());
    for (index, item) in items {
        if pair_indices.contains(&index) {
            if let Some((transition, (from, to))) = pending.take().zip(pair) {
                layers.push(Layer::Transition {
                    from: from.scene_root,
                    to: to.scene_root,
                    progress: transition.easing.eval(progress),
                    kind: transition.kind,
                });
            }
        } else {
            layers.push(Layer::Scene { root: item.scene_root, opacity: 1.0 });
        }
    }
    layers
}

/// Active clips of enabled tracks in z order, under track opacity and clip fades.
fn track_layers(director: &Director, time: f64) -> Vec<Layer> {
    let mut clips: Vec<_> = director
        .tracks
        .iter()
        .filter(|track| track.enabled)
        .flat_map(|track| {
            track
                .clips
                .iter()
                .filter(|clip| clip.is_active_at(time))
                .map(move |clip| (track.opacity, clip))
        })
        .collect();
    clips.sort_by_key(|(_, clip)| clip.z_index);
    clips
        .into_iter()
        .map(|(track_opacity, clip)| Layer::Scene {
            root: clip.scene_root,
            opacity: track_opacity * clip.fade_opacity(time),
        })
        .collect()
}

/// The colour every frame starts from: transparent for alpha export, opaque black otherwise.
fn clear_color(director: &Director) -> Color {
    if director.transparent_background {
        Color::TRANSPARENT
    } else {
        Color::BLACK
    }
}

/// Map layout pixels onto the output surface. Scale 1 leaves the matrix alone.
fn begin_scene_canvas(canvas: &Canvas, (sx, sy): (f32, f32)) {
    if sx != 1.0 || sy != 1.0 {
        canvas.reset_matrix();
        canvas.scale((sx, sy));
    }
}

/// Draw `plan` onto `target`, reusing static layers from `director.static_cache`.
/// Transition rasters match `target` and are allocated on first use.
fn paint(
    director: &mut Director,
    plan: &FramePlan,
    target: &mut Surface,
    transitions: &mut Option<(Surface, Surface)>,
) -> Result<(), RenderError> {
    if transitions.is_none() && plan.layers.iter().any(|layer| matches!(layer, Layer::Transition { .. })) {
        let info = target.image_info();
        let mut raster = || target.new_surface(&info).ok_or(RenderError::Surface(info.width(), info.height()));
        *transitions = Some((raster()?, raster()?));
    }
    let scale = (director.output_scale_x, director.output_scale_y);
    target.canvas().clear(clear_color(director));
    begin_scene_canvas(target.canvas(), scale);
    let painter = Painter { scene: &director.scene, dirty: plan.dirty.as_ref() };
    let cache = &mut director.static_cache;
    for layer in &plan.layers {
        match layer {
            Layer::Scene { root, opacity } => painter.root(cache, *root, target.canvas(), *opacity)?,
            Layer::Transition { from, to, progress, kind } => {
                let (a, b) = transitions.as_mut().unwrap();
                for (surface, root) in [(&mut *a, *from), (&mut *b, *to)] {
                    surface.canvas().clear(Color::TRANSPARENT);
                    begin_scene_canvas(surface.canvas(), scale);
                    painter.root(cache, root, surface.canvas(), 1.0)?;
                }
                let (from_image, to_image) = (a.image_snapshot(), b.image_snapshot());
                let canvas = target.canvas();
                let scaled = scale != (1.0, 1.0);
                let (width, height) = if scaled {
                    canvas.save();
                    canvas.reset_matrix();
                    (
                        (director.width as f32 * scale.0).round() as i32,
                        (director.height as f32 * scale.1).round() as i32,
                    )
                } else {
                    (director.width, director.height)
                };
                draw_transition(canvas, &from_image, &to_image, *progress, kind, width, height);
                if scaled {
                    canvas.restore();
                }
            }
        }
    }
    if director.debug_overlays {
        debug_overlays(&director.scene, target.canvas());
    }
    Ok(())
}

impl FrameState {
    /// Evaluate and paint the frame at `time` into `target`. With more than one
    /// sample per frame, sub-frames spread across the shutter add up at equal
    /// weight. Scratch rasters come from `target`, so they share its backend.
    pub fn render(&mut self, director: &mut Director, time: f64, target: &mut Surface) -> Result<(), RenderError> {
        let samples = director.samples_per_frame.max(1);
        if samples == 1 {
            let plan = evaluate(director, &mut self.layout, time);
            return paint(director, &plan, target, &mut self.transitions);
        }
        if self.accumulation.is_none() {
            let info = target.image_info();
            self.accumulation =
                Some(target.new_surface(&info).ok_or(RenderError::Surface(info.width(), info.height()))?);
        }
        let sample_target = self.accumulation.as_mut().unwrap();
        let shutter = (1.0 / director.fps) * (director.shutter_angle.clamp(0.0, 360.0) as f64 / 360.0);
        let shutter_start = time - shutter / 2.0;
        let mut weight = Paint::default();
        weight.set_alpha_f(1.0 / samples as f32);
        weight.set_blend_mode(BlendMode::Plus);
        target.canvas().clear(clear_color(director));
        for sample in 0..samples {
            let sample_time = shutter_start + (sample as f64 / (samples - 1) as f64) * shutter;
            let plan = evaluate(director, &mut self.layout, sample_time);
            paint(director, &plan, sample_target, &mut self.transitions)?;
            target.canvas().draw_image(sample_target.image_snapshot(), (0, 0), Some(&weight));
        }
        Ok(())
    }
}
