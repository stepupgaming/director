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
use crate::compiler::{DependencyGraph, DependencyInput};
use crate::entity::{EntityId, EntityRegistry};
use crate::node::caption::{CaptionLine, CaptionNode};
use crate::node::smart_caption::{SmartCaptionConfig, SmartCaptionNode};
use crate::render_manifest::IssueSeverity;
use crate::semantic::{SemanticManifest, SolverConfig};
use crate::scene::SceneGraph;
use crate::style_policy::{StylePolicy, StylePolicyScorer, PolicyScore};
use crate::systems::layout::LayoutEngine;
use crate::types::BoundsRect;
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

/// Lightweight snapshot of node bounds at a specific time.
///
/// This is cheaper than cloning the whole [`Director`] and is intended for
/// verification passes (e.g. static-stretch detection) that only need to
/// compare per-node layout rectangles across time.
#[derive(Debug, Clone)]
pub struct LayoutSnapshot {
    pub width: i32,
    pub height: i32,
    pub sample_time: f64,
    pub nodes: Vec<LayoutNodeSnapshot>,
}

/// A single node's bounds inside a [`LayoutSnapshot`].
#[derive(Debug, Clone)]
pub struct LayoutNodeSnapshot {
    pub id: usize,
    pub kind: &'static str,
    pub bounds: BoundsRect,
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
    /// Frames Per Second.
    pub fps: u32,
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
    /// Persistent semantic entity registry (faces, speakers, objects, caption phrases).
    pub entities: EntityRegistry,
    /// Compile-phase dependency graph built by `compile()`.
    pub compiler: DependencyGraph,
    /// Nodes marked dirty by the last `invalidate()` call.
    pub dirty_nodes: std::collections::HashSet<NodeId>,
    /// Production style policy used by the compiler to score the scene.
    pub style_policy: StylePolicy,
    /// Most recent style-policy score produced by `compile()`, including any
    /// violations that were emitted as warnings/errors.
    pub last_policy_score: Option<PolicyScore>,
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
        fps: u32,
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
            entities: EntityRegistry::new(),
            compiler: DependencyGraph::new(),
            dirty_nodes: std::collections::HashSet::new(),
            style_policy: StylePolicy::default(),
            last_policy_score: None,
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

    /// Create a `SmartCaptionNode` from caption lines and a semantic manifest,
    /// register it in the scene graph, and wire it to the entity registry.
    ///
    /// The node's `entity_refs` are set from the manifest so that compile-time
    /// dependency tracking and invalidation know which entities affect this
    /// caption.
    ///
    /// # Note
    /// This helper only registers the node in the scene graph. The caller is
    /// responsible for wiring the returned `NodeId` into a timeline item or
    /// track clip so the caption is rendered during export.
    ///
    /// Returns the `NodeId` of the registered smart caption node.
    pub fn add_smart_caption(
        &mut self,
        lines: Vec<CaptionLine>,
        manifest: SemanticManifest,
        config: SmartCaptionConfig,
    ) -> NodeId {
        self.add_smart_caption_with(lines, manifest, config, SmartCaptionNode::from_manifest)
    }

    /// Create a `SmartCaptionNode` using the continuous temporal constraint
    /// solver, register it in the scene graph, and wire it to the entity registry.
    ///
    /// This is the same as [`Director::add_smart_caption`] but uses the
    /// continuous solver, which produces absolute `(x, y)` trajectories instead
    /// of discrete anchors.
    ///
    /// # Note
    /// This helper only registers the node in the scene graph. The caller is
    /// responsible for wiring the returned `NodeId` into a timeline item or
    /// track clip so the caption is rendered during export.
    pub fn add_smart_caption_continuous(
        &mut self,
        lines: Vec<CaptionLine>,
        manifest: SemanticManifest,
        config: SmartCaptionConfig,
    ) -> NodeId {
        self.add_smart_caption_with(lines, manifest, config, SmartCaptionNode::from_manifest_continuous)
    }

