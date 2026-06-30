//! # Lifecycle API
//!
//! Director and scene lifecycle management for Rhai scripts.
//!
//! ## Responsibilities
//! - **Director Creation**: `new_director` with various overloads
//! - **Scene Management**: `add_scene`, `add_transition`
//! - **Configuration**: `configure_motion_blur`, `configure_encoder`
//! - **Validation**: rejects malformed transition ranges/durations

use crate::director::{Director, TimelineItem};
use crate::node::BoxNode;
use crate::systems::transitions::{Transition, TransitionType};
use crate::timeline::TrackClip;
use crate::video_wrapper::{get_encoder_info, EncoderMode, RenderMode};
use crate::AssetLoader;
use rhai::Engine;
use std::sync::{Arc, Mutex};
use taffy::prelude::*;
use tracing::warn;

use super::super::types::{ClipHandle, MovieHandle, SceneHandle, TrackHandle};
use super::super::utils::parse_easing;

/// Register lifecycle-related Rhai functions.
pub fn register(engine: &mut Engine, loader: Arc<dyn AssetLoader>, default_mode: RenderMode) {
    // Randomness
    engine.register_fn("rand_float", |min: f64, max: f64| {
        use rand::Rng;
        rand::thread_rng().gen_range(min..max)
    });

    // 1. Director/Movie
    engine.register_type_with_name::<MovieHandle>("Movie");

    // Overload 1: 3 args (Default Preview)
    let l1 = loader.clone();
    engine.register_fn("new_director", move |w: i64, h: i64, fps: i64| {
        let director = Director::new(
            w as i32,
            h as i32,
            fps as u32,
            l1.clone(),
            default_mode,
            None,
        );
        MovieHandle {
            director: Arc::new(Mutex::new(director)),
        }
    });

    // Overload 2: 4 args (Config)
    let l2 = loader.clone();
    engine.register_fn(
        "new_director",
        move |w: i64, h: i64, fps: i64, config: rhai::Map| {
            let mode_str = config
                .get("mode")
                .and_then(|v| v.clone().into_string().ok())
                .unwrap_or_else(|| "preview".to_string());
            let mode = match mode_str.as_str() {
                "export" => RenderMode::Export,
                _ => RenderMode::Preview,
            };
            let director = Director::new(w as i32, h as i32, fps as u32, l2.clone(), mode, None);
            MovieHandle {
                director: Arc::new(Mutex::new(director)),
            }
        },
    );

    engine.register_fn(
        "configure_motion_blur",
        |movie: &mut MovieHandle, samples: i64, shutter_angle: f64| {
            let mut d = match movie.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            d.samples_per_frame = samples as u32;
            d.shutter_angle = shutter_angle as f32;
        },
    );

    // Encoder Configuration
    engine.register_fn(
        "configure_encoder",
        |movie: &mut MovieHandle, mode: &str| {
            let mut d = match movie.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            let encoder_mode = match mode {
                "auto" => EncoderMode::Auto,
                "software" => EncoderMode::Software,
                "nvenc" | "nvidia" => EncoderMode::Nvenc,
                "qsv" | "intel" => EncoderMode::Qsv,
                "amf" | "amd" => EncoderMode::Amf,
                _ => EncoderMode::Auto,
            };
            d.encoder_mode = encoder_mode;

            let info = get_encoder_info(encoder_mode);
            tracing::info!(
                "Encoder configured: {} ({}) - Hardware: {}",
                info.codec,
                info.encoder_name,
                info.is_hardware
            );
        },
    );

    // Get encoder diagnostics with warnings
    engine.register_fn("get_encoder_info", |movie: &mut MovieHandle| -> String {
        let d = match movie.lock_director() {
            Ok(d) => d,
            Err(_) => return String::new(),
        };
        let info = get_encoder_info(d.encoder_mode);

        let available = info.available_encoders.join(", ");
        let current_type = if info.is_hardware {
            "Hardware"
        } else {
            "Software"
        };

        let mut result = format!(
            "Current: {} ({}) - {} | Available: {}",
            info.codec, info.encoder_name, current_type, available
        );

        // Add warnings if any encoders failed
        if !info.warnings.is_empty() {
            result += "\nWarnings:";
            for warning in &info.warnings {
                result += &format!("\n  - {}", warning);
            }
        }

        result
    });

    // 2. Scene Management
    engine.register_type_with_name::<SceneHandle>("Scene");
    engine.register_type_with_name::<TrackHandle>("Track");
    engine.register_type_with_name::<ClipHandle>("Clip");

    engine.register_fn("add_scene", |movie: &mut MovieHandle, duration: f64| -> SceneHandle {
        add_scene_internal(movie, duration, None)
    });

    engine.register_fn(
        "add_scene_on_track",
        |movie: &mut MovieHandle, track: TrackHandle, duration: f64| -> SceneHandle {
            add_scene_internal(movie, duration, Some(track.track_idx))
        },
    );

    // ========== TRACK MANAGEMENT ==========
    engine.register_fn("add_track", |movie: &mut MovieHandle| -> TrackHandle {
        let mut d = match movie.lock_director() {
            Ok(d) => d,
            Err(_) => return TrackHandle { director: movie.director.clone(), track_idx: usize::MAX },
        };
        let idx = d.tracks.len();
        d.tracks.push(crate::timeline::Track::new(&format!("Track {}", idx)));
        TrackHandle {
            director: movie.director.clone(),
            track_idx: idx,
        }
    });

    engine.register_fn("add_track", |movie: &mut MovieHandle, name: &str| -> TrackHandle {
        let mut d = match movie.lock_director() {
            Ok(d) => d,
            Err(_) => return TrackHandle { director: movie.director.clone(), track_idx: usize::MAX },
        };
        let idx = d.tracks.len();
        d.tracks.push(crate::timeline::Track::new(name));
        TrackHandle {
            director: movie.director.clone(),
            track_idx: idx,
        }
    });

    engine.register_fn(
        "add_clip",
        |track: &mut TrackHandle, scene: SceneHandle, start_time: f64, duration: f64| -> ClipHandle {
            let mut d = match track.lock_director() {
                Ok(d) => d,
                Err(_) => {
                    return ClipHandle {
                        director: track.director.clone(),
                        track_idx: track.track_idx,
                        clip_idx: usize::MAX,
                    };
                }
            };
            if track.track_idx >= d.tracks.len() {
                return ClipHandle {
                    director: track.director.clone(),
                    track_idx: track.track_idx,
                    clip_idx: usize::MAX,
                };
            }
            let clip = TrackClip::new(scene.root_id, start_time, duration);
            d.tracks[track.track_idx].clips.push(clip);
            let clip_idx = d.tracks[track.track_idx].clips.len() - 1;
            ClipHandle {
                director: track.director.clone(),
                track_idx: track.track_idx,
                clip_idx,
            }
        },
    );

    engine.register_fn("set_clip_start", |clip: &mut ClipHandle, start_time: f64| {
        let mut d = match clip.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(c) = d
            .tracks
            .get_mut(clip.track_idx)
            .and_then(|t| t.clips.get_mut(clip.clip_idx))
        {
            c.start_time = start_time;
        }
    });

    engine.register_fn(
        "set_clip_range",
        |clip: &mut ClipHandle, source_start: f64, source_end: f64| {
            let mut d = match clip.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(c) = d
                .tracks
                .get_mut(clip.track_idx)
                .and_then(|t| t.clips.get_mut(clip.clip_idx))
            {
                c.source_start = source_start;
                c.source_end = source_end;
            }
        },
    );

    engine.register_fn("set_clip_speed", |clip: &mut ClipHandle, speed: f64| {
        let mut d = match clip.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(c) = d
            .tracks
            .get_mut(clip.track_idx)
            .and_then(|t| t.clips.get_mut(clip.clip_idx))
        {
            c.speed = speed;
        }
    });

    engine.register_fn("remove_clip", |clip: &mut ClipHandle| {
        let mut d = match clip.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(track) = d.tracks.get_mut(clip.track_idx) {
            if clip.clip_idx < track.clips.len() {
                track.clips.remove(clip.clip_idx);
            }
        }
    });

    // ========== TRACK CONTROL ==========
    engine.register_fn("set_track_enabled", |track: &mut TrackHandle, enabled: bool| {
        let mut d = match track.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(t) = d.tracks.get_mut(track.track_idx) {
            t.enabled = enabled;
        }
    });

    engine.register_fn("set_track_opacity", |track: &mut TrackHandle, opacity: f64| {
        let mut d = match track.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(t) = d.tracks.get_mut(track.track_idx) {
            t.opacity = opacity as f32;
        }
    });

    engine.register_fn("get_track", |movie: &mut MovieHandle, idx: i64| -> TrackHandle {
        TrackHandle {
            director: movie.director.clone(),
            track_idx: idx as usize,
        }
    });

    engine.register_fn("remove_track", |movie: &mut MovieHandle, idx: i64| {
        let mut d = match movie.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if idx >= 0 && (idx as usize) < d.tracks.len() {
            d.tracks.remove(idx as usize);
        }
    });

    engine.register_fn("set_track_name", |track: &mut TrackHandle, name: &str| {
        let mut d = match track.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(t) = d.tracks.get_mut(track.track_idx) {
            t.name = name.to_string();
        }
    });

    engine.register_fn(
        "get_track_clip_count",
        |track: &mut TrackHandle| -> i64 {
            let d = match track.lock_director() {
                Ok(d) => d,
                Err(_) => return 0,
            };
            d.tracks
                .get(track.track_idx)
                .map(|t| t.clips.len() as i64)
                .unwrap_or(0)
        },
    );

    engine.register_fn("list_clips", |track: &mut TrackHandle| -> rhai::Array {
        let d = match track.lock_director() {
            Ok(d) => d,
            Err(_) => return rhai::Array::new(),
        };
        let mut arr = rhai::Array::new();
        if let Some(t) = d.tracks.get(track.track_idx) {
            for (i, clip) in t.clips.iter().enumerate() {
                let mut map = rhai::Map::new();
                map.insert("index".into(), rhai::Dynamic::from_int(i as i64));
                map.insert("start_time".into(), rhai::Dynamic::from_float(clip.start_time));
                map.insert("duration".into(), rhai::Dynamic::from_float(clip.duration));
                map.insert("source_start".into(), rhai::Dynamic::from_float(clip.source_start));
                map.insert("source_end".into(), rhai::Dynamic::from_float(clip.source_end));
                map.insert("speed".into(), rhai::Dynamic::from_float(clip.speed));
                map.insert("z_index".into(), rhai::Dynamic::from_int(clip.z_index as i64));
                if let Some(ref name) = clip.name {
                    map.insert("name".into(), rhai::Dynamic::from(name.clone()));
                }
                arr.push(rhai::Dynamic::from(map));
            }
        }
        drop(d);
        arr
    });

    engine.register_fn("set_clip_fade_in", |clip: &mut ClipHandle, duration: f64| {
        let mut d = match clip.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(track) = d.tracks.get_mut(clip.track_idx) {
            if let Some(c) = track.clips.get_mut(clip.clip_idx) {
                c.fade_in = duration.max(0.0);
            }
        }
    });

    engine.register_fn("set_clip_fade_out", |clip: &mut ClipHandle, duration: f64| {
        let mut d = match clip.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(track) = d.tracks.get_mut(clip.track_idx) {
            if let Some(c) = track.clips.get_mut(clip.clip_idx) {
                c.fade_out = duration.max(0.0);
            }
        }
    });

    engine.register_fn("set_clip_z_index", |clip: &mut ClipHandle, z_index: i64| {
        let mut d = match clip.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(track) = d.tracks.get_mut(clip.track_idx) {
            if let Some(c) = track.clips.get_mut(clip.clip_idx) {
                c.z_index = z_index as i32;
            }
        }
    });

    engine.register_fn(
        "add_transition",
        |movie: &mut MovieHandle,
         from: SceneHandle,
         to: SceneHandle,
         type_str: &str,
         duration: f64,
         easing_str: &str| {
            add_transition_internal(movie, from, to, type_str, duration, easing_str, None);
        },
    );

    // Wave transition with amplitude and frequency parameters
    engine.register_fn(
        "add_wave_transition",
        |movie: &mut MovieHandle,
         from: SceneHandle,
         to: SceneHandle,
         duration: f64,
         easing_str: &str,
         amplitude: f64,
         frequency: f64| {
            let kind = TransitionType::Wave {
                amplitude: amplitude as f32,
                frequency: frequency as f32,
            };
            add_transition_with_kind(movie, from, to, duration, easing_str, kind);
        },
    );

    // Glitch transition with intensity parameter
    engine.register_fn(
        "add_glitch_transition",
        |movie: &mut MovieHandle,
         from: SceneHandle,
         to: SceneHandle,
         duration: f64,
         easing_str: &str,
         intensity: f64| {
            let kind = TransitionType::Glitch {
                intensity: intensity as f32,
            };
            add_transition_with_kind(movie, from, to, duration, easing_str, kind);
        },
    );

    // Iris transition with start and end radius parameters
    engine.register_fn(
        "add_iris_transition",
        |movie: &mut MovieHandle,
         from: SceneHandle,
         to: SceneHandle,
         duration: f64,
         easing_str: &str,
         start_radius: f64,
         end_radius: f64| {
            let kind = TransitionType::Iris {
                start_radius: start_radius as f32,
                end_radius: end_radius as f32,
            };
            add_transition_with_kind(movie, from, to, duration, easing_str, kind);
        },
    );

    // Spiral transition with rotations parameter
    engine.register_fn(
        "add_spiral_transition",
        |movie: &mut MovieHandle,
         from: SceneHandle,
         to: SceneHandle,
         duration: f64,
         easing_str: &str,
         rotations: f64| {
            let kind = TransitionType::Spiral {
                rotations: rotations as f32,
            };
            add_transition_with_kind(movie, from, to, duration, easing_str, kind);
        },
    );
}

