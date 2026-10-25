//! Core Lottie and audio scenes render and mix the same from their scene
//! document as from the live `Director`.

use director_core::animation::{Animated, EasingType};
use director_core::audio::load_audio_bytes;
use director_core::audio_effects::{AudioEffect, BiquadEQ, BiquadType, Compressor, Delay, EffectParamAnimation, Reverb};
use director_core::node::{BoxNode, LottieNode, SpectrumVisualizerNode};
use director_core::scene::AudioBinding;
use director_core::scene_export::export_scene;
use director_core::scene_import::{import_scene, ImportRegistry};
use director_core::types::Color;
use director_core::video_wrapper::RenderMode;
use director_core::{AssetLoader, DefaultAssetLoader, Director};
use director_render::Renderer;
use director_scene::{NodeBody, SceneDocument};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

mod common;

const TIMES: [f64; 4] = [0.0, 0.5, 1.25, 1.9];

fn repo(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(path)
}

fn loader() -> Arc<dyn AssetLoader> {
    Arc::new(DefaultAssetLoader)
}

fn director(size: f32) -> Director {
    Director::new(size as i32, size as i32, 30.0, loader(), RenderMode::Export, None)
}

fn sized(size: f32) -> taffy::Style {
    taffy::Style {
        size: taffy::Size {
            width: taffy::Dimension::length(size),
            height: taffy::Dimension::length(size),
        },
        ..Default::default()
    }
}

/// The document after a JSON round trip.
fn document(director: &Director) -> SceneDocument {
    let json = export_scene(director).unwrap().to_json().unwrap();
    SceneDocument::from_json(&json).unwrap()
}

/// Frames at [`TIMES`] where the document render differs from the live `Director` on the core path.
fn mismatches(name: &str, original: Director) -> Vec<String> {
    let renderer = Renderer::new(Arc::new(document(&original)), loader()).with_render_mode(original.render_mode);
    common::mismatches(name, original, &renderer, &TIMES)
}

fn lottie_scene() -> Director {
    let mut director = director(256.0);
    let mut root = BoxNode::new();
    root.style = sized(256.0);
    root.bg_color = Some(Animated::new(Color::new(0.95, 0.95, 0.96, 1.0)));
    let root = director.scene.add_node(Box::new(root));
    let json = std::fs::read(repo("crates/lottie-data/tests/heart_eyes.json")).unwrap();
    let photo = std::fs::read(repo("examples/assets/photo_a.jpg")).unwrap();
    let photo = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&photo)).unwrap();
    let images = HashMap::from([("override".to_string(), photo)]);
    let mut lottie = LottieNode::new(&json, images, &director.assets).unwrap();
    lottie.style = sized(256.0);
    lottie.speed = 1.5;
    lottie.loop_anim = true;
    lottie.opacity.add_segment(0.4, 1.0, 1.0, EasingType::EaseOut);
    let lottie = director.scene.add_node(Box::new(lottie));
    assert!(director.add_child(root, lottie));
    director.push_scene_root(root, 2.0);
    director
}

