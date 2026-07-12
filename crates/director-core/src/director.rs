//! # Director Module
//!
//! The central orchestrator for the video rendering engine.
//!
//!
//! ## Responsibilities
//! - **Timeline Management**: Maintains a `Vec<TimelineItem>` of scenes.
//! - **Update Loop**: Drives animation, audio sync, and scene transitions.
//! - **Scene Coordination**: Manages active scenes and their time ranges.
//! - **Audio Reactivity**: Applies track-relative FFT bindings with start/duration windows.
//!
//! ## Key Types
//! - `Director`: The god object that owns timeline, assets, and context.
//! - `TimelineItem`: A scene with start time, duration, optional name, and transitions.
//! - `DirectorContext`: Shared state for nested compositions.

use rayon::prelude::*;
use crate::audio::{AudioAnalyzer, AudioMixer, AudioTrack};
use crate::scene::SceneGraph;
use crate::systems::assets::AssetManager;
use crate::systems::transitions::Transition;
use crate::timeline::Track;
use crate::types::NodeId;
use crate::video_wrapper::{EncoderMode, RenderMode};
use crate::AssetLoader;
use skia_safe::textlayout::{FontCollection, TypefaceFontProvider};
use skia_safe::{Data, FontMgr};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::instrument;

/// Shared resources context that can be passed between Directors (e.g. for sub-compositions).
#[derive(Clone)]
pub struct DirectorContext {
    pub assets: AssetManager,
}

/// Represents a scene (or clip) on the timeline.
#[derive(Clone, Debug)]
pub struct TimelineItem {
    /// The root node of this scene.
    pub scene_root: NodeId,
    /// Optional scene label (used by UIs and diagnostics).
    pub name: Option<String>,
    /// The global start time in seconds.
    pub start_time: f64,
    /// The duration of the scene in seconds.
    pub duration: f64,
    /// Z-index for rendering order.
    pub z_index: i32,
    /// Associated audio tracks.
    pub audio_tracks: Vec<usize>,
    /// When `Some(t)`, the renderer keeps sampling the scene at local time
    /// `t` for the entire `duration` window instead of advancing — the
    /// canonical "freeze frame" mechanism used by `add_freeze`.
    pub freeze_time: Option<f64>,
}

/// The central engine coordinator.
///
/// `Director` manages the Scene Graph (nodes), Timeline (sequencing),
/// and shared resources (Assets, Fonts, Audio).
#[derive(Clone)]
pub struct Director {
    /// The Scene Graph.
    pub scene: SceneGraph,
    /// The timeline of scenes (legacy, replaced by tracks below).
    pub timeline: Vec<TimelineItem>,
    /// Multi-track timeline (replaces legacy timeline). Each track is a compositing layer.
    pub tracks: Vec<Track>,
    /// Index of the default track that `add_scene` pushes to.
    pub default_track_idx: usize,
    /// Active transitions.
    pub transitions: Vec<Transition>,
    /// Output width in pixels.
    pub width: i32,
    /// Output height in pixels.
    pub height: i32,
    /// Frames Per Second. Stored as `f64` so 23.976, 29.97, and other
    /// NTSC-accurate rates round-trip exactly rather than truncating to a `u32`.
    /// Use [`Director::frame_to_time`] / [`Director::time_to_frame`] for the
    /// canonical frame↔time conversions.
    pub fps: f64,
    /// Number of sub-frame samples for motion blur (default: 1).
    pub samples_per_frame: u32,
    /// Shutter angle in degrees (0.0 to 360.0). Default: 180.0.
    pub shutter_angle: f32,
    /// Render Mode (Preview or Export).
    pub render_mode: RenderMode,
    /// Audio Mixer state.
    pub audio_mixer: AudioMixer,
    /// Audio Analyzer for FFT-based spectrum analysis.
    pub audio_analyzer: AudioAnalyzer,
    /// Shared Asset Manager.
    pub assets: AssetManager,
    /// Video encoder mode (hardware acceleration).
    pub encoder_mode: EncoderMode,
    /// Render debug overlays (bounding boxes, ids, z-indices) on top of frames.
    pub debug_overlays: bool,
    /// Cached rasterized images for static subtrees.
    pub static_cache: std::collections::HashMap<NodeId, skia_safe::Image>,
}

/// SAFETY: Director is not thread-safe due to Skia internals (RefCounts).
/// However, we wrap it in a Mutex for Rhai, and we strictly ensure in the App
/// that it remains on a single thread (Axis Actor pattern).
/// This unsafe impl allows us to put it in Arc<Mutex<>>.
unsafe impl Send for Director {}