fn add_scene_internal(
    movie: &mut MovieHandle,
    duration: f64,
    target_track_idx: Option<usize>,
) -> SceneHandle {
    let mut d = match movie.lock_director() {
        Ok(d) => d,
        Err(_) => return SceneHandle {
            director: movie.director.clone(),
            root_id: 0,
            start_time: 0.0,
            duration: 0.0,
            audio_tracks: Vec::new(),
        },
    };
    let track_idx = target_track_idx.unwrap_or(d.default_track_idx);
    let start_time = {
        if d.tracks.is_empty() || track_idx >= d.tracks.len() || d.tracks[track_idx].clips.is_empty() {
            d.timeline
                .last()
                .map(|i| i.start_time + i.duration)
                .unwrap_or(0.0)
        } else {
            let clips = &d.tracks[track_idx].clips;
            clips
                .last()
                .map(|c| c.start_time + c.duration)
                .unwrap_or(0.0)
        }
    };

    let mut root = BoxNode::new();
    root.style.size = taffy::geometry::Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    let id = d.scene.add_node(Box::new(root));

    // Add to target track
    let clip = TrackClip::new(id, start_time, duration);
    if track_idx < d.tracks.len() {
        d.tracks[track_idx].clips.push(clip);
    }

    // Add to legacy timeline
    let item = TimelineItem {
        scene_root: id,
        name: None,
        start_time,
        duration,
        z_index: 0,
        audio_tracks: Vec::new(),
    };
    d.timeline.push(item);

    SceneHandle {
        director: movie.director.clone(),
        root_id: id,
        start_time,
        duration,
        audio_tracks: Vec::new(),
    }
}

