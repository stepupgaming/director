//! # Lifecycle API
//!
//! Director and scene lifecycle management for Rhai scripts.
//!
//! ## Responsibilities
//! - **Director Creation**: `new_director` with various overloads
//! - **Scene Management**: `add_scene`, `add_transition`
//! - **Configuration**: `configure_motion_blur`, `configure_encoder`
//! - **Validation**: rejects malformed transition ranges/durations

use crate::director::Director;
use crate::node::BoxNode;
use crate::systems::transitions::{Transition, TransitionType};
use crate::timeline::TrackClip;
use crate::video_wrapper::{get_encoder_info, EncoderMode, RenderMode};
use crate::AssetLoader;
use rhai::Engine;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::sync::{Arc, Mutex};
use std::sync::OnceLock;
use taffy::prelude::*;
use tracing::warn;

/// Process-global seedable RNG used by `rand_float`. Defaults to a fixed seed so
/// renders are reproducible; `set_seed` re-seeds it. This makes procedurally
/// generated scenes deterministic across runs (required for stable golden tests
/// and agent iteration).
static SEEDED_RNG: OnceLock<Mutex<StdRng>> = OnceLock::new();

fn seeded_rng() -> &'static Mutex<StdRng> {
    SEEDED_RNG.get_or_init(|| Mutex::new(StdRng::seed_from_u64(0xC0FFEE)))
}

use super::super::types::{ClipHandle, MovieHandle, SceneHandle, SequenceHandle, TrackHandle};
use super::super::utils::parse_easing_result;