    /// Shared implementation for creating and registering a `SmartCaptionNode`.
    fn add_smart_caption_with(
        &mut self,
        lines: Vec<CaptionLine>,
        manifest: SemanticManifest,
        config: SmartCaptionConfig,
        build: fn(CaptionNode, &EntityRegistry, &SemanticManifest, SmartCaptionConfig) -> SmartCaptionNode,
    ) -> NodeId {
        let entity_refs = manifest.entity_refs.clone();
        let font_collection = self.assets.font_collection.clone();
        let inner = CaptionNode::new(lines, font_collection);
        let smart = build(inner, &self.entities, &manifest, config);
        let id = self.scene.add_node(Box::new(smart));
        if let Some(node) = self.scene.get_node_mut(id) {
            node.entity_refs = entity_refs;
        }
        id
    }

    /// Convenience overload: build a `SemanticManifest` from entity refs and
    /// shot boundaries, then create and register a `SmartCaptionNode`.
    ///
    /// This is the same as [`Director::add_smart_caption`] but builds the
    /// manifest for you from the supplied shot range and entity references.
    pub fn add_smart_caption_for_shot(
        &mut self,
        lines: Vec<CaptionLine>,
        entity_refs: Vec<EntityId>,
        start_time: f64,
        end_time: f64,
        config: SmartCaptionConfig,
    ) -> NodeId {
        let manifest = SemanticManifest {
            entity_refs,
            start_time,
            end_time,
            solver: SolverConfig::default(),
        };
        self.add_smart_caption(lines, manifest, config)
    }