// Helper function for standard transitions
fn add_transition_internal(
    movie: &mut MovieHandle,
    from: SceneHandle,
    to: SceneHandle,
    type_str: &str,
    duration: f64,
    easing_str: &str,
    params: Option<rhai::Map>,
) {
    let kind = match type_str {
        "fade" => TransitionType::Fade,
        "slide_left" | "slide-left" => TransitionType::SlideLeft,
        "slide_right" | "slide-right" => TransitionType::SlideRight,
        "wipe_left" | "wipe-left" => TransitionType::WipeLeft,
        "wipe_right" | "wipe-right" => TransitionType::WipeRight,
        "circle_open" | "circle-open" => TransitionType::CircleOpen,
        "wave" => TransitionType::Wave {
            amplitude: params.as_ref().and_then(|m| m.get("amplitude").and_then(|v| v.as_float().ok().map(|f| f as f32))).unwrap_or(0.1),
            frequency: params.as_ref().and_then(|m| m.get("frequency").and_then(|v| v.as_float().ok().map(|f| f as f32))).unwrap_or(5.0),
        },
        "glitch" => TransitionType::Glitch {
            intensity: params.as_ref().and_then(|m| m.get("intensity").and_then(|v| v.as_float().ok().map(|f| f as f32))).unwrap_or(0.5),
        },
        "iris" => TransitionType::Iris {
            start_radius: params.as_ref().and_then(|m| m.get("start_radius").and_then(|v| v.as_float().ok().map(|f| f as f32))).unwrap_or(0.0),
            end_radius: params.as_ref().and_then(|m| m.get("end_radius").and_then(|v| v.as_float().ok().map(|f| f as f32))).unwrap_or(500.0),
        },
        "spiral" => TransitionType::Spiral {
            rotations: params.as_ref().and_then(|m| m.get("rotations").and_then(|v| v.as_float().ok().map(|f| f as f32))).unwrap_or(2.0),
        },
        "zoom_in" | "zoom-in" | "zoomin" => TransitionType::ZoomIn,
        "radial_wipe" | "radial-wipe" | "radialwipe" => TransitionType::RadialWipe,
        "pixelate" | "pixelate_dissolve" | "pixelate-dissolve" => TransitionType::PixelateDissolve,
        "cross_zoom" | "cross-zoom" | "crosszoom" => TransitionType::CrossZoom,
        "wind" => TransitionType::Wind,
        "burn" => TransitionType::Burn,
        _ => TransitionType::Fade,
    };
    add_transition_with_kind(movie, from, to, duration, easing_str, kind);
}