#[test]
fn lottie_renders_from_the_document() {
    let director = lottie_scene();
    let document = document(&director);
    let lottie = document.nodes.iter().find(|node| node.kind == "lottie").unwrap();
    let NodeBody::Lottie(body) = &lottie.body else {
        panic!("lottie record holds {:?}", lottie.body.kind());
    };
    assert_eq!(body.images.keys().collect::<Vec<_>>(), ["override"]);
    let imported = import_scene(&document, loader(), &ImportRegistry::core(), RenderMode::Export).unwrap();
    assert_eq!(export_scene(&imported).unwrap(), export_scene(&director).unwrap());
    let failures = mismatches("lottie", director);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A tone file trimmed by `source_start` with an animated effect chain, an
/// empty slot, generated looping samples, a binding and a spectrum.
fn audio_scene() -> Director {
    let mut director = director(96.0);
    let tone = repo("examples/assets/tone.wav");
    let mut samples = load_audio_bytes(&std::fs::read(&tone).unwrap(), 48000).unwrap();
    samples.drain(..(0.25 * 96000.0) as usize);
    let file = director.add_scene_audio(samples, 0.1, 1.5);
    let removed = director.add_global_audio(vec![0.5; 960]);
    let generated: Vec<f32> = (0..9600).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
    let looped = director.add_global_audio(generated);

    let track = director.audio_mixer.tracks[file].as_mut().unwrap();
    track.source_path = Some(tone.to_string_lossy().into_owned());
    track.source_start = 0.25;
    track.volume.add_segment(0.8, 0.2, 1.0, EasingType::Linear);
    track.pan = Animated::new(0.3);
    let mut eq = BiquadEQ::new(48000);
    eq.filter_type = BiquadType::HighPass;
    eq.set_parameter("frequency", 400.0);
    eq.set_parameter("q", 0.9);
    track.effects.add(Box::new(eq));
    let mut reverb = Reverb::new(48000);
    reverb.set_parameter("wet", 0.3);
    track.effects.add(Box::new(reverb));
    let mut delay = Delay::new(48000);
    delay.set_parameter("delay_time", 0.12);
    delay.set_parameter("feedback", 0.4);
    track.effects.add(Box::new(delay));
    let mut compressor = Compressor::new(48000);
    compressor.set_parameter("threshold", -18.0);
    compressor.set_parameter("ratio", 3.0);
    track.effects.add(Box::new(compressor));
    let mut frequency = Animated::new(400.0);
    frequency.add_segment(400.0, 2000.0, 1.5, EasingType::EaseInOut);
    track.effect_param_animations.push(EffectParamAnimation {
        effect_idx: 0,
        param_name: "frequency".to_string(),
        animated: frequency,
    });

    director.audio_mixer.tracks[removed] = None;
    let track = director.audio_mixer.tracks[looped].as_mut().unwrap();
    track.loop_audio = true;
    track.duration = Some(1.2);

    let mut root = BoxNode::new();
    root.style = sized(96.0);
    root.bg_color = Some(Animated::new(Color::new(0.02, 0.02, 0.06, 1.0)));
    let root = director.scene.add_node(Box::new(root));
    let mut bars = SpectrumVisualizerNode::new(
        file,
        12,
        Color::new(0.0, 1.0, 0.66, 1.0),
        2.0,
        0.5,
        "bottom".to_string(),
        0.05,
        0.9,
        2.0,
    );
    bars.style = sized(80.0);
    let bars = director.scene.add_node(Box::new(bars));
    director.scene.get_node_mut(bars).unwrap().audio_bindings.push(AudioBinding {
        track_id: file,
        band: "bass".to_string(),
        property: "scale".to_string(),
        min_value: 0.8,
        max_value: 1.2,
        smoothing: 0.3,
        prev_value: 0.8,
    });
    assert!(director.add_child(root, bars));
    director.push_scene_root(root, 2.0);
    director
}

#[test]
fn audio_mixes_and_renders_from_the_document() {
    let mut original = audio_scene();
    let document = document(&original);
    assert_eq!(document.audio.tracks.iter().map(|t| t.slot).collect::<Vec<_>>(), [0, 2]);
    assert_eq!(document.asset(&document.audio.tracks[0].asset).unwrap().media_type, "audio/*");
    let mut imported = import_scene(&document, loader(), &ImportRegistry::core(), RenderMode::Export).unwrap();
    assert!(imported.audio_mixer.tracks[1].is_none());
    assert_eq!(export_scene(&imported).unwrap(), export_scene(&original).unwrap());
    let block = 1600;
    for step in 0..60 {
        let time = step as f64 * block as f64 / 2.0 / 48000.0;
        let expected = original.mix_audio(block, time);
        let actual = imported.mix_audio(block, time);
        assert_eq!(expected.len(), actual.len());
        let worst = expected.iter().zip(&actual).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-6, "mix at {time}s differs by {worst}");
    }
    let failures = mismatches("audio", audio_scene());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
