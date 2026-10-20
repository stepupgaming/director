//! Layout style, looping audio, and generational scene-audio checks.

use director_core::animation::Animated;
use director_core::audio::{mix_scene_audio, AudioMixer, AudioTrack};
use director_core::audio_effects::AudioEffectChain;
use director_core::node::{BoxNode, CompositionNode, EffectNode, ShapeNode, ShapeType, TextNode};
use director_core::scene::SceneGraph;
use director_core::systems::layout::LayoutEngine;
use director_core::video_wrapper::RenderMode;
use director_core::{DefaultAssetLoader, Director, Element, RenderError};
use skia_safe::textlayout::FontCollection;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use taffy::prelude::*;

fn fixed_style(width: f32, height: f32) -> Style {
    Style {
        display: Display::Flex,
        size: Size {
            width: Dimension::length(width),
            height: Dimension::length(height),
        },
        ..Style::default()
    }
}

fn assert_layout_style_applies(mut element: Box<dyn Element>, label: &str) {
    element.set_layout_style(fixed_style(80.0, 40.0));
    let stored = element.layout_style();
    assert_eq!(
        stored.size.width,
        Dimension::length(80.0),
        "{label} stored width"
    );
    assert_eq!(
        stored.size.height,
        Dimension::length(40.0),
        "{label} stored height"
    );

    let mut scene = SceneGraph::new();
    let mut root = BoxNode::new();
    root.style.display = Display::Flex;
    root.style.flex_direction = FlexDirection::Row;
    root.style.align_items = Some(AlignItems::FlexStart);
    root.style.size = Size {
        width: Dimension::length(400.0),
        height: Dimension::length(400.0),
    };
    let root_id = scene.add_node(Box::new(root));
    let child_id = scene.add_node(element);
    scene.add_child(root_id, child_id);
    scene.get_node_mut(root_id).unwrap().last_visit_time = 0.0;

    let mut layout = LayoutEngine::new();
    layout.compute_layout(&mut scene, 400, 400, 0.0);
    let rect = scene.get_node(child_id).unwrap().layout_rect;
    assert!(
        (rect.width() - 80.0).abs() < 0.01,
        "{label} laid out width {}",
        rect.width()
    );
    assert!(
        (rect.height() - 40.0).abs() < 0.01,
        "{label} laid out height {}",
        rect.height()
    );

    {
        let node = scene.get_node_mut(child_id).unwrap();
        node.element.set_layout_style(fixed_style(120.0, 70.0));
        node.dirty_style = true;
        node.last_visit_time = 0.0;
    }
    scene.get_node_mut(root_id).unwrap().last_visit_time = 0.0;
    layout.compute_layout(&mut scene, 400, 400, 0.0);
    let rect = scene.get_node(child_id).unwrap().layout_rect;
    assert!(
        (rect.width() - 120.0).abs() < 0.01,
        "{label} updated width {}",
        rect.width()
    );
    assert!(
        (rect.height() - 70.0).abs() < 0.01,
        "{label} updated height {}",
        rect.height()
    );
}

#[test]
fn set_layout_style_updates_box_text_effect_shape_and_composition() {
    assert_layout_style_applies(Box::new(BoxNode::new()), "box");
    assert_layout_style_applies(
        Box::new(TextNode::new(
            Vec::new(),
            Arc::new(Mutex::new(FontCollection::new())),
        )),
        "text",
    );
    assert_layout_style_applies(
        Box::new(EffectNode {
            effects: Vec::new(),
            style: Style::default(),
            shader_cache: Arc::new(Mutex::new(HashMap::new())),
            current_time: 0.0,
        }),
        "effect",
    );
    assert_layout_style_applies(
        Box::new(ShapeNode::new(ShapeType::Circle {
            cx: 1.0,
            cy: 1.0,
            r: 1.0,
        })),
        "shape",
    );
    assert_layout_style_applies(
        Box::new(CompositionNode::new(Director::new(
            64,
            64,
            30.0,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        ))),
        "composition",
    );
}

fn track(samples: Vec<f32>, loop_audio: bool, duration: Option<f64>) -> AudioTrack {
    AudioTrack {
        samples,
        volume: Animated::new(1.0),
        pan: Animated::new(0.0),
        effects: AudioEffectChain::new(),
        effect_param_animations: Vec::new(),
        start_time: 0.0,
        duration,
        loop_audio,
    }
}

#[test]
fn loop_audio_skips_empty_tracks_and_wraps_a_real_track() {
    let mut mixer = AudioMixer::new(48_000);
    mixer.add_track(track(Vec::new(), true, None));
    mixer.add_track(track(Vec::new(), true, Some(0.0)));
    mixer.add_track(track(vec![0.5, 0.5], true, Some(0.0)));
    let silent = mixer.mix(8, 0.0);
    assert_eq!(silent, vec![0.0; 16]);

    let mut mixer = AudioMixer::new(1);
    mixer.add_track(track(vec![0.25, 0.0, 0.75, 0.0], true, None));
    let mixed = mixer.mix(4, 0.0);
    let expected = [0.25, 0.0, 0.75, 0.0, 0.25, 0.0, 0.75, 0.0];
    for (got, want) in mixed.iter().zip(expected) {
        assert!((got - want).abs() < 1e-5, "looped sample {got} != {want}");
    }
}

#[derive(Clone, Debug)]
struct Tone {
    sample: f32,
}

impl Element for Tone {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn layout_style(&self) -> Style {
        Style::default()
    }
    fn update(&mut self, _time: f64) -> bool {
        false
    }
    fn render(
        &self,
        _canvas: &skia_safe::Canvas,
        _layout_rect: skia_safe::Rect,
        _opacity: f32,
        _draw_children: &mut dyn FnMut(&skia_safe::Canvas),
    ) -> Result<(), RenderError> {
        Ok(())
    }
    fn animate_property(
        &mut self,
        _property: &str,
        _start: f32,
        _target: f32,
        _duration: f64,
        _easing: &str,
    ) -> Result<(), String> {
        Ok(())
    }
    fn get_audio(&self, _time: f64, samples_needed: usize, _sample_rate: u32) -> Option<Vec<f32>> {
        Some(vec![self.sample; samples_needed * 2])
    }
}

#[test]
fn mix_scene_audio_ignores_a_recycled_node_index() {
    let mut scene = SceneGraph::new();
    let parent = scene.add_node(Box::new(BoxNode::new()));
    let stale = scene.add_node(Box::new(Tone { sample: 0.25 }));
    scene.add_child(parent, stale);
    scene.destroy_node(stale);
    let live = scene.add_node(Box::new(Tone { sample: 0.75 }));
    assert_eq!(stale.index(), live.index());
    assert_ne!(stale.generation, live.generation);

    let mut from_stale = vec![0.0; 2];
    mix_scene_audio(&mut from_stale, &scene, &[(stale, 0.0)], 1, 48_000);
    assert!(
        from_stale.iter().all(|sample| sample.abs() < 1e-6),
        "stale id mixed {from_stale:?}"
    );

    scene.get_node_mut(parent).unwrap().children.push(stale);
    scene.get_node_mut(parent).unwrap().children.push(live);
    let mut mixed = vec![0.0; 2];
    mix_scene_audio(&mut mixed, &scene, &[(parent, 0.0)], 1, 48_000);
    assert!(
        (mixed[0] - 0.75).abs() < 1e-5 && (mixed[1] - 0.75).abs() < 1e-5,
        "recycled child mixed {mixed:?}"
    );
}