/// Bundled fonts registered with the Skia font collection at startup.
///
/// Each entry is `(filename, [family aliases], required)`. Registering multiple
/// weights (Regular/Bold/Italic) under the same family alias lets
/// `font_weight`/`font_style` resolve deterministically instead of falling back
/// to host system fonts (which breaks cross-machine reproducibility).
///
/// Files live in `crates/director-core/assets/fonts/` (force-committed via
/// `.gitignore`) so they are present regardless of the working directory.
/// `Roboto-Regular` is required; the rest are optional and only warn if missing
/// (CI/dev setup downloads them).
const BUNDLED_FONTS: &[(&str, &[&str], bool)] = &[
    ("Roboto-Regular.ttf", &["Roboto", "Sans Serif", "Arial"], true),
    ("Roboto-Bold.ttf", &["Roboto", "Sans Serif", "Arial"], false),
    ("Roboto-Italic.ttf", &["Roboto", "Sans Serif", "Arial"], false),
    ("Inter-Regular.ttf", &["Inter"], false),
    ("Inter-Bold.ttf", &["Inter"], false),
    ("Poppins-Bold.ttf", &["Poppins"], false),
];

impl Director {
    /// Computes the effective analysis time for an audio track at a given global time.
    /// Returns `None` if the track hasn't started, has ended, or has no samples.
    fn audio_analysis_time(track: &AudioTrack, global_time: f64, sample_rate: u32) -> Option<f64> {
        let relative_time = global_time - track.start_time;
        if relative_time < 0.0 {
            return None;
        }
        if let Some(max_duration) = track.duration {
            if relative_time >= max_duration {
                return None;
            }
        }
        let frame_count = track.samples.len() / 2;
        if frame_count == 0 {
            return None;
        }
        let source_duration = frame_count as f64 / sample_rate as f64;
        if source_duration <= 0.0 {
            return None;
        }
        Some(if track.loop_audio {
            relative_time % source_duration
        } else {
            relative_time
        })
    }

    fn audio_binding_energy(
        analyzer: &AudioAnalyzer,
        track: &AudioTrack,
        global_time: f64,
        band: &str,
    ) -> f32 {
        let Some(analysis_time) = Self::audio_analysis_time(track, global_time, analyzer.sample_rate) else {
            return 0.0;
        };
        analyzer.get_energy(&track.samples, analysis_time, band)
    }

    /// Creates a new Director instance.
    ///
    /// # Arguments
    /// * `width` - Output width in pixels.
    /// * `height` - Output height in pixels.
    /// * `fps` - Frame rate.
    /// * `asset_loader` - Implementation of asset loading strategy.
    /// * `render_mode` - Mode hint (e.g. Preview vs Export).
    /// * `context` - Optional existing context (for nested compositions).
    pub fn new(
        width: i32,
        height: i32,
        fps: f64,
        asset_loader: Arc<dyn AssetLoader>,
        render_mode: RenderMode,
        context: Option<DirectorContext>,
    ) -> Self {
        let assets = if let Some(ctx) = context {
            ctx.assets
        } else {
            let mut font_collection = FontCollection::new();
            let mut font_provider = TypefaceFontProvider::new();

            // Load system fonts fallback
            font_collection.set_default_font_manager(FontMgr::default(), None);

            // Register every bundled font under its family aliases. Multiple
            // weights of the same family (Regular/Bold/Italic) let Skia pick the
            // correct variant for `font_weight`/`font_style` deterministically.
            for (filename, aliases, required) in BUNDLED_FONTS {
                match asset_loader.load_bytes(&format!("fonts/{}", filename)) {
                    Ok(bytes) => {
                        let data = Data::new_copy(&bytes);
                        if let Some(typeface) = FontMgr::new().new_from_data(&data, 0) {
                            for alias in *aliases {
                                font_provider.register_typeface(typeface.clone(), Some(*alias));
                            }
                        } else {
                            tracing::error!(
                                "Failed to create typeface from bundled font '{}'",
                                filename
                            );
                        }
                    }
                    Err(_) => {
                        if *required {
                            tracing::error!(
                                "Required bundled font '{}' not found at assets/fonts/{}; \
                                 text will NOT render on systems without working system fonts. \
                                 Run `setup_assets.sh` or CI to fetch it before exporting.",
                                filename,
                                filename
                            );
                        } else {
                            tracing::warn!(
                                "Optional bundled font '{}' not found at assets/fonts/{}; \
                                 falling back to system fonts for that family.",
                                filename,
                                filename
                            );
                        }
                    }
                }
            }

            // Load fallback font (e.g. Emoji) if available
            if let Some(bytes) = asset_loader.load_font_fallback() {
                let data = Data::new_copy(&bytes);
                if let Some(typeface) = FontMgr::new().new_from_data(&data, 0) {
                    font_provider.register_typeface(typeface, Some("Fallback"));
                }
            }

            // Connect provider to collection
            font_collection.set_asset_font_manager(Some(font_provider.clone().into()));

            AssetManager::new(
                asset_loader,
                Arc::new(Mutex::new(font_collection)),
                Arc::new(Mutex::new(font_provider)),
                Arc::new(Mutex::new(HashMap::new())),
            )
        };

        let mut tracks = Vec::new();
        tracks.push(Track::new("Default"));
        Self {
            scene: SceneGraph::new(),
            timeline: Vec::new(),
            tracks,
            default_track_idx: 0,
            transitions: Vec::new(),
            width,
            height,
            fps,
            samples_per_frame: 1, // Default to no motion blur
            shutter_angle: 180.0,
            render_mode,
            audio_mixer: AudioMixer::new(48000),
            audio_analyzer: AudioAnalyzer::new(2048, 48000),
            assets,
            encoder_mode: EncoderMode::Auto, // Default to auto-detect hardware
            debug_overlays: false,
            static_cache: HashMap::new(),
        }
    }