/// Register lifecycle-related Rhai functions.
pub fn register(engine: &mut Engine, loader: Arc<dyn AssetLoader>, default_mode: RenderMode) {
    // Randomness — uses a seedable process-global RNG so output is reproducible.
    engine.register_fn("rand_float", |min: f64, max: f64| -> f64 {
        seeded_rng().lock().unwrap().gen_range(min..max)
    });

    // Re-seed the global RNG so agent runs / golden tests are deterministic.
    engine.register_fn("set_seed", |seed: i64| {
        let mut rng = seeded_rng().lock().unwrap();
        *rng = StdRng::seed_from_u64(seed as u64);
    });

    // ========== NOISE UTILITY ==========
    // Simple 2D value noise for organic animation (mirrors Remotion's @remotion/noise)
    engine.register_fn("noise2d", |x: f64, y: f64| -> f64 {
        noise2d_impl(x, y)
    });

    // 3D noise (time-based: noise3d(x, y, t))
    engine.register_fn("noise3d", |x: f64, y: f64, z: f64| -> f64 {
        // Interpolate between 2D noise slices
        let n1 = noise2d_impl(x, y + z * 17.0);
        let n2 = noise2d_impl(x + z * 13.0, y);
        (n1 + n2) / 2.0
    });

    // ========== INTERPOLATE COLORS UTILITY ==========
    // Maps progress (0..1) to a color between two hex colors
    engine.register_fn("interpolate_color", |from: &str, to: &str, progress: f64| -> String {
        let c1 = crate::scripting::utils::parse_hex_color(from);
        let c2 = crate::scripting::utils::parse_hex_color(to);
        if let (Some(a), Some(b)) = (c1, c2) {
            let t = progress.clamp(0.0, 1.0) as f32;
            let mixed = crate::types::Color::new(
                a.r + (b.r - a.r) * t,
                a.g + (b.g - a.g) * t,
                a.b + (b.b - a.b) * t,
                a.a + (b.a - a.a) * t,
            );
            format!("#{:02x}{:02x}{:02x}{:02x}",
                (mixed.r * 255.0) as u8,
                (mixed.g * 255.0) as u8,
                (mixed.b * 255.0) as u8,
                (mixed.a * 255.0) as u8)
        } else {
            from.to_string()
        }
    });

    // ========== MEASURE TEXT UTILITY ==========
    // Returns the approximate width of text in pixels for a given font size
    engine.register_fn("measure_text", |text: &str, font_size: f64| -> f64 {
        // Approximate: average character width is ~0.5 * font_size for most fonts
        let char_count = text.chars().count() as f64;
        char_count * font_size * 0.5
    });

    // ========== LOOP SCENE PRIMITIVE ==========
    // Repeat a scene's content N times as track clips (copies audio_tracks).
    engine.register_fn("add_loop", |movie: &mut MovieHandle, scene: SceneHandle, count: i64| {
        let mut d = match movie.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        let n = count.max(1) as usize;
        let scene_duration = scene.duration;
        let root_id = scene.root_id;
        let start = scene.start_time + scene_duration;
        let audio_tracks = scene.audio_tracks.clone();
        for i in 1..n {
            let mut clip = TrackClip::new(
                root_id,
                start + ((i - 1) as f64) * scene_duration,
                scene_duration,
            );
            // i starts at 1: first extra copy at start, then +duration each
            clip.start_time = start + ((i - 1) as f64) * scene_duration;
            clip.name = Some(format!("loop_{}", i));
            clip.audio_tracks = audio_tracks.clone();
            d.push_clip(None, clip);
        }
    });

    // ========== FREEZE SCENE PRIMITIVE ==========
    // Hold a frozen source time after the scene ends.
    engine.register_fn("add_freeze", |movie: &mut MovieHandle, scene: SceneHandle, freeze_at: f64, hold_duration: f64| {
        let mut d = match movie.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if hold_duration <= 0.0 {
            return;
        }
        let freeze_start = scene.start_time + scene.duration;
        let mut clip = TrackClip::new(scene.root_id, freeze_start, hold_duration);
        clip.name = Some("freeze".to_string());
        clip.freeze_time = Some(freeze_at);
        clip.audio_tracks = scene.audio_tracks.clone();
        d.push_clip(None, clip);
    });

    engine.register_fn(
        "set_clip_freeze",
        |clip: &mut ClipHandle, freeze_at: f64| {
            let mut d = match clip.lock_director() {
                Ok(d) => d,
                Err(_) => return,
            };
            if let Some(c) = d
                .tracks
                .get_mut(clip.track_idx)
                .and_then(|t| t.clips.get_mut(clip.clip_idx))
            {
                c.freeze_time = Some(freeze_at);
            }
        },
    );

    engine.register_fn("clear_clip_freeze", |clip: &mut ClipHandle| {
        let mut d = match clip.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        if let Some(c) = d
            .tracks
            .get_mut(clip.track_idx)
            .and_then(|t| t.clips.get_mut(clip.clip_idx))
        {
            c.freeze_time = None;
        }
    });

    // ========== SERIES / SEQUENCE HELPERS ==========
    // These mirror Remotion's <Series> and <Sequence> primitives for
    // sequential scene building without manual start-time bookkeeping.

    // `add_sequence(movie, duration)` — creates a scene and returns a SequenceHandle.
    // Use `sequence.next(duration)` to chain scenes back-to-back.
    engine.register_type_with_name::<super::super::types::SequenceHandle>("Sequence");

    engine.register_fn(
        "add_sequence",
        |movie: &mut MovieHandle| -> super::super::types::SequenceHandle {
            super::super::types::SequenceHandle {
                director: movie.director.clone(),
                current_time: 0.0,
            }
        },
    );

    // `sequence.next(duration)` — adds a scene at the current sequence time,
    // then advances the cursor by `duration`. Returns a SceneHandle.
    engine.register_fn(
        "next",
        |seq: &mut super::super::types::SequenceHandle,
         duration: f64|
         -> Result<SceneHandle, Box<rhai::EvalAltResult>> {
            let scene = add_scene_internal_at(seq, duration);
            seq.current_time += duration;
            Ok(scene)
        },
    );

    // `sequence.next_with_overlap(duration, overlap)` — adds a scene but
    // only advances the cursor by `duration - overlap` (for transitions).
    engine.register_fn(
        "next_with_overlap",
        |seq: &mut super::super::types::SequenceHandle,
         duration: f64,
         overlap: f64|
         -> Result<SceneHandle, Box<rhai::EvalAltResult>> {
            let scene = add_scene_internal_at(seq, duration);
            seq.current_time += (duration - overlap).max(0.0);
            Ok(scene)
        },
    );

    // `sequence.skip(duration)` — advances the cursor without adding a scene
    // (creates a gap/blank period in the timeline).
    engine.register_fn(
        "skip",
        |seq: &mut super::super::types::SequenceHandle, duration: f64| {
            seq.current_time += duration;
        },
    );

    // `sequence.current_time()` — returns the current cursor position.
    engine.register_fn(
        "current_time",
        |seq: &mut super::super::types::SequenceHandle| -> f64 {
            seq.current_time
        },
    );

    // 1. Director/Movie
    engine.register_type_with_name::<MovieHandle>("Movie");

    // Overload 1: 3 args integer fps (Default Preview)
    let l1 = loader.clone();
    engine.register_fn("new_director", move |w: i64, h: i64, fps: i64| {
        let director = Director::new(
            w as i32,
            h as i32,
            fps as f64,
            l1.clone(),
            default_mode,
            None,
        );
        MovieHandle {
            director: Arc::new(Mutex::new(director)),
        }
    });

    // Overload: fractional fps (23.976, 29.97, …)
    let l1f = loader.clone();
    engine.register_fn("new_director", move |w: i64, h: i64, fps: f64| {
        let director = Director::new(
            w as i32,
            h as i32,
            fps,
            l1f.clone(),
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
            let director = Director::new(w as i32, h as i32, fps as f64, l2.clone(), mode, None);
            MovieHandle {
                director: Arc::new(Mutex::new(director)),
            }
        },
    );

    let l2f = loader.clone();
    engine.register_fn(
        "new_director",
        move |w: i64, h: i64, fps: f64, config: rhai::Map| {
            let mode_str = config
                .get("mode")
                .and_then(|v| v.clone().into_string().ok())
                .unwrap_or_else(|| "preview".to_string());
            let mode = match mode_str.as_str() {
                "export" => RenderMode::Export,
                _ => RenderMode::Preview,
            };
            let director = Director::new(w as i32, h as i32, fps, l2f.clone(), mode, None);
            MovieHandle {
                director: Arc::new(Mutex::new(director)),
            }
        },
    );

    engine.register_fn("movie_fps", |movie: &mut MovieHandle| -> f64 {
        movie
            .lock_director()
            .map(|d| d.fps)
            .unwrap_or(0.0)
    });

    engine.register_fn(
        "frame_to_time",
        |movie: &mut MovieHandle, frame: i64| -> f64 {
            movie
                .lock_director()
                .map(|d| d.frame_to_time(frame.max(0) as u64))
                .unwrap_or(0.0)
        },
    );

    engine.register_fn(
        "time_to_frame",
        |movie: &mut MovieHandle, time: f64| -> i64 {
            movie
                .lock_director()
                .map(|d| d.time_to_frame(time) as i64)
                .unwrap_or(0)
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
    engine.register_fn("update", |movie: &mut MovieHandle, time: f64| {
        let mut d = match movie.lock_director() {
            Ok(d) => d,
            Err(_) => return,
        };
        d.update(time, None);
    });

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
         easing_str: &str|
         -> Result<(), Box<rhai::EvalAltResult>> {
            add_transition_internal(movie, from, to, type_str, duration, easing_str, None)
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
         frequency: f64|
         -> Result<(), Box<rhai::EvalAltResult>> {
            let kind = TransitionType::Wave {
                amplitude: amplitude as f32,
                frequency: frequency as f32,
            };
            add_transition_with_kind(movie, from, to, duration, easing_str, kind)
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
         intensity: f64|
         -> Result<(), Box<rhai::EvalAltResult>> {
            let kind = TransitionType::Glitch {
                intensity: intensity as f32,
            };
            add_transition_with_kind(movie, from, to, duration, easing_str, kind)
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
         end_radius: f64|
         -> Result<(), Box<rhai::EvalAltResult>> {
            let kind = TransitionType::Iris {
                start_radius: start_radius as f32,
                end_radius: end_radius as f32,
            };
            add_transition_with_kind(movie, from, to, duration, easing_str, kind)
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
         rotations: f64|
         -> Result<(), Box<rhai::EvalAltResult>> {
            let kind = TransitionType::Spiral {
                rotations: rotations as f32,
            };
            add_transition_with_kind(movie, from, to, duration, easing_str, kind)
        },
    );
}

/// Helper: 2D value noise implementation (used by noise2d and noise3d Rhai functions).
fn noise2d_impl(x: f64, y: f64) -> f64 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
    let hash = |i: i32, j: i32| -> f64 {
        let n = (i.wrapping_mul(374761393).wrapping_add(j.wrapping_mul(668265263))) as u32;
        let n = (n ^ (n >> 13)).wrapping_mul(1274126177);
        ((n ^ (n >> 16)) as f64) / (u32::MAX as f64) * 2.0 - 1.0
    };
    let a = hash(ix, iy);
    let b = hash(ix + 1, iy);
    let c = hash(ix, iy + 1);
    let d = hash(ix + 1, iy + 1);
    let sx = smooth(fx);
    let sy = smooth(fy);
    let ab = a + (b - a) * sx;
    let cd = c + (d - c) * sx;
    ab + (cd - ab) * sy
}

/// Add a scene at a specific start time (used by Sequence builder).
fn add_scene_internal_at(
    seq: &SequenceHandle,
    duration: f64,
) -> SceneHandle {
    let mut d = match seq.lock_director() {
        Ok(d) => d,
        Err(_) => return SceneHandle {
            director: seq.director.clone(),
            root_id: crate::types::NodeId::default(),
            start_time: 0.0,
            duration: 0.0,
            audio_tracks: Vec::new(),
        },
    };
    let track_idx = d.ensure_default_track();
    let start_time = seq.current_time;

    let mut root = BoxNode::new();
    root.style.size = taffy::geometry::Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    let id = d.scene.add_node(Box::new(root));

    d.push_clip(Some(track_idx), TrackClip::new(id, start_time, duration));

    SceneHandle {
        director: seq.director.clone(),
        root_id: id,
        start_time,
        duration,
        audio_tracks: Vec::new(),
    }
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
            root_id: crate::types::NodeId::default(),
            start_time: 0.0,
            duration: 0.0,
            audio_tracks: Vec::new(),
        },
    };
    let track_idx = target_track_idx.unwrap_or_else(|| d.ensure_default_track());
    let start_time = if track_idx < d.tracks.len() {
        d.tracks[track_idx]
            .clips
            .last()
            .map(|c| c.end_time())
            .unwrap_or(0.0)
    } else {
        0.0
    };

    let mut root = BoxNode::new();
    root.style.size = taffy::geometry::Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    let id = d.scene.add_node(Box::new(root));

    d.push_clip(Some(track_idx), TrackClip::new(id, start_time, duration));

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
) -> Result<(), Box<rhai::EvalAltResult>> {
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
        "dissolve" => TransitionType::Dissolve,
        "flip" => TransitionType::Flip,
        "clock_wipe" | "clock-wipe" | "clockwipe" => TransitionType::ClockWipe,
        "cube" => TransitionType::Cube,
        _ => return Err(crate::api_catalog::unknown_transition_message(type_str).into()),
    };
    if let Some(map) = params.as_ref() {
        if let Some(entry) = crate::api_catalog::find_transition(type_str) {
            let mut unknown = Vec::new();
            for key in map.keys() {
                let key = key.as_str();
                if !entry.params.iter().any(|(param, _)| *param == key) {
                    unknown.push(key.to_string());
                }
            }
            if !unknown.is_empty() {
                let valid: Vec<&str> = entry.params.iter().map(|(param, _)| *param).collect();
                let valid_list = if valid.is_empty() {
                    "(none)".to_string()
                } else {
                    valid.join(", ")
                };
                return Err(format!(
                    "unknown parameter '{}' for transition '{type_str}'; valid: {valid_list}",
                    unknown.join("', '")
                )
                .into());
            }
        }
    }
    add_transition_with_kind(movie, from, to, duration, easing_str, kind)
}

// Helper function to add a transition with a specific kind
fn add_transition_with_kind(
    movie: &mut MovieHandle,
    from: SceneHandle,
    to: SceneHandle,
    duration: f64,
    easing_str: &str,
    kind: TransitionType,
) -> Result<(), Box<rhai::EvalAltResult>> {
    let easing = parse_easing_result(easing_str)?;
    let mut d = movie.lock_director()?;

    if duration <= 0.0 {
        warn!(
            "Ignoring transition with non-positive duration: {}",
            duration
        );
        return Ok(());
    }

    // Flat indices match Director::flat_timeline (track order, then clip order).
    let flat = d.flat_timeline();
    let from_idx = flat.iter().position(|i| i.scene_root == from.root_id);
    let to_idx = flat.iter().position(|i| i.scene_root == to.root_id);

    if let (Some(f_idx), Some(t_idx)) = (from_idx, to_idx) {
        if t_idx <= f_idx {
            warn!(
                from_scene = f_idx,
                to_scene = t_idx,
                "Ignoring transition: target scene must come after source scene"
            );
            return Ok(());
        }
        if duration > flat[f_idx].duration {
            warn!(
                duration,
                from_duration = flat[f_idx].duration,
                "Ignoring transition: duration exceeds source scene duration"
            );
            return Ok(());
        }

        // Ripple left: shift 'to' clip and all later flat clips left by duration.
        // Walk tracks/clips in the same order as flat_timeline.
        let mut audio_shift: Vec<usize> = Vec::new();
        let mut flat_i = 0usize;
        for track in d.tracks.iter_mut() {
            for clip in track.clips.iter_mut() {
                if flat_i >= t_idx {
                    clip.start_time -= duration;
                    audio_shift.extend(clip.audio_tracks.iter().copied());
                }
                flat_i += 1;
            }
        }
        for track_id in audio_shift {
            if let Some(audio) = d.audio_mixer.get_track_mut(track_id) {
                audio.start_time -= duration;
            }
        }

        // Recompute start after ripple.
        let start_time = d
            .flat_timeline()
            .get(t_idx)
            .map(|i| i.start_time)
            .unwrap_or(0.0);

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
    Ok(())
}
