use crate::animation::Animated;
use crate::element::Element;
use crate::errors::RenderError;
use crate::node::parse_easing;
use crate::types::Color;
use skia_safe::{Canvas, Paint, RRect, Rect};
use std::any::Any;
use taffy::style::Style;

/// An audio-reactive spectrum visualizer that renders frequency bars
/// directly from FFT spectrum data.
///
/// Bar heights are computed during `Director::update()` via downcast
/// and smoothed temporally for fluid animation.
#[derive(Debug, Clone)]
pub struct SpectrumVisualizerNode {
    pub style: Style,
    /// Audio track index in the mixer
    pub track_id: usize,
    /// Number of bars to render
    pub bar_count: usize,
    /// Base color of the bars
    pub bar_color: Color,
    /// Horizontal gap between bars in pixels
    pub bar_gap: f32,
    /// Current normalized bar heights (0..1), computed each frame
    pub bar_heights: Vec<f32>,
    /// Smoothed heights for temporal interpolation
    pub smoothed_heights: Vec<f32>,
    /// Smoothing factor (0.0 = instant response, 0.9 = very smooth)
    pub smoothing: f32,
    /// Vertical alignment: "bottom" (grow up), "top" (grow down), "center" (grow from middle)
    pub position: String,
    /// Minimum bar height as fraction of layout height (0..1)
    pub min_height: f32,
    /// Maximum bar height as fraction of layout height (0..1)
    pub max_height: f32,
    /// Corner radius of each bar
    pub border_radius: f32,
    /// Node opacity
    pub opacity: Animated<f32>,
}

impl SpectrumVisualizerNode {
    pub fn new(
        track_id: usize,
        bar_count: usize,
        bar_color: Color,
        bar_gap: f32,
        smoothing: f32,
        position: String,
        min_height: f32,
        max_height: f32,
        border_radius: f32,
    ) -> Self {
        Self {
            style: Style::default(),
            track_id,
            bar_count,
            bar_color,
            bar_gap,
            bar_heights: vec![0.0; bar_count],
            smoothed_heights: vec![0.0; bar_count],
            smoothing: smoothing.clamp(0.0, 0.99),
            position,
            min_height: min_height.clamp(0.0, 1.0),
            max_height: max_height.clamp(0.0, 1.0),
            border_radius,
            opacity: Animated::new(1.0),
        }
    }
}

impl Element for SpectrumVisualizerNode {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn layout_style(&self) -> Style {
        self.style.clone()
    }

    fn set_layout_style(&mut self, style: Style) {
        self.style = style;
    }

    fn update(&mut self, time: f64) -> bool {
        // Update opacity animation.
        self.opacity.update(time);

        // If no audio has ever driven the bars (all smoothed heights still zero),
        // show a gentle sine fallback so the visualizer isn't flat for scripts
        // that don't provide an audio track.
        let has_audio = self.smoothed_heights.iter().any(|&h| h > 0.0);
        if !has_audio {
            for i in 0..self.bar_count {
                let phase = time * 2.0 + i as f64 * 0.4;
                let h = ((phase.sin() * 0.5 + 0.5) * 0.4 + 0.1) as f32;
                self.bar_heights[i] = h;
            }
        }
        true
    }

    fn render(
        &self,
        canvas: &Canvas,
        rect: Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        let local_opacity = self.opacity.current_value * opacity;
        if local_opacity <= 0.0 || self.bar_count == 0 {
            draw_children(canvas);
            return Ok(());
        }

        let total_gap = self.bar_gap * (self.bar_count.saturating_sub(1)) as f32;
        let available_width = (rect.width() - total_gap).max(1.0);
        let bar_width = available_width / self.bar_count as f32;

        for i in 0..self.bar_count {
            let height_norm = self.bar_heights[i].clamp(0.0, 1.0);
            let bar_height = rect.height()
                * (self.min_height + height_norm * (self.max_height - self.min_height));

            let x = rect.left + i as f32 * (bar_width + self.bar_gap);

            let (y, h) = match self.position.as_str() {
                "top" => (rect.top, bar_height),
                "center" | "middle" => {
                    let half = bar_height / 2.0;
                    (rect.top + rect.height() / 2.0 - half, bar_height)
                }
                _ => {
                    // bottom (default): bars grow upward from bottom
                    (rect.bottom - bar_height, bar_height)
                }
            };

            let bar_rect = Rect::from_xywh(x, y, bar_width, h.max(0.1));
            let rrect = RRect::new_rect_xy(&bar_rect, self.border_radius, self.border_radius);

            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            let mut c = self.bar_color;
            c.a *= local_opacity;
            paint.set_color4f(c.to_color4f(), None);

            canvas.draw_rrect(rrect, &paint);
        }

        draw_children(canvas);
        Ok(())
    }

    fn set_opacity(&mut self, opacity: f32) {
        self.opacity.current_value = opacity;
    }

    fn animate_property(
        &mut self,
        property: &str,
        start: f32,
        target: f32,
        duration: f64,
        easing: &str,
    ) {
        let ease_fn = parse_easing(easing);
        match property {
            "opacity" => self.opacity.add_segment(start, target, duration, ease_fn),
            _ => {}
        }
    }

    fn animate_property_spring(
        &mut self,
        property: &str,
        start: Option<f32>,
        target: f32,
        config: crate::animation::SpringConfig,
    ) {
        let apply = |anim: &mut Animated<f32>| {
            if let Some(s) = start {
                anim.add_spring_with_start(s, target, config);
            } else {
                anim.add_spring(target, config);
            }
        };
        match property {
            "opacity" => apply(&mut self.opacity),
            _ => {}
        }
    }
}
