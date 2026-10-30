//! Per-frame evaluation and paint.
//!
//! [`evaluate`] advances the `Director` to a time and resolves what the frame
//! draws into a [`FramePlan`]. [`paint`] draws a plan, and [`FrameState::render`]
//! averages evaluated sub-frames across the shutter for motion blur.

use crate::paint::{box_placements, debug_overlays, paint_travels, subtree_moves, Painter};
use crate::RenderError;
use director_core::systems::layout::LayoutEngine;
use director_core::systems::transitions::{draw_transition, Transition, TransitionType};
use director_core::types::NodeId;
use director_core::Director;
use crate::paint::BoxPlacement;
use skia_safe::{BlendMode, Canvas, Color, ColorType, Contains, Matrix, Paint, Point, Rect, Surface, M44};
use std::collections::{HashMap, HashSet};

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
    /// The sub-frame raster and the half-float sum the sub-frames add into.
    accumulation: Option<(Surface, Surface)>,
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

/// True when anything the frame draws may change between `t0` and `t1`: a
/// transition, a clip starting, ending or fading, or a moving node in an
/// active clip at that clip's local times.
fn moves_between(director: &Director, t0: f64, t1: f64) -> bool {
    let overlaps = |start: f64, end: f64| t0 < end && t1 >= start;
    if director.transitions.iter().any(|t| overlaps(t.start_time, t.start_time + t.duration)) {
        return true;
    }
    let clips = director.tracks.iter().filter(|track| track.enabled).flat_map(|track| &track.clips);
    clips.into_iter().any(|clip| {
        let (start, end) = (clip.start_time, clip.end_time());
        if !overlaps(start, end) {
            return false;
        }
        t0 < start
            || t1 >= end
            || (clip.fade_in > 0.0 && t0 < start + clip.fade_in)
            || (clip.fade_out > 0.0 && t1 > end - clip.fade_out)
            || subtree_moves(&director.scene, clip.scene_root, clip.local_time(t0), clip.local_time(t1))
    })
}

/// Device pixels between neighbouring motion blur samples of the box that
/// travels furthest across the shutter.
const SAMPLE_SPACING: f32 = 2.5;

/// Nodes of active clips whose paint may move inside their box over
/// `[t0, t1]`, with that travel in local pixels. `None` when a transition
/// runs or a clip starts or ends, which every sample must show.
fn clip_paint_travels(director: &Director, t0: f64, t1: f64) -> Option<Vec<(NodeId, Option<f32>)>> {
    if director.transitions.iter().any(|t| t0 < t.start_time + t.duration && t1 >= t.start_time) {
        return None;
    }
    let mut travels = Vec::new();
    let clips = director.tracks.iter().filter(|track| track.enabled).flat_map(|track| &track.clips);
    for clip in clips.filter(|clip| t0 < clip.end_time() && t1 >= clip.start_time) {
        if t0 < clip.start_time || t1 >= clip.end_time() {
            return None;
        }
        paint_travels(&director.scene, clip.scene_root, clip.local_time(t0), clip.local_time(t1), &mut travels);
    }
    Some(travels)
}

/// Device pixels one local pixel spans at the centre of a placed box.
fn device_scale(p: &BoxPlacement) -> Option<f32> {
    let (x, y) = (p.size.0 / 2.0, p.size.1 / 2.0);
    let at = |x: f32, y: f32| {
        let q = p.matrix.map(x, y, 0.0, 1.0);
        (q.w > f32::EPSILON).then(|| Point::new(q.x / q.w, q.y / q.w))
    };
    let centre = at(x, y)?;
    Some((at(x + 1.0, y)? - centre).length().max((at(x, y + 1.0)? - centre).length()))
}

/// Placement of every box the frame at `time` draws.
fn placements_at(director: &mut Director, layout: &mut LayoutEngine, time: f64) -> HashMap<NodeId, BoxPlacement> {
    let plan = evaluate(director, layout, time);
    let device = M44::scale(director.output_scale_x, director.output_scale_y, 1.0);
    let mut placements = HashMap::new();
    for layer in &plan.layers {
        if let Layer::Scene { root, .. } = layer {
            box_placements(&director.scene, *root, device.clone(), &mut placements);
        }
    }
    placements
}