    /// Convenience overload for the continuous solver.
    ///
    /// This is the same as [`Director::add_smart_caption_for_shot`] but uses the
    /// continuous solver.
    pub fn add_smart_caption_for_shot_continuous(
        &mut self,
        lines: Vec<CaptionLine>,
        entity_refs: Vec<EntityId>,
        start_time: f64,
        end_time: f64,
        config: SmartCaptionConfig,
    ) -> NodeId {
        let manifest = SemanticManifest {
            entity_refs,
            start_time,
            end_time,
            solver: SolverConfig::default(),
        };
        self.add_smart_caption_continuous(lines, manifest, config)
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

    fn collect_active_roots<'a>(&'a self, global_time: f64) -> Vec<(NodeId, f64)> {
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
                    let local_time = global_time - item.start_time;
                    roots.push((item.scene_root, local_time));
                }
            }
        }
        roots
    }

    /// Updates the state of active nodes for the given global time.
    ///
    /// When `dirty` is `None`, every active node is updated as before. When it
    /// is `Some`, only the nodes in the set are updated, allowing
    /// incremental re-rendering after `invalidate()`. The caller is responsible
    /// for expanding the dirty set to include any ancestors or descendants
    /// required for correct rendering (see [`Director::expand_dirty_set`]).
    #[instrument(level = "debug", skip(self), fields(time = global_time))]
    pub fn update(&mut self, global_time: f64, dirty: Option<&std::collections::HashSet<NodeId>>) {
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
                if (node.last_visit_time - global_time).abs() >= 0.0001 {
                    return;
                }
                // Skip nodes that are not in the dirty set during incremental updates.
                if let Some(ref dirty) = dirty {
                    if !dirty.contains(&node_id) {
                        return;
                    }
                }
                if node.element.update(node.local_time) {
                    node.dirty_style = true;
                }
                node.update_generation = node.update_generation.wrapping_add(1);

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
        });

        // Pass 3: Audio Reactive Bindings
        // Process after animations so audio values take priority
        let analyzer = self.audio_analyzer.clone();
        for (node_id, node_opt) in self.scene.nodes.iter_mut().enumerate() {
            if let Some(node) = node_opt {
                if (node.last_visit_time - global_time).abs() < 0.0001 {
                    // During incremental updates, skip audio bindings for nodes
                    // that are not dirty.
                    if let Some(ref dirty) = dirty {
                        if !dirty.contains(&node_id) {
                            continue;
                        }
                    }
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

    /// Expand a dirty set to include all ancestors and descendants of dirty
    /// nodes.
    ///
    /// Ancestors are included because a parent's transform/layout context is
    /// required to render a dirty child correctly. Descendants are included
    /// because a change to a parent (e.g., a transform or layout change) may
    /// affect its children. Mask nodes are treated as descendants for the
    /// same reason.
    pub(crate) fn expand_dirty_set(&self, dirty: &std::collections::HashSet<NodeId>) -> std::collections::HashSet<NodeId> {
        let mut expanded = dirty.clone();

        // Add ancestors.
        for &node_id in dirty {
            let mut current = node_id;
            while let Some(node) = self.scene.get_node(current) {
                if let Some(parent_id) = node.parent {
                    // If the parent was already in the set, it was processed by
                    // an earlier chain that walked up to the root, so this
                    // branch is fully covered.
                    if !expanded.insert(parent_id) {
                        break;
                    }
                    current = parent_id;
                } else {
                    break;
                }
            }
        }

        // Add descendants (children and mask nodes) recursively.
        let mut stack: Vec<NodeId> = dirty.iter().copied().collect();
        while let Some(node_id) = stack.pop() {
            if let Some(node) = self.scene.get_node(node_id) {
                for &child_id in &node.children {
                    if expanded.insert(child_id) {
                        stack.push(child_id);
                    }
                }
                if let Some(mask_id) = node.mask_node {
                    if expanded.insert(mask_id) {
                        stack.push(mask_id);
                    }
                }
            }
        }

        expanded
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

    /// Build the dependency graph for the current scene graph and run the
    /// configured `StylePolicy` scorer.
    ///
    /// This records dependencies between nodes and their inputs (entities,
    /// audio tracks, hierarchy, masks) so that future incremental passes can
    /// invalidate only the affected nodes. The graph is stored in
    /// `self.compiler` and the dirty node set is reset. Each node's
    /// `last_input_hash` is updated to reflect its current structural inputs.
    ///
    /// After the graph is built, the scene is scored against `style_policy`.
    /// Any violations are emitted as `tracing` logs (errors/warnings/info)
    /// and stored in `self.last_policy_score` so callers can inspect them before
    /// export.
    pub fn compile(&mut self) {
        self.compiler.clear();
        self.dirty_nodes.clear();

        for (node_id, node_opt) in self.scene.nodes.iter_mut().enumerate() {
            let Some(node) = node_opt else { continue };

            // Entity references.
            for &entity_id in &node.entity_refs {
                self.compiler.record(node_id, DependencyInput::Entity(entity_id));
            }

            // Audio-reactive bindings.
            for binding in &node.audio_bindings {
                self.compiler.record(node_id, DependencyInput::AudioTrack(binding.track_id));
            }

            // Hierarchy: children depend on their parent for transform/layout.
            // We record only the child→parent edge so that invalidating a child
            // does not pull in its siblings. This means invalidating a parent
            // will not currently invalidate its children; add a descendants
            // expansion when that direction is needed.
            if let Some(parent_id) = node.parent {
                self.compiler.record(node_id, DependencyInput::Node(parent_id));
            }

            // Mask references.
            if let Some(mask_id) = node.mask_node {
                self.compiler.record(node_id, DependencyInput::Node(mask_id));
            }

            // Snapshot the current structural input hash for cache invalidation.
            let hash = node.compute_input_hash();
            node.set_input_hash(Some(hash));
        }

        // Run the style policy scorer over the timeline so violations are
        // emitted as warnings/errors before export.
        self.last_policy_score = Some(self.score_style_policy_for_compile());
    }

    /// Score the scene against `style_policy` over the full timeline duration.
    ///
    /// Used internally by `compile()` to surface production-rule violations
    /// before export. Returns the computed `PolicyScore`.
    fn score_style_policy_for_compile(&self) -> PolicyScore {
        let duration_minutes = crate::style_policy::total_duration_minutes(self);
        let duration = duration_minutes * 60.0;
        let time_range = if duration > 0.0 { (0.0, duration) } else { (0.0, 1.0) };
        let score = self.score_style_policy(time_range, 10);

        for violation in &score.violations {
            let message = format!(
                "StylePolicy violation [{}]: {}{}",
                violation.rule,
                violation.description,
                match violation.node_id {
                    Some(id) => format!(" (node {})", id),
                    None => String::new(),
                }
            );
            match violation.severity {
                IssueSeverity::Error => tracing::error!("{}", message),
                IssueSeverity::Warning => tracing::warn!("{}", message),
                IssueSeverity::Info => tracing::info!("{}", message),
            }
        }

        score
    }

    /// Invalidate nodes affected by the given inputs.
    ///
    /// Returns the set of affected node IDs and stores it in
    /// `self.dirty_nodes`. Only nodes whose current structural input hash
    /// differs from the hash recorded at the last `compile()` are marked dirty,
    /// so unchanged nodes are not recomputed.
    pub fn invalidate(&mut self, inputs: Vec<DependencyInput>) -> std::collections::HashSet<NodeId> {
        let potentially_affected = self.compiler.affected_nodes(&inputs);
        let mut dirty: std::collections::HashSet<NodeId> = std::collections::HashSet::new();

        for node_id in &potentially_affected {
            if let Some(Some(node)) = self.scene.nodes.get_mut(*node_id) {
                let current_hash = node.compute_input_hash();
                match node.last_input_hash() {
                    None => {
                        // Never compiled; treat as dirty and snapshot the hash
                        // so future invalidations have a baseline.
                        node.set_input_hash(Some(current_hash));
                        dirty.insert(*node_id);
                    }
                    Some(stored_hash) if stored_hash != current_hash => {
                        dirty.insert(*node_id);
                    }
                    _ => {}
                }
            }
        }

        self.dirty_nodes = dirty.clone();
        dirty
    }

    /// Score the current scene against the configured `StylePolicy`.
    ///
    /// The scorer samples the scene over `time_range` using `samples` points.
    /// Results include per-dimension sub-scores and a list of violations.
    pub fn score_style_policy(
        &self,
        time_range: (f64, f64),
        samples: usize,
    ) -> PolicyScore {
        StylePolicyScorer::new().score(self, &self.style_policy, time_range, samples)
    }

    /// Capture a lightweight layout snapshot at `time` without cloning the
    /// whole Director.
    ///
    /// The snapshot contains only the per-node bounds and kind strings needed
    /// by verification passes such as static-stretch detection. It runs the
    /// update and layout passes on `self`, so it mutates the Director's
    /// internal state (local times, layout rectangles, etc.).
    pub fn layout_snapshot(&mut self, time: f64) -> LayoutSnapshot {
        self.update(time, None);

        let mut layout_engine = LayoutEngine::new();
        layout_engine.compute_layout(&mut self.scene, self.width, self.height, time);

        let nodes = self
            .scene
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(id, node_opt)| {
                let node = node_opt.as_ref()?;
                let r = node.layout_rect;
                Some(LayoutNodeSnapshot {
                    id,
                    kind: crate::element::element_kind(node.element.as_ref()),
                    bounds: BoundsRect::new(r.left(), r.top(), r.width(), r.height()),
                })
            })
            .collect();

        LayoutSnapshot {
            width: self.width,
            height: self.height,
            sample_time: time,
            nodes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::BoxNode;
    use crate::video_wrapper::RenderMode;
    use crate::{DefaultAssetLoader, Director};
    use std::sync::Arc;

    #[test]
    fn add_smart_caption_creates_node_and_wires_entities() {
        use crate::entity::{BoundingBox, EntityKind};
        use crate::node::caption::{CaptionLine, CaptionWord};
        use crate::node::smart_caption::SmartCaptionConfig;

        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        let face = director.entities.create(EntityKind::Face, "mara".to_string());
        director.entities.add_sample(
            face,
            0.0,
            BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
            1.0,
        );
        director.entities.add_sample(
            face,
            2.0,
            BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
            1.0,
        );

        let line = CaptionLine {
            text: "hello".to_string(),
            words: vec![CaptionWord {
                text: "hello".to_string(),
                start_sec: 0.0,
                end_sec: 2.0,
                display_line_idx: 0,
            }],
            start_sec: 0.0,
            end_sec: 2.0,
        };

        let id = director.add_smart_caption_for_shot(
            vec![line],
            vec![face],
            0.0,
            2.0,
            SmartCaptionConfig::default(),
        );

        let node = director.scene.get_node(id).expect("smart caption node exists");
        assert_eq!(node.entity_refs.len(), 1);
        assert_eq!(node.entity_refs[0], face);
        assert!(
            node.element
                .as_any()
                .downcast_ref::<SmartCaptionNode>()
                .is_some(),
            "registered element should be a SmartCaptionNode"
        );

        director.update(0.5, None);
        let node = director.scene.get_node(id).unwrap();
        let smart = node
            .element
            .as_any()
            .downcast_ref::<SmartCaptionNode>()
            .expect("element is a SmartCaptionNode");
        assert!(!smart.trajectory.is_empty());
    }

    #[test]
    fn add_smart_caption_with_explicit_manifest_wires_entities() {
        use crate::entity::{BoundingBox, EntityKind};
        use crate::node::caption::{CaptionLine, CaptionWord};
        use crate::node::smart_caption::SmartCaptionConfig;
        use crate::semantic::{SemanticManifest, SolverConfig};

        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        let face = director.entities.create(EntityKind::Face, "mara".to_string());
        director.entities.add_sample(
            face,
            0.0,
            BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
            1.0,
        );

        let line = CaptionLine {
            text: "hello".to_string(),
            words: vec![CaptionWord {
                text: "hello".to_string(),
                start_sec: 0.0,
                end_sec: 2.0,
                display_line_idx: 0,
            }],
            start_sec: 0.0,
            end_sec: 2.0,
        };

        let manifest = SemanticManifest {
            entity_refs: vec![face],
            start_time: 0.0,
            end_time: 2.0,
            solver: SolverConfig::default(),
        };

        let id = director.add_smart_caption(vec![line], manifest, SmartCaptionConfig::default());

        let node = director.scene.get_node(id).expect("smart caption node exists");
        assert_eq!(node.entity_refs, vec![face]);
        assert!(node
            .element
            .as_any()
            .downcast_ref::<SmartCaptionNode>()
            .is_some());

        director.update(0.5, None);
        let node = director.scene.get_node(id).unwrap();
        let smart = node
            .element
            .as_any()
            .downcast_ref::<SmartCaptionNode>()
            .unwrap();
        assert!(!smart.trajectory.is_empty());
    }

    #[test]
    fn add_smart_caption_continuous_creates_node_and_wires_entities() {
        use crate::entity::{BoundingBox, EntityKind};
        use crate::node::caption::{CaptionLine, CaptionWord};
        use crate::node::smart_caption::SmartCaptionConfig;

        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        let face = director.entities.create(EntityKind::Face, "mara".to_string());
        director.entities.add_sample(
            face,
            0.0,
            BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
            1.0,
        );
        director.entities.add_sample(
            face,
            2.0,
            BoundingBox::new(400.0, 1500.0, 200.0, 200.0),
            1.0,
        );

        let line = CaptionLine {
            text: "hello".to_string(),
            words: vec![CaptionWord {
                text: "hello".to_string(),
                start_sec: 0.0,
                end_sec: 2.0,
                display_line_idx: 0,
            }],
            start_sec: 0.0,
            end_sec: 2.0,
        };

        let id = director.add_smart_caption_for_shot_continuous(
            vec![line],
            vec![face],
            0.0,
            2.0,
            SmartCaptionConfig::default(),
        );

        let node = director.scene.get_node(id).expect("smart caption node exists");
        assert_eq!(node.entity_refs.len(), 1);
        assert_eq!(node.entity_refs[0], face);
        assert!(
            node.element
                .as_any()
                .downcast_ref::<SmartCaptionNode>()
                .is_some(),
            "registered element should be a SmartCaptionNode"
        );

        director.update(0.5, None);
        let node = director.scene.get_node(id).unwrap();
        let smart = node
            .element
            .as_any()
            .downcast_ref::<SmartCaptionNode>()
            .expect("element is a SmartCaptionNode");
        assert!(smart.is_continuous());
        assert!(!smart.trajectory.is_empty());
    }

    #[test]
    fn layout_snapshot_captures_node_bounds() {
        let mut director = Director::new(
            1080,
            1920,
            30,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        let mut box_node = BoxNode::new();
        box_node.style.size.width = taffy::style::Dimension::length(1080.0);
        box_node.style.size.height = taffy::style::Dimension::length(1920.0);
        let root = director.scene.add_node(Box::new(box_node));
        director.timeline.push(TimelineItem {
            scene_root: root,
            name: Some("main".into()),
            start_time: 0.0,
            duration: 2.0,
            z_index: 0,
            audio_tracks: vec![],
        });

        let snapshot = director.layout_snapshot(0.5);

        assert_eq!(snapshot.width, 1080);
        assert_eq!(snapshot.height, 1920);
        assert_eq!(snapshot.sample_time, 0.5);
        assert_eq!(snapshot.nodes.len(), 1);
        assert_eq!(snapshot.nodes[0].id, root);
        assert_eq!(snapshot.nodes[0].kind, "box");
        assert!(snapshot.nodes[0].bounds.w > 0.0);
        assert!(snapshot.nodes[0].bounds.h > 0.0);
    }

}
