//! Parity helpers: the core `Director` render path is the reference that
//! `director-render` frames must match.

use director_core::systems::layout::LayoutEngine;
use director_core::systems::renderer::render_sampled_at_time;
use director_core::Director;
use director_render::{Frame, Renderer};
use skia_safe::{AlphaType, ColorSpace, ColorType, ImageInfo, Surface};

pub const CHANNEL_DELTA: u8 = 16;
pub const MAX_FRACTION: f64 = 0.001;

/// A `Director` painted through core `render_sampled_at_time`.
pub struct CoreSession {
    director: Director,
    layout: LayoutEngine,
    info: ImageInfo,
    surface: Surface,
    accumulation: Option<Surface>,
    transitions: Option<(Surface, Surface)>,
}

impl CoreSession {
    pub fn new(director: Director, (width, height): (i32, i32)) -> Self {
        let info = ImageInfo::new(
            (width, height),
            ColorType::RGBA8888,
            AlphaType::Premul,
            Some(ColorSpace::new_srgb()),
        );
        let raster = || skia_safe::surfaces::raster(&info, None, None).unwrap();
        let transitions = (!director.transitions.is_empty()).then(|| (raster(), raster()));
        Self {
            director,
            layout: LayoutEngine::new(),
            surface: raster(),
            info,
            accumulation: None,
            transitions,
        }
    }

    pub fn render(&mut self, time: f64) -> Frame {
        render_sampled_at_time(
            &mut self.director,
            &mut self.layout,
            time,
            &mut self.surface,
            &mut self.accumulation,
            &mut self.transitions,
        )
        .unwrap();
        let (width, height) = (self.info.width() as usize, self.info.height() as usize);
        let mut rgba = vec![0u8; width * height * 4];
        let read_info = self.info.clone().with_alpha_type(AlphaType::Unpremul);
        assert!(self.surface.read_pixels(&read_info, &mut rgba, width * 4, (0, 0)));
        Frame {
            width: width as u32,
            height: height as u32,
            rgba,
        }
    }
}

pub fn differing_fraction(a: &Frame, b: &Frame) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let different = a
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.rgba.as_chunks::<4>().0)
        .filter(|(x, y)| x.iter().zip(y.iter()).any(|(p, q)| p.abs_diff(*q) > CHANNEL_DELTA))
        .count();
    different as f64 / (a.width as f64 * a.height as f64)
}

/// Frames at `times` where `renderer` differs from `original` on the core path.
#[allow(dead_code)]
pub fn mismatches(name: &str, original: Director, renderer: &Renderer, times: &[f64]) -> Vec<String> {
    let mut imported = renderer.session().unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut original = CoreSession::new(original, renderer.output_size());
    let mut failures = Vec::new();
    for &time in times {
        let expected = original.render(time);
        let actual = imported.render(time).unwrap_or_else(|e| panic!("{name} at {time}s: {e}"));
        let fraction = differing_fraction(&expected, &actual);
        if fraction > MAX_FRACTION {
            failures.push(format!("{name} at {time}s: {fraction:.6} of pixels differ"));
        }
    }
    failures
}