/// The part of `a`'s box that can land near `near` on screen, in box space.
/// The whole box when a corner of `near` misses its plane.
fn visible_part(a: &BoxPlacement, near: Rect) -> Rect {
    let m = &a.matrix;
    let box_rect = Rect::from_wh(a.size.0, a.size.1);
    let plane = Matrix::new_all(m.rc(0, 0), m.rc(0, 1), m.rc(0, 3), m.rc(1, 0), m.rc(1, 1), m.rc(1, 3), m.rc(3, 0), m.rc(3, 1), m.rc(3, 3));
    let Some(inverse) = plane.invert() else {
        return box_rect;
    };
    let mut seen = Rect::new(f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for corner in [(near.left, near.top), (near.right, near.top), (near.right, near.bottom), (near.left, near.bottom)] {
        let local = inverse.map_point(corner);
        let back = m.map(local.x, local.y, 0.0, 1.0);
        if back.w <= f32::EPSILON || !local.x.is_finite() || !local.y.is_finite() {
            return box_rect;
        }
        seen = Rect::new(seen.left.min(local.x), seen.top.min(local.y), seen.right.max(local.x), seen.bottom.max(local.y));
    }
    if Rect::intersects(&box_rect, seen) {
        Rect::new(box_rect.left.max(seen.left), box_rect.top.max(seen.top), box_rect.right.min(seen.right), box_rect.bottom.min(seen.bottom))
    } else {
        Rect::new_empty()
    }
}

/// Device pixels a box travels from placement `a` to `b`, measured on the
/// part of it near `screen`. A flat box moves affinely, so the corners of
/// that part bound its travel; a box in perspective is measured on a grid of
/// points both placements put in front of the eye and near the screen. Zero
/// when no point lands near the screen, and `None` when a point near it
/// crosses behind the eye.
fn box_travel(a: &BoxPlacement, b: &BoxPlacement, screen: Rect) -> Option<f32> {
    let projective = |m: &M44| m.rc(3, 0).abs() > 1e-7 || m.rc(3, 1).abs() > 1e-7;
    let steps = if projective(&a.matrix) || projective(&b.matrix) { 8 } else { 1 };
    let near = screen.with_outset((screen.width() / 4.0, screen.height() / 4.0));
    let part = visible_part(a, near);
    if part.is_empty() {
        return Some(0.0);
    }
    let map = |p: &BoxPlacement, x: f32, y: f32| {
        // `b` may have another size; the same fraction of its box stands in.
        let (x, y) = if p.size == a.size { (x, y) } else { (x / a.size.0.max(1e-3) * p.size.0, y / a.size.1.max(1e-3) * p.size.1) };
        let q = p.matrix.map(x, y, 0.0, 1.0);
        (q.w > f32::EPSILON).then(|| Point::new(q.x / q.w, q.y / q.w))
    };
    let mut travel = 0.0_f32;
    for i in 0..=steps {
        for j in 0..=steps {
            let x = part.left + part.width() * i as f32 / steps as f32;
            let y = part.top + part.height() * j as f32 / steps as f32;
            match (map(a, x, y), map(b, x, y)) {
                (Some(pa), Some(pb)) if steps == 1 || near.contains(pa) || near.contains(pb) => {
                    travel = travel.max((pa - pb).length());
                }
                (Some(p), None) | (None, Some(p)) if near.contains(p) => return None,
                _ => {}
            }
        }
    }
    Some(travel)
}

/// Samples a moving shutter needs: enough to space them `SAMPLE_SPACING`
/// apart along the furthest on-screen travel of a box or of paint inside one,
/// and every one through a transition or a travel that cannot be measured.
/// The shutter's two ends always count, so a fade averages and a cut inside
/// the shutter blends.
fn shutter_samples(director: &mut Director, layout: &mut LayoutEngine, start: f64, shutter: f64, most: u32) -> u32 {
    let Some(travels) = clip_paint_travels(director, start, start + shutter) else {
        return most;
    };
    let open = placements_at(director, layout, start);
    let closed = placements_at(director, layout, start + shutter);
    let screen = Rect::from_wh(director.width as f32 * director.output_scale_x, director.height as f32 * director.output_scale_y);
    let mut furthest = 0.0_f32;
    for (id, a) in &open {
        let Some(b) = closed.get(id) else {
            continue;
        };
        let Some(travel) = box_travel(a, b, screen) else {
            return most;
        };
        furthest = furthest.max(travel);
    }
    for (id, travel) in travels {
        match (travel, open.get(&id).and_then(device_scale)) {
            (Some(travel), Some(scale)) => furthest = furthest.max(travel * scale),
            _ => {
                return most;
            }
        }
    }
    ((furthest / SAMPLE_SPACING).ceil() as u32 + 1).clamp(2, most)
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
    /// sample per frame, sub-frames spread across the shutter that opens at
    /// `time` add up at equal weight. A shutter in which nothing moves paints
    /// once, and a moving one takes as many samples as [`shutter_samples`]
    /// asks. Scratch rasters come from `target`, so they share its backend.
    pub fn render(&mut self, director: &mut Director, time: f64, target: &mut Surface) -> Result<(), RenderError> {
        let samples = director.samples_per_frame.max(1);
        let shutter = (1.0 / director.fps) * (director.shutter_angle.clamp(0.0, 360.0) as f64 / 360.0);
        // The shutter opens at the frame time, so a cut on a frame lands whole.
        let shutter_start = time;
        if samples == 1 || !moves_between(director, shutter_start, shutter_start + shutter) {
            let plan = evaluate(director, &mut self.layout, time);
            return paint(director, &plan, target, &mut self.transitions);
        }
        if self.accumulation.is_none() {
            // Summing 1/N-weighted samples in 8 bits rounds each one, which leaves
            // smooth gradients with about 256/N levels; the sum stays in half floats.
            let info = target.image_info();
            let failed = || RenderError::Surface(info.width(), info.height());
            let sample = target.new_surface(&info).ok_or_else(failed)?;
            let sum = target.new_surface(&info.with_color_type(ColorType::RGBAF16)).ok_or_else(failed)?;
            self.accumulation = Some((sample, sum));
        }
        let samples = shutter_samples(director, &mut self.layout, shutter_start, shutter, samples);
        let (sample_target, sum) = self.accumulation.as_mut().unwrap();
        let mut weight = Paint::default();
        weight.set_alpha_f(1.0 / samples as f32);
        weight.set_blend_mode(BlendMode::Plus);
        sum.canvas().clear(Color::TRANSPARENT);
        for sample in 0..samples {
            let sample_time = shutter_start + (sample as f64 / (samples - 1) as f64) * shutter;
            let plan = evaluate(director, &mut self.layout, sample_time);
            paint(director, &plan, sample_target, &mut self.transitions)?;
            sum.canvas().draw_image(sample_target.image_snapshot(), (0, 0), Some(&weight));
        }
        let mut add = Paint::default();
        add.set_blend_mode(BlendMode::Plus);
        target.canvas().clear(clear_color(director));
        target.canvas().draw_image(sum.image_snapshot(), (0, 0), Some(&add));
        Ok(())
    }
}