    /// Mixes audio for the current frame time by traversing the scene graph.
    ///
    /// This aggregates audio from both global tracks and active scene nodes (including nested compositions).
    pub fn mix_audio(&mut self, samples_needed: usize, time: f64) -> Vec<f32> {
        let mut output = self.audio_mixer.mix(samples_needed, time);

        // Collect active scene roots (tracks or legacy timeline)
        let active_roots = self.collect_active_roots(time);

        // Delegate scene graph audio mixing to audio module
        crate::audio::mix_scene_audio(
            &mut output,
            &self.scene.nodes,
            &active_roots,
            samples_needed,
            self.audio_mixer.sample_rate,
        );

        output
    }

    /// Adds a global audio track that plays independently of scenes.
    pub fn add_global_audio(&mut self, samples: Vec<f32>) -> usize {
        let track = AudioTrack {
            samples,
            volume: crate::animation::Animated::new(1.0),
            pan: crate::animation::Animated::new(0.5),
            effects: crate::audio_effects::AudioEffectChain::new(),
            effect_param_animations: Vec::new(),
            start_time: 0.0,
            duration: None,
            loop_audio: false,
        };
        self.audio_mixer.add_track(track)
    }

    /// Adds an audio track synchronized to a specific scene's start time.
    pub fn add_scene_audio(&mut self, samples: Vec<f32>, start_time: f64, duration: f64) -> usize {
        let track = AudioTrack {
            samples,
            volume: crate::animation::Animated::new(1.0),
            pan: crate::animation::Animated::new(0.5),
            effects: crate::audio_effects::AudioEffectChain::new(),
            effect_param_animations: Vec::new(),
            start_time,
            duration: Some(duration),
            loop_audio: false,
        };
        self.audio_mixer.add_track(track)
    }

