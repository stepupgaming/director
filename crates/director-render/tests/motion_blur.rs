//! Motion blur averages shutter samples, and a shutter in which nothing moves
//! paints once.

use director_core::element::Element;
use director_core::video_wrapper::RenderMode;
use director_core::{DefaultAssetLoader, Director, RenderError};
use director_render::FrameState;
use skia_safe::{Canvas, Paint, Rect};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Counts its paints and moves only between 1 s and 2 s of local time.
#[derive(Clone, Debug)]
struct Mover(Arc<AtomicUsize>);

impl Element for Mover {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn layout_style(&self) -> taffy::style::Style {
        taffy::style::Style::default()
    }
    fn update(&mut self, _: f64) -> bool {
        false
    }
    fn moves_between(&self, t0: f64, t1: f64) -> bool {
        t0 < 2.0 && t1 >= 1.0
    }
    fn render(&self, canvas: &Canvas, rect: Rect, _: f32, _: &mut dyn FnMut(&Canvas)) -> Result<(), RenderError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        canvas.draw_rect(rect, &Paint::default());
        Ok(())
    }
    fn animate_property(&mut self, _: &str, _: f32, _: f32, _: f64, _: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Eight samples at 10 fps over a 180 degree shutter, with one `Mover` clip
/// from `start` for `duration` seconds.
fn blurred(start: f64, duration: f64) -> (Director, Arc<AtomicUsize>) {
    let mut director = Director::new(16, 16, 10.0, Arc::new(DefaultAssetLoader), RenderMode::Preview, None);
    (director.samples_per_frame, director.shutter_angle) = (8, 180.0);
    let paints = Arc::new(AtomicUsize::new(0));
    let root = director.scene.add_node(Box::new(Mover(paints.clone())));
    director.push_scene_root_at(root, start, duration, None);
    (director, paints)
}

fn paints_at(director: &mut Director, paints: &AtomicUsize, time: f64) -> usize {
    let mut surface = skia_safe::surfaces::raster_n32_premul((16, 16)).unwrap();
    let before = paints.load(Ordering::SeqCst);
    FrameState::default().render(director, time, &mut surface).unwrap();
    paints.load(Ordering::SeqCst) - before
}

#[test]
fn a_still_shutter_paints_one_sample() {
    let (mut director, paints) = blurred(0.0, 5.0);
    assert_eq!(paints_at(&mut director, &paints, 0.5), 1);
    assert_eq!(paints_at(&mut director, &paints, 1.5), 8);
    // The shutter at 1 s opens at 0.975 s, before the motion starts.
    assert_eq!(paints_at(&mut director, &paints, 1.0), 8);
    assert_eq!(paints_at(&mut director, &paints, 3.0), 1);
}

#[test]
fn clip_edges_and_fades_inside_the_shutter_sample_every_time() {
    // The clip runs 3..4 s, so its local clock never reaches the motion.
    let (mut director, paints) = blurred(3.0, 1.0);
    assert_eq!(paints_at(&mut director, &paints, 3.5), 1);
    // At 4 s the shutter spans 3.975..4.025 s and the clip ends inside it.
    assert_eq!(paints_at(&mut director, &paints, 4.0), 4);
    director.tracks[0].clips[0].fade_in = 0.25;
    assert_eq!(paints_at(&mut director, &paints, 3.2), 8);
    assert_eq!(paints_at(&mut director, &paints, 3.5), 1);
}
