//! # Audio API
//!
//! Audio playback and analysis functions for Rhai scripts.
//!
//! ## Responsibilities
//! - **Audio Loading**: `add_audio` for adding audio tracks, `add_global_audio` for raw samples
//! - **Volume Animation**: `animate_volume` for volume fades
//! - **Audio Analysis**: `bass`, `mids`, `highs`, `get_energy`, `get_spectrum`, `loudness`
//! - **Audio Reactivity**: `bind_audio` for audio-reactive properties
//! - **Safety Guards**: lock-poison handling and track/node validation

use crate::audio_effects::{AudioEffect, BiquadEQ, BiquadType, Compressor, Delay, EffectParamAnimation, Reverb};
use rhai::Engine;
use tracing::error;

use super::super::types::{AudioTrackHandle, MovieHandle, NodeHandle, SceneHandle};
use super::super::utils::parse_easing;

/// Register audio-related Rhai functions.
pub fn register(engine: &mut Engine) {
    engine.register_type_with_name::<AudioTrackHandle>("AudioTrack");

    engine.register_fn("add_audio", |movie: &mut MovieHandle, path: &str| {
        let mut d = match movie.lock_director() {
            Ok(d) => d,
            Err(e) => {
                error!("{}", e);
                return AudioTrackHandle {
                    director: movie.director.clone(),
                    id: usize::MAX,
                };
            }
        };
        let bytes = d.assets.loader.load_bytes(path).unwrap_or(Vec::new());
        let samples = crate::audio::load_audio_bytes(&bytes, d.audio_mixer.sample_rate)
            .unwrap_or_else(|e| {
                error!("Audio error: {}", e);
                Vec::new()
            });

        let id = d.add_global_audio(samples);
        AudioTrackHandle {
            director: movie.director.clone(),
            id,
        }
    });

    engine.register_fn(
        "add_global_audio",
        |movie: &mut MovieHandle, samples: rhai::Array| -> Result<AudioTrackHandle, Box<rhai::EvalAltResult>> {
            let mut d = movie.lock_director()?;
            let samples: Vec<f32> = samples
                .into_iter()
                .map(|v| v.as_float().map(|f| f as f32))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("invalid audio sample: {}", e))?;
            let id = d.add_global_audio(samples);
            Ok(AudioTrackHandle {
                director: movie.director.clone(),
                id,
            })
        },
    );

    engine.register_fn("add_audio", |scene: &mut SceneHandle, path: &str| {
        let id = {
            let mut d = match scene.lock_director() {
                Ok(d) => d,
                Err(e) => {
                    error!("{}", e);
                    return AudioTrackHandle {
                        director: scene.director.clone(),
                        id: usize::MAX,
                    };
                }
            };
            let bytes = d.assets.loader.load_bytes(path).unwrap_or(Vec::new());
            let samples = crate::audio::load_audio_bytes(&bytes, d.audio_mixer.sample_rate)
                .unwrap_or_else(|e| {
                    error!("Audio error: {}", e);
                    Vec::new()
                });

            let id = d.add_scene_audio(samples, scene.start_time, scene.duration);
            if let Some(clip) = d
                .tracks
                .iter_mut()
                .flat_map(|t| t.clips.iter_mut())
                .find(|c| c.scene_root == scene.root_id)
            {
                clip.audio_tracks.push(id);
            }
            id
        };

        // Update SceneHandle tracking after dropping Director lock.
        scene.audio_tracks.push(id);

        AudioTrackHandle {
            director: scene.director.clone(),
            id,
        }
    });

    engine.register_fn(
        "animate_volume",
        |track: &mut AudioTrackHandle, start: f64, end: f64, dur: f64, ease: &str| {
            let mut d = match track.lock_director() {
                Ok(d) => d,
                Err(e) => {
                    error!("{}", e);
                    return;
                }
            };
            if let Some(t) = d.audio_mixer.get_track_mut(track.id) {
                let ease_fn = parse_easing(ease);
                t.volume.add_segment(start as f32, end as f32, dur, ease_fn);
            }
        },
    );

    // Audio Analysis (FFT)
    engine.register_fn("bass", |track: &mut AudioTrackHandle, time: f64| -> f64 {
        let d = match track.lock_director() {
            Ok(d) => d,
            Err(_) => return 0.0,
        };
        if let Some(t) = d.audio_mixer.tracks.get(track.id).and_then(|t| t.as_ref()) {
            d.audio_analyzer.bass(&t.samples, time) as f64
        } else {
            0.0
        }
    });

    engine.register_fn("mids", |track: &mut AudioTrackHandle, time: f64| -> f64 {
        let d = match track.lock_director() {
            Ok(d) => d,
            Err(_) => return 0.0,
        };
        if let Some(t) = d.audio_mixer.tracks.get(track.id).and_then(|t| t.as_ref()) {
            d.audio_analyzer.mids(&t.samples, time) as f64
        } else {
            0.0
        }
    });

    engine.register_fn("highs", |track: &mut AudioTrackHandle, time: f64| -> f64 {
        let d = match track.lock_director() {
            Ok(d) => d,
            Err(_) => return 0.0,
        };
        if let Some(t) = d.audio_mixer.tracks.get(track.id).and_then(|t| t.as_ref()) {
            d.audio_analyzer.highs(&t.samples, time) as f64
        } else {
            0.0
        }
    });

    engine.register_fn(
        "get_energy",
        |track: &mut AudioTrackHandle, time: f64, band: &str| -> f64 {
            let d = match track.lock_director() {
                Ok(d) => d,
                Err(_) => return 0.0,
            };
            if let Some(t) = d.audio_mixer.tracks.get(track.id).and_then(|t| t.as_ref()) {
                d.audio_analyzer.get_energy(&t.samples, time, band) as f64
            } else {
                0.0
            }
        },
    );

    engine.register_fn(
        "get_spectrum",
        |track: &mut AudioTrackHandle, time: f64| -> rhai::Array {
            let d = match track.lock_director() {
                Ok(d) => d,
                Err(_) => return rhai::Array::new(),
            };
            if let Some(t) = d.audio_mixer.tracks.get(track.id).and_then(|t| t.as_ref()) {
                d.audio_analyzer
                    .compute_spectrum(&t.samples, time)
                    .into_iter()
                    .map(|v| rhai::Dynamic::from(v as f64))
                    .collect()
            } else {
                rhai::Array::new()
            }
        },
    );

    // Audio Reactive Binding
    // Usage: node.bind_audio(track_id, "bass", "scale")
    // Maps audio energy (0-1) to property values with sensible defaults
    engine.register_fn(
        "bind_audio",
        |node: &mut NodeHandle,
         track_id: i64,
         band: &str,
         property: &str|
         -> Result<(), Box<rhai::EvalAltResult>> {
            let mut d = node.lock_director()?;
            node.ensure_alive(&d)?;
            let track_idx = track_id as usize;
            if d.audio_mixer
                .tracks
                .get(track_idx)
                .and_then(|t| t.as_ref())
                .is_none()
            {
                return Err(format!("Unknown audio track id {}", track_id).into());
            }
            // Default range based on property type
            let (min_val, max_val) = match property {
                "scale" | "scale_x" | "scale_y" => (1.0, 2.0),
                "rotation" => (0.0, 30.0),
                _ => (0.0, 100.0),
            };
            let scene_node = d
                .scene
                .get_node_mut(node.id)
                .ok_or_else(|| format!("Node {} not found", node.id))?;
            scene_node.audio_bindings.push(crate::scene::AudioBinding {
                track_id: track_idx,
                band: band.to_string(),
                property: property.to_string(),
                min_value: min_val,
                max_value: max_val,
                smoothing: 0.3,
                prev_value: min_val,
            });
            Ok(())
        },
    );

    // Get track ID from handle for use with bind_audio
    engine.register_fn("id", |track: &mut AudioTrackHandle| -> i64 {
        track.id as i64
    });

    // Loudness analysis (IT-R BS.1770)
    // Usage: let loudness = track.loudness();
    //        print("LUFS: " + loudness.integrated_lufs);
    engine.register_fn(
        "loudness",
        |track: &mut AudioTrackHandle| -> Result<rhai::Map, Box<rhai::EvalAltResult>> {
            let d = match track.lock_director() {
                Ok(d) => d,
                Err(e) => return Err(e),
            };
            let Some(t) = d.audio_mixer.tracks.get(track.id).and_then(|t| t.as_ref()) else {
                return Err(format!("audio track {} not found", track.id).into());
            };

            let analysis = crate::loudness::analyze_stereo(&t.samples, d.audio_mixer.sample_rate);
            let mut map = rhai::Map::new();
            map.insert("integrated_lufs".into(), analysis.integrated_lufs.into());
            map.insert("true_peak_db".into(), analysis.true_peak_db.into());
            map.insert("loudness_range_lu".into(), analysis.loudness_range_lu.into());
            map.insert("channels".into(), (analysis.channels as i64).into());
            map.insert("sample_rate".into(), (analysis.sample_rate as i64).into());
            Ok(map)
        },
    );

    // ========== PAN ==========
    engine.register_fn("set_pan", |track: &mut AudioTrackHandle, pan: f64| {
        let mut d = match track.lock_director() {
            Ok(d) => d,
            Err(e) => {
                error!("{}", e);
                return;
            }
        };
        if let Some(t) = d.audio_mixer.get_track_mut(track.id) {
            t.pan.current_value = pan as f32;
        }
    });

    engine.register_fn(
        "animate_pan",
        |track: &mut AudioTrackHandle, start: f64, end: f64, dur: f64, ease: &str| {
            let mut d = match track.lock_director() {
                Ok(d) => d,
                Err(e) => {
                    error!("{}", e);
                    return;
                }
            };
            if let Some(t) = d.audio_mixer.get_track_mut(track.id) {
                let ease_fn = parse_easing(ease);
                t.pan.add_segment(start as f32, end as f32, dur, ease_fn);
            }
        },
    );

    // ========== EFFECTS ==========
    fn get_param_f(params: &rhai::Map, key: &str) -> Option<f32> {
        params.get(key).and_then(|v| v.as_float().ok().map(|f| f as f32))
    }
    fn get_param_f2(params: &rhai::Map, k1: &str, k2: &str) -> Option<f32> {
        params
            .get(k1)
            .or_else(|| params.get(k2))
            .and_then(|v| v.as_float().ok().map(|f| f as f32))
    }
    fn get_param_s(params: &rhai::Map, key: &str) -> Option<String> {
        params
            .get(key)
            .and_then(|v| v.clone().into_string().ok())
    }

    engine.register_fn(
        "add_effect",
        |track: &mut AudioTrackHandle, effect_type: &str, params: rhai::Map| -> i64 {
            let mut d = match track.lock_director() {
                Ok(d) => d,
                Err(e) => {
                    error!("{}", e);
                    return -1;
                }
            };
            let sr = d.audio_mixer.sample_rate;
            let Some(t) = d.audio_mixer.get_track_mut(track.id) else {
                return -1;
            };
            let effect: Option<Box<dyn crate::audio_effects::AudioEffect>> = match effect_type {
                "reverb" => {
                    let mut e = Reverb::new(sr);
                    if let Some(v) = get_param_f(&params, "decay") {
                        e.set_parameter("decay", v);
                    }
                    if let Some(v) = get_param_f(&params, "wet") { e.set_parameter("wet", v); }
                    if let Some(v) = get_param_f(&params, "dry") { e.set_parameter("dry", v); }
                    Some(Box::new(e))
                }
                "delay" => {
                    let mut e = Delay::new(sr);
                    if let Some(v) = get_param_f2(&params, "time", "delay_time") {
                        e.set_parameter("delay_time", v);
                    }
                    if let Some(v) = get_param_f(&params, "feedback") { e.set_parameter("feedback", v); }
                    if let Some(v) = get_param_f(&params, "wet") { e.set_parameter("wet", v); }
                    if let Some(v) = get_param_f(&params, "dry") { e.set_parameter("dry", v); }
                    Some(Box::new(e))
                }
                "eq" | "biquad" | "filter" => {
                    let mut e = BiquadEQ::new(sr);
                    if let Some(v) = get_param_s(&params, "type") {
                        let ft = match v.as_str() {
                            "lowpass" | "low_pass" => BiquadType::LowPass,
                            "highpass" | "high_pass" => BiquadType::HighPass,
                            "bandpass" | "band_pass" => BiquadType::BandPass,
                            "peak" | "peaking" => BiquadType::Peak,
                            "notch" => BiquadType::Notch,
                            "lowshelf" | "low_shelf" => BiquadType::LowShelf,
                            "highshelf" | "high_shelf" => BiquadType::HighShelf,
                            _ => BiquadType::Peak,
                        };
                        e.filter_type = ft;
                        e.set_parameter("frequency", e.frequency);
                    }
                    if let Some(v) = get_param_f2(&params, "frequency", "freq") {
                        e.set_parameter("frequency", v);
                    }
                    if let Some(v) = get_param_f2(&params, "q", "resonance") {
                        e.set_parameter("q", v);
                    }
                    if let Some(v) = get_param_f2(&params, "gain", "gain_db") {
                        e.set_parameter("gain", v);
                    }
                    Some(Box::new(e))
                }
                "compressor" => {
                    let mut e = Compressor::new(sr);
                    if let Some(v) = get_param_f(&params, "threshold") { e.set_parameter("threshold", v); }
                    if let Some(v) = get_param_f(&params, "ratio") { e.set_parameter("ratio", v); }
                    if let Some(v) = get_param_f(&params, "knee") { e.set_parameter("knee", v); }
                    if let Some(v) = get_param_f(&params, "attack") { e.set_parameter("attack", v); }
                    if let Some(v) = get_param_f(&params, "release") { e.set_parameter("release", v); }
                    if let Some(v) = get_param_f2(&params, "makeup_gain", "makeup") {
                        e.set_parameter("makeup_gain", v);
                    }
                    Some(Box::new(e))
                }
                _ => {
                    error!("Unknown effect type: {}", effect_type);
                    None
                }
            };

            if let Some(efx) = effect {
                let idx = t.effects.add(efx);
                idx as i64
            } else {
                -1
            }
        },
    );

    engine.register_fn(
        "set_effect_param",
        |track: &mut AudioTrackHandle, effect_idx: i64, param: &str, value: f64| {
            let mut d = match track.lock_director() {
                Ok(d) => d,
                Err(e) => {
                    error!("{}", e);
                    return;
                }
            };
            if let Some(t) = d.audio_mixer.get_track_mut(track.id) {
                if let Some(effect) = t.effects.get_mut(effect_idx as usize) {
                    effect.set_parameter(param, value as f32);
                }
            }
        },
    );

    engine.register_fn(
        "animate_effect_param",
        |track: &mut AudioTrackHandle,
         effect_idx: i64,
         param: &str,
         start: f64,
         end: f64,
         dur: f64,
         ease: &str| {
            let mut d = match track.lock_director() {
                Ok(d) => d,
                Err(e) => {
                    error!("{}", e);
                    return;
                }
            };
            if let Some(t) = d.audio_mixer.get_track_mut(track.id) {
                let ease_fn = parse_easing(ease);
                let mut animated = crate::animation::Animated::new(start as f32);
                animated.add_segment(start as f32, end as f32, dur, ease_fn);
                t.effect_param_animations.push(EffectParamAnimation {
                    effect_idx: effect_idx as usize,
                    param_name: param.to_string(),
                    animated,
                });
            }
        },
    );

    engine.register_fn(
        "remove_effect",
        |track: &mut AudioTrackHandle, effect_idx: i64| {
            let mut d = match track.lock_director() {
                Ok(d) => d,
                Err(e) => {
                    error!("{}", e);
                    return;
                }
            };
            if let Some(t) = d.audio_mixer.get_track_mut(track.id) {
                if effect_idx >= 0 && (effect_idx as usize) < t.effects.len() {
                    t.effects.effects.remove(effect_idx as usize);
                    t.effect_param_animations
                        .retain(|a| a.effect_idx != effect_idx as usize);
                }
            }
        },
    );
}