    pub fn collect_active_roots<'a>(&'a self, global_time: f64) -> Vec<(NodeId, f64)> {
        let mut roots = Vec::new();
        // Try new track system first
        if self.tracks.iter().any(|t| !t.clips.is_empty()) {
            for track in &self.tracks {
                for clip in &track.clips {
                    if track.enabled
                        && global_time >= clip.start_time
                        && global_time < clip.start_time + clip.duration
                    {
                        let local_time = clip.local_time(global_time);
                        roots.push((clip.scene_root, local_time));
                    }
                }
            }
        } else {
            // Legacy timeline fallback
            for item in &self.timeline {
                if global_time >= item.start_time
                    && global_time < item.start_time + item.duration
                {
                    // Freeze takes precedence over wall-clock advancement.
                    let local_time = item.freeze_time.unwrap_or_else(|| {
                        global_time - item.start_time
                    });
                    roots.push((item.scene_root, local_time));
                }
            }
        }
        roots
    }

    /// Convert a frame index to a wall-clock time in seconds using the current
    /// `fps`. Frame `n` corresponds to time `n / fps`.
    #[inline]
    pub fn frame_to_time(&self, frame: f64) -> f64 {
        if self.fps <= 0.0 {
            return 0.0;
        }
        frame / self.fps
    }

    /// Convert a wall-clock time in seconds to a fractional frame index using
    /// the current `fps`. Inverse of [`Director::frame_to_time`].
    #[inline]
    pub fn time_to_frame(&self, time: f64) -> f64 {
        if self.fps <= 0.0 {
            return 0.0;
        }
        time * self.fps
    }

    /// Updates the state of all active nodes for the given global time.
    ///
    /// This method calculates local time for each node, updates animations (transform, path),
    /// and calls `update()` on the underlying Elements.
    #[instrument(level = "debug", skip(self), fields(time = global_time))]
    pub fn update(&mut self, global_time: f64) {
        // Pass 1: Mark active nodes and set local time
        let active_roots = self.collect_active_roots(global_time);

        let mut stack = Vec::new();
        for (root, t) in active_roots {
            stack.push((root, t));
        }

        while let Some((id, time)) = stack.pop() {
            if id >= self.scene.nodes.len() {
                continue;
            }
            if self.scene.nodes[id].is_none() {
                continue;
            }

            let Some(node) = self.scene.nodes[id].as_mut() else {
                continue;
            };

            node.local_time = time;
            node.last_visit_time = global_time;

            let children = node.children.clone();
            for child in children {
                stack.push((child, time));
            }

            // Also traverse mask node to ensure it gets updates
            if let Some(mask_id) = node.mask_node {
                stack.push((mask_id, time));
            }
        }

        // Pass 2: Parallel node update via Rayon
        self.scene.nodes.par_iter_mut().enumerate().for_each(|(node_id, node_opt)| {
            if let Some(node) = node_opt {
                if (node.last_visit_time - global_time).abs() < 0.0001 {
                    if node.element.update(node.local_time) {
                        node.dirty_style = true;
                    }

                    // Update Transform Animations
                    node.transform.scale_x.update(node.local_time);
                    node.transform.scale_y.update(node.local_time);
                    node.transform.rotation.update(node.local_time);
                    node.transform.skew_x.update(node.local_time);
                    node.transform.skew_y.update(node.local_time);
                    node.transform.translate_x.update(node.local_time);
                    node.transform.translate_y.update(node.local_time);

                    // Update Path Animation (using cached PathMeasure)
                    if let Some(path_anim) = &mut node.path_animation {
                        let effective_time = node.local_time * path_anim.speed as f64;
                        let loop_dur = path_anim.duration;
                        let wrapped = if loop_dur > 0.0 && effective_time >= loop_dur {
                            let elapsed = (effective_time / loop_dur) as i32;
                            if path_anim.loop_count < 0 || elapsed < path_anim.loop_count {
                                effective_time % loop_dur
                            } else {
                                loop_dur
                            }
                        } else {
                            effective_time.min(loop_dur)
                        };
                        path_anim.progress.update(wrapped);
                        let length = path_anim.length();
                        let dist = path_anim.progress.current_value * length;
                        if let Some((p, angle)) = path_anim.sample(dist) {
                            node.transform.translate_x.current_value = p.x;
                            node.transform.translate_y.current_value = p.y;
                            // Apply tangent rotation if enabled
                            if path_anim.follow_tangent {
                                node.transform.rotation.current_value = angle.to_degrees();
                            }
                        }
                    }

                    // Update Path Morph (interpolate between two SVG paths)
                    // progress controls both position along the path AND the morph blend.
                    // At 0% -> start of from_path. At 100% -> end of to_path.
                    if let Some(morph) = &mut node.path_morph {
                        let effective_time = node.local_time * morph.speed as f64;
                        let loop_dur = morph.duration;
                        let wrapped = if loop_dur > 0.0 && effective_time >= loop_dur {
                            let elapsed = (effective_time / loop_dur) as i32;
                            if morph.loop_count < 0 || elapsed < morph.loop_count {
                                effective_time % loop_dur
                            } else {
                                loop_dur
                            }
                        } else {
                            effective_time.min(loop_dur)
                        };
                        morph.progress.update(wrapped);
                        let t = morph.progress.current_value.clamp(0.0, 1.0);
                        if let Some((p, angle)) = morph.sample_morph(t, t) {
                            node.transform.translate_x.current_value = p.x;
                            node.transform.translate_y.current_value = p.y;
                            if morph.follow_tangent {
                                node.transform.rotation.current_value = angle.to_degrees();
                            }
                        }
                    }

                    if node.transform.translate_x.current_value > 0.0 {
                        tracing::debug!(
                            "Node {} updated: x={}, local_time={}",
                            node_id,
                            node.transform.translate_x.current_value,
                            node.local_time
                        );
                    }
                }
            }
        });

        // Pass 3: Audio Reactive Bindings
        // Process after animations so audio values take priority
        let analyzer = self.audio_analyzer.clone();
        for node_opt in self.scene.nodes.iter_mut() {
            if let Some(node) = node_opt {
                if (node.last_visit_time - global_time).abs() < 0.0001 {
                    for binding in &mut node.audio_bindings {
                        // Get audio samples from the track
                        let energy = if let Some(Some(track)) =
                            self.audio_mixer.tracks.get(binding.track_id)
                        {
                            Self::audio_binding_energy(&analyzer, track, global_time, &binding.band)
                        } else {
                            0.0
                        };

                        // Map energy (0-1) to output range
                        let raw_value =
                            binding.min_value + energy * (binding.max_value - binding.min_value);

                        // Apply temporal smoothing
                        let smoothed = if binding.smoothing > 0.0 {
                            binding.prev_value * binding.smoothing
                                + raw_value * (1.0 - binding.smoothing)
                        } else {
                            raw_value
                        };
                        binding.prev_value = smoothed;

                        // Apply to property
                        match binding.property.as_str() {
                            "scale" => {
                                node.transform.scale_x.current_value = smoothed;
                                node.transform.scale_y.current_value = smoothed;
                            }
                            "scale_x" => node.transform.scale_x.current_value = smoothed,
                            "scale_y" => node.transform.scale_y.current_value = smoothed,
                            "x" => node.transform.translate_x.current_value = smoothed,
                            "y" => node.transform.translate_y.current_value = smoothed,
                            "rotation" => node.transform.rotation.current_value = smoothed,
                            "opacity" => node.element.set_opacity(smoothed),
                            _ => {}
                        }
                    }

                    // Spectrum visualizer: compute FFT and update bar heights
                    if let Some(sv) = node.element.as_any_mut().downcast_mut::<crate::node::SpectrumVisualizerNode>() {
                        match self.audio_mixer.tracks.get(sv.track_id) {
                            Some(Some(track)) => {
                                if let Some(analysis_time) = Self::audio_analysis_time(track, global_time, analyzer.sample_rate) {
                                    let spectrum = analyzer.compute_spectrum(&track.samples, analysis_time);
                                    if !spectrum.is_empty() && sv.bar_count > 0 {
                                        let bins_per_bar = (spectrum.len() / sv.bar_count).max(1);
                                        let effective_bars = sv.bar_count.min(spectrum.len());
                                        for i in 0..effective_bars {
                                            let bin_start = i * bins_per_bar;
                                            let bin_end = ((i + 1) * bins_per_bar).min(spectrum.len());
                                            let avg = if bin_end > bin_start {
                                                spectrum[bin_start..bin_end].iter().sum::<f32>() / (bin_end - bin_start) as f32
                                            } else {
                                                0.0
                                            };
                                            // Boost for visibility; typical music peaks around 0.3-0.5
                                            let target = (avg * 4.0).min(1.0);
                                            sv.smoothed_heights[i] = sv.smoothed_heights[i] * sv.smoothing
                                                + target * (1.0 - sv.smoothing);
                                            sv.bar_heights[i] = sv.smoothed_heights[i];
                                        }
                                    }
                                }
                            }
                            Some(None) | None => {
                                tracing::debug!(
                                    "SpectrumVisualizerNode references missing or empty audio track {}",
                                    sv.track_id
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Triggers `post_layout` on all active nodes.
    ///
    /// This is called after the Layout Engine has computed the final boxes, allowing elements
    /// to adjust their internal state (e.g., text resizing) based on the final layout.
    pub fn run_post_layout(&mut self, global_time: f64) {
        for node_opt in self.scene.nodes.iter_mut() {
            if let Some(node) = node_opt {
                if (node.last_visit_time - global_time).abs() < 0.0001 {
                    node.element.post_layout(node.layout_rect);
                }
            }
        }
    }
}

#[cfg(test)]
mod fps_tests {
    use super::*;

    #[test]
    fn frame_to_time_inverse_time_to_frame() {
        let mut d = Director::new(
            10,
            10,
            23.976,
            Arc::new(crate::DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );
        // NTSC-accurate 23.976 must round-trip exactly (no integer truncation).
        assert!((d.fps - 23.976).abs() < f64::EPSILON);
        for frame in [0.0, 0.5, 1.0, 23.976, 47.952, 72.0, 1000.0] {
            let t = d.frame_to_time(frame);
            let frame_back = d.time_to_frame(t);
            assert!(
                (frame_back - frame).abs() < 1e-9,
                "round-trip mismatch at {frame}: t={t}, back={frame_back}"
            );
        }
    }

    #[test]
    fn zero_fps_is_safe_and_returns_zero() {
        let mut d = Director::new(
            10,
            10,
            0.0,
            Arc::new(crate::DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );
        assert_eq!(d.frame_to_time(100.0), 0.0);
        assert_eq!(d.time_to_frame(5.0), 0.0);
    }
}
