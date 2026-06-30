use crate::director::Director;
use crate::element::Element;
use crate::errors::RenderError;
use crate::systems::layout::LayoutEngine;
use crate::systems::renderer::render_recursive;
use skia_safe::{AlphaType, Canvas, ColorType, Paint, Rect, Surface};
use std::any::Any;
use std::fmt;
use std::sync::Mutex;
use taffy::style::{AlignItems, FlexDirection, JustifyContent, Style};

// --- Composition Node (RFC 010) ---

/// A node that contains its own isolated timeline and Director.
///
/// Used for nesting compositions (e.g. pre-comps). It renders the sub-timeline to a surface.
pub struct CompositionNode {
    pub internal_director: Mutex<Director>,
    pub start_offset: f64,
    pub surface_cache: Mutex<Option<Surface>>,
    pub style: Style,
}

impl CompositionNode {
    pub fn new(internal_director: Director) -> Self {
        let mut style = Style::default();
        // Defaults to "Vertical Stack, Centered"
        style.flex_direction = FlexDirection::Column;
        style.align_items = Some(AlignItems::Center);
        style.justify_content = Some(JustifyContent::Center);

        Self {
            internal_director: Mutex::new(internal_director),
            start_offset: 0.0,
            surface_cache: Mutex::new(None),
            style,
        }
    }
}

impl Clone for CompositionNode {
    fn clone(&self) -> Self {
        let dir = match self.internal_director.lock() {
            Ok(g) => g.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        Self {
            internal_director: Mutex::new(dir),
            start_offset: self.start_offset,
            surface_cache: Mutex::new(None),
            style: self.style.clone(),
        }
    }
}

impl fmt::Debug for CompositionNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompositionNode")
            .field("start_offset", &self.start_offset)
            .finish()
    }
}

impl Element for CompositionNode {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout_style(&self) -> Style {
        self.style.clone()
    }

    fn update(&mut self, time: f64) -> bool {
        let comp_time = time - self.start_offset;
        let mut d = match self.internal_director.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        d.update(comp_time);

        let mut layout_engine = LayoutEngine::new();
        let w = d.width;
        let h = d.height;
        layout_engine.compute_layout(&mut d.scene, w, h, comp_time);
        d.run_post_layout(comp_time);

        true
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        let d = match self.internal_director.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };

        let width = d.width;
        let height = d.height;

        let mut surface_opt = match self.surface_cache.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };

        // Recreate surface if needed
        let need_new = if let Some(s) = surface_opt.as_ref() {
            s.width() != width || s.height() != height
        } else {
            true
        };

        if need_new {
            let info = skia_safe::ImageInfo::new(
                (width, height),
                ColorType::RGBA8888,
                AlphaType::Premul,
                Some(skia_safe::ColorSpace::new_srgb()),
            );
            *surface_opt = skia_safe::surfaces::raster(&info, None, None);
        }

        if let Some(surface) = surface_opt.as_mut() {
            // Render internal director to surface
            let c = surface.canvas();
            c.clear(skia_safe::Color::TRANSPARENT);

            // Reuse render logic
            let current_time = d
                .scene
                .nodes
                .iter()
                .flatten()
                .next()
                .map(|n| n.last_visit_time)
                .unwrap_or(0.0);

            let has_tracks = d.tracks.iter().any(|t| !t.clips.is_empty());
            if has_tracks {
                let mut clips: Vec<(usize, usize, &crate::timeline::TrackClip)> = Vec::new();
                for (ti, track) in d.tracks.iter().enumerate() {
                    for (ci, clip) in track.clips.iter().enumerate() {
                        if track.enabled
                            && current_time >= clip.start_time
                            && current_time < clip.start_time + clip.duration
                        {
                            clips.push((ti, ci, clip));
                        }
                    }
                }
                clips.sort_by_key(|(_, _, c)| c.z_index);
                for (ti, _, clip) in clips {
                    let track_opacity = d.tracks[ti].opacity;
                    let fade_opacity = clip.fade_opacity(current_time);
                    render_recursive(
                        &d.scene,
                        &d.assets,
                        clip.scene_root,
                        c,
                        track_opacity * fade_opacity,
                        0,
                    )?;
                }
            } else {
                let mut items: Vec<(usize, crate::director::TimelineItem)> = d
                    .timeline
                    .iter()
                    .cloned()
                    .enumerate()
                    .filter(|(_, item)| {
                        current_time >= item.start_time
                            && current_time < item.start_time + item.duration
                    })
                    .collect();
                items.sort_by_key(|(_, item)| item.z_index);

                for (_, item) in items {
                    render_recursive(&d.scene, &d.assets, item.scene_root, c, 1.0, 0)?;
                }
            }

            // Now draw surface to main canvas
            let image = surface.image_snapshot();
            let mut paint = Paint::default();
            paint.set_alpha_f(opacity);

            // Draw image filling the layout rect
            canvas.draw_image_rect(&image, None, rect, &paint);
        }

        draw_children(canvas);
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
        // No animatable properties on CompositionNode itself yet (e.g. opacity is handled by SceneNode blending)
    }

    fn get_audio(&self, time: f64, samples_needed: usize, _sample_rate: u32) -> Option<Vec<f32>> {
        let comp_time = time - self.start_offset;
        let mut d = match self.internal_director.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        Some(d.mix_audio(samples_needed, comp_time))
    }
}