// Helper function to add a transition with a specific kind
fn add_transition_with_kind(
    movie: &mut MovieHandle,
    from: SceneHandle,
    to: SceneHandle,
    duration: f64,
    easing_str: &str,
    kind: TransitionType,
) {
    let mut d = match movie.lock_director() {
        Ok(d) => d,
        Err(_) => return,
    };

    if duration <= 0.0 {
        warn!(
            "Ignoring transition with non-positive duration: {}",
            duration
        );
        return;
    }

    // Find indices
    let from_idx = d.timeline.iter().position(|i| i.scene_root == from.root_id);
    let to_idx = d.timeline.iter().position(|i| i.scene_root == to.root_id);

    if let (Some(f_idx), Some(t_idx)) = (from_idx, to_idx) {
        if t_idx <= f_idx {
            warn!(
                from_scene = f_idx,
                to_scene = t_idx,
                "Ignoring transition: target scene must come after source scene"
            );
            return;
        }
        if duration > d.timeline[f_idx].duration {
            warn!(
                duration,
                from_duration = d.timeline[f_idx].duration,
                "Ignoring transition: duration exceeds source scene duration"
            );
            return;
        }

        // Ripple Left Logic
        // We shift 'to' scene and all subsequent scenes (index >= t_idx) left by duration.

        for i in t_idx..d.timeline.len() {
            d.timeline[i].start_time -= duration;

            // Sync Audio
            let audio_ids = d.timeline[i].audio_tracks.clone();
            for track_id in audio_ids {
                if let Some(track) = d.audio_mixer.get_track_mut(track_id) {
                    track.start_time -= duration;
                }
            }
        }

        let easing = parse_easing(easing_str);

        let start_time = d.timeline[t_idx].start_time;

        let transition = Transition {
            from_scene_idx: f_idx,
            to_scene_idx: t_idx,
            start_time,
            duration,
            kind,
            easing,
        };

        d.transitions.push(transition);
    }
}
