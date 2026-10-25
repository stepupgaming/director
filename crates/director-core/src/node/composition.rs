use crate::director::Director;
use crate::element::Element;
use crate::errors::RenderError;
use crate::systems::layout::LayoutEngine;
use skia_safe::{Canvas, Rect, Surface};
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
        // Defaults to "Vertical Stack, Centered"
        let style = Style {
            flex_direction: FlexDirection::Column,
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::Center),
            ..Default::default()
        };

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
    fn export_body(
        &self,
        cx: &mut crate::scene_export::ExportContext,
    ) -> Result<crate::scene_export::ElementExport, crate::scene_export::ExportError> {
        crate::scene_export::nodes::composition_body(self, cx)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn kind(&self) -> &'static str {
        "composition"
    }

    fn layout_style(&self) -> Style {
        self.style.clone()
    }

    fn set_layout_style(&mut self, style: Style) {
        self.style = style;
    }

    fn update(&mut self, time: f64) -> bool {
        let comp_time = time - self.start_offset;
        let mut d = match self.internal_director.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        d.update(comp_time, None);

        let mut layout_engine = LayoutEngine::new();
        let w = d.width;
        let h = d.height;
        layout_engine.compute_layout(&mut d.scene, w, h, comp_time);
        d.run_post_layout(comp_time);

        true
    }

    /// `director-render` paints the nested timeline into `surface_cache`; this
    /// only paints children.
    fn render(
        &self,
        canvas: &Canvas,
        _rect: Rect,
        _opacity: f32,
        draw_children: &mut dyn FnMut(&Canvas),
    ) -> Result<(), RenderError> {
        draw_children(canvas);
        Ok(())
    }

    fn animate_property(
        &mut self,
        property: &str,
        _start: f32,
        _target: f32,
        _duration: f64,
        _easing: &str,
    ) -> Result<(), String> {
        // No animatable properties on CompositionNode itself yet
        // (opacity is handled by SceneNode blending).
        Err(format!(
            "unknown property '{property}' on composition; no element-level animatable properties"
        ))
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
