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

/// Flat view of a sequenced clip (for transitions, manifests, and tests).
///
/// Authoring and playback use [`Track`]/`TrackClip`] only. This DTO is derived
/// via [`Director::flat_timeline`] and is no longer a second stored timeline.
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

impl From<&crate::timeline::TrackClip> for TimelineItem {
    fn from(clip: &crate::timeline::TrackClip) -> Self {
        Self {
            scene_root: clip.scene_root,
            name: clip.name.clone(),
            start_time: clip.start_time,
            duration: clip.duration,
            z_index: clip.z_index,
            audio_tracks: clip.audio_tracks.clone(),
        }
    }
}

/// Inputs baked into a static-subtree snapshot.
///
/// `time` is `None` when the subtree's paint does not depend on time, so a
/// later frame may reuse the image. `Some` must match the node's local time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StaticCacheKey {
    pub width: f32,
    pub height: f32,
    pub translate_x: f32,
    pub translate_y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation: f32,
    pub skew_x: f32,
    pub skew_y: f32,
    pub pivot_x: f32,
    pub pivot_y: f32,
    pub parent_opacity: f32,
    pub time: Option<f64>,
}

/// Rasterized static subtree plus the key that was baked into it.
#[derive(Clone, Debug)]
pub struct StaticCacheEntry {
    pub image: skia_safe::Image,
    pub key: StaticCacheKey,
}

/// The central engine coordinator.
///
/// `Director` manages the Scene Graph (nodes), Timeline (sequencing),
/// and shared resources (Assets, Fonts, Audio).
#[derive(Clone)]
pub struct Director {
    /// The Scene Graph.
    pub scene: SceneGraph,
    /// Multi-track timeline — the only sequencing structure (plan 01).
    pub tracks: Vec<Track>,
    /// Index of the default track that `add_scene` pushes to.
    pub default_track_idx: usize,
    /// Active transitions.
    pub transitions: Vec<Transition>,
    /// Output width in pixels.
    pub width: i32,
    /// Output height in pixels.
    pub height: i32,
    /// Device scale for the current export raster. Layout stays at `width`.
    pub output_scale_x: f32,
    /// Device scale for the current export raster. Layout stays at `height`.
    pub output_scale_y: f32,
    /// Clear each frame to transparent instead of opaque black. Alpha export sets it.
    pub transparent_background: bool,
    /// Frames per second (`f64` so NTSC rates like 23.976 / 29.97 round-trip).
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
    /// Rasterized static subtrees, keyed by layout, baked transform, opacity, and time.
    pub static_cache: std::collections::HashMap<NodeId, StaticCacheEntry>,
    /// Persistent semantic entity registry (faces, speakers, objects, caption phrases).
    pub entities: EntityRegistry,
    /// Compile-phase dependency graph built by `compile()`.
    pub compiler: DependencyGraph,
    /// Nodes marked dirty by the last `invalidate()` call.
    pub dirty_nodes: std::collections::HashSet<NodeId>,
    /// Last known input hash for each node, recorded by `compile()` and used
    /// by `invalidate()` to detect structural changes. Owned by `Director` so
    /// only the compile/invalidation passes can mutate it.
    pub input_hashes: std::collections::HashMap<NodeId, u64>,
    /// Production style policy used by the compiler to score the scene.
    pub style_policy: StylePolicy,
    /// Most recent style-policy score produced by `compile()`, including any
    /// violations that were emitted as warnings/errors.
    pub last_policy_score: Option<PolicyScore>,
    /// Bundled fonts that failed to load at construction (Plan 06).
    /// Export must fail closed when this is non-empty.
    pub missing_required_fonts: Vec<String>,
}

/// SAFETY (P0 threading audit): `Director` is only `Send` so it can travel
/// inside `Arc<Mutex<Director>>` (`MovieHandle`) between the thread that
/// builds it and the thread that locks it. It is NOT safe for concurrent
/// `&`/`&mut` access. The enforced invariants are:
///
/// 1. **Mutex confinement.** Every cross-thread handoff goes through
///    `Arc<Mutex<Director>>`. The preview server (`director-cli/src/preview`)
///    holds that lock across the whole `render_png` call, so two HTTP
///    handlers never render concurrently.
/// 2. **Disjoint parallel update.** `Director::update` uses Rayon's
///    `par_iter_mut` over arena slots; each `SceneNode` is touched by exactly
///    one thread, and cross-node handles (`NodeId`) are only read there.
///    Shared element state (video frames, Lottie players/caches,
///    composition sub-directors) sits behind `Mutex`es.
/// 3. **Thread-confined decoders.** `SyncDecoder` owns a live `ffmpeg` child
///    + pipe reader and is only driven from its owner's thread; the worker
///      thread owns the `ThreadedDecoder` pipe exclusively.
///
/// Skia objects (`Image` in `static_cache`, surfaces borrowed per call) are
/// never shared across threads without the `Director` lock held. If a new
/// `Director` field is added that is `!Send` (e.g. raw GPU contexts, `Rc`,
/// `RefCell`), this impl must be re-audited — do not extend it blindly.
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
/// All entries are **required** for export/CI determinism (Plan 06).
    /// Preview mode logs errors but continues so local iteration is not blocked;
    /// [`Director::ensure_required_fonts`] fails closed for export.
    const BUNDLED_FONTS: &[(&str, &[&str], bool)] = &[
        ("Roboto-Regular.ttf", &["Roboto", "Sans Serif", "Arial"], true),
        ("Roboto-Bold.ttf", &["Roboto", "Sans Serif", "Arial"], true),
        ("Roboto-Italic.ttf", &["Roboto", "Sans Serif", "Arial"], true),
        ("Inter-Regular.ttf", &["Inter"], true),
        ("Inter-Bold.ttf", &["Inter"], true),
        ("Poppins-Bold.ttf", &["Poppins"], true),
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
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new(
        width: i32,
        height: i32,
        fps: f64,
        asset_loader: Arc<dyn AssetLoader>,
        render_mode: RenderMode,
        context: Option<DirectorContext>,
    ) -> Self {
        let fps = if fps.is_finite() && fps > 0.0 { fps } else { 30.0 };
        let mut missing_required_fonts = Vec::new();
        let assets = if let Some(ctx) = context {
            // Nested comps inherit parent fonts; parent already tracked missing.
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
                            if *required {
                                missing_required_fonts.push((*filename).to_string());
                            }
                        }
                    }
                    Err(_) => {
                        if *required {
                            missing_required_fonts.push((*filename).to_string());
                            tracing::error!(
                                "Required bundled font '{}' not found at assets/fonts/{}; \
                                 run `setup_assets.sh` or CI font step before exporting.",
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

        let tracks = vec![Track::new("Default")];
        Self {
            scene: SceneGraph::new(),
            tracks,
            default_track_idx: 0,
            transitions: Vec::new(),
            width,
            height,
            output_scale_x: 1.0,
            output_scale_y: 1.0,
            transparent_background: false,
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
            input_hashes: std::collections::HashMap::new(),
            style_policy: StylePolicy::default(),
            last_policy_score: None,
            missing_required_fonts,
        }
    }

    /// Filenames of the bundled font set required for deterministic export (Plan 06).
    pub fn required_bundled_font_files() -> &'static [&'static str] {
        &[
            "Roboto-Regular.ttf",
            "Roboto-Bold.ttf",
            "Roboto-Italic.ttf",
            "Inter-Regular.ttf",
            "Inter-Bold.ttf",
            "Poppins-Bold.ttf",
        ]
    }

    /// Fail closed when required bundled fonts are missing.
    ///
    /// Called at the start of export. Preview construction still succeeds so
    /// local iteration is not blocked, but export refuses non-reproducible runs.
    pub fn ensure_required_fonts(&self) -> Result<(), crate::RenderError> {
        if self.missing_required_fonts.is_empty() {
            return Ok(());
        }
        Err(crate::RenderError::MissingRequiredFonts(
            self.missing_required_fonts.clone(),
        ))
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
            &self.scene,
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

    fn collect_active_roots(&self, global_time: f64) -> Vec<(NodeId, f64)> {
        let mut roots = Vec::new();
        for track in &self.tracks {
            if !track.enabled {
                continue;
            }
            for clip in &track.clips {
                if clip.is_active_at(global_time) {
                    roots.push((clip.scene_root, clip.local_time(global_time)));
                }
            }
        }
        roots
    }

    /// Flatten all track clips into a `TimelineItem` list (stable order: track
    /// order, then clip order within each track). Used by transitions, manifests,
    /// and tests that need a linear scene index.
    pub fn flat_timeline(&self) -> Vec<TimelineItem> {
        let mut items = Vec::new();
        for track in &self.tracks {
            for clip in &track.clips {
                items.push(TimelineItem::from(clip));
            }
        }
        items
    }

    /// Maximum end time across all clips (seconds). Returns 0.0 if empty.
    pub fn timeline_duration(&self) -> f64 {
        let mut max_end = 0.0f64;
        for track in &self.tracks {
            for clip in &track.clips {
                max_end = max_end.max(clip.end_time());
            }
        }
        max_end
    }

    /// Ensure `default_track_idx` is valid and return it.
    pub fn ensure_default_track(&mut self) -> usize {
        if self.tracks.is_empty() {
            self.tracks.push(Track::new("Default"));
            self.default_track_idx = 0;
        } else if self.default_track_idx >= self.tracks.len() {
            self.default_track_idx = 0;
        }
        self.default_track_idx
    }

    /// Push a clip onto a track (default track if `track_idx` is `None`).
    pub fn push_clip(&mut self, track_idx: Option<usize>, clip: crate::timeline::TrackClip) {
        let idx = track_idx.unwrap_or_else(|| self.ensure_default_track());
        if idx >= self.tracks.len() {
            let default = self.ensure_default_track();
            self.tracks[default].clips.push(clip);
        } else {
            self.tracks[idx].clips.push(clip);
        }
    }

    /// Test/helper: place a root on the default track covering `[0, duration)`.
    pub fn push_scene_root(&mut self, scene_root: NodeId, duration: f64) {
        self.push_scene_root_at(scene_root, 0.0, duration, None);
    }

    /// Test/helper: place a root on a track at an explicit start time.
    pub fn push_scene_root_at(
        &mut self,
        scene_root: NodeId,
        start_time: f64,
        duration: f64,
        name: Option<String>,
    ) {
        let mut clip = crate::timeline::TrackClip::new(scene_root, start_time, duration);
        clip.name = name;
        self.push_clip(None, clip);
    }

    /// Frame index → time (seconds). See [`crate::timeline::frame_to_time`].
    pub fn frame_to_time(&self, frame: u64) -> f64 {
        crate::timeline::frame_to_time(frame, self.fps)
    }

    /// Time (seconds) → frame index. See [`crate::timeline::time_to_frame`].
    pub fn time_to_frame(&self, time: f64) -> u64 {
        crate::timeline::time_to_frame(time, self.fps)
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
            let Some(node) = self.scene.get_node_mut(id) else {
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
        let generations = self.scene.generations_snapshot();
        self.scene.nodes.par_iter_mut().enumerate().for_each(|(index, node_opt)| {
            if let Some(node) = node_opt {
                let node_id = NodeId::new(index as u32, generations[index]);
                if (node.last_visit_time - global_time).abs() >= 0.0001 {
                    return;
                }
                // Skip nodes that are not in the dirty set during incremental updates.
                if let Some(dirty) = dirty {
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
                    let wrapped = crate::types::wrap_looped_time(
                        effective_time,
                        path_anim.duration,
                        path_anim.loop_count,
                    );
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
                    let wrapped = crate::types::wrap_looped_time(
                        effective_time,
                        morph.duration,
                        morph.loop_count,
                    );
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
        for (node_id, node) in self.scene.iter_alive_mut() {
            if (node.last_visit_time - global_time).abs() >= 0.0001 {
                continue;
            }
            // During incremental updates, skip audio bindings for nodes
            // that are not dirty.
            if let Some(dirty) = dirty {
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
            if let Some(sv) = node
                .element
                .as_any_mut()
                .downcast_mut::<crate::node::SpectrumVisualizerNode>()
            {
                match self.audio_mixer.tracks.get(sv.track_id) {
                    Some(Some(track)) => {
                        if let Some(analysis_time) =
                            Self::audio_analysis_time(track, global_time, analyzer.sample_rate)
                        {
                            let spectrum =
                                analyzer.compute_spectrum(&track.samples, analysis_time);
                            if !spectrum.is_empty() && sv.bar_count > 0 {
                                let bins_per_bar = (spectrum.len() / sv.bar_count).max(1);
                                let effective_bars = sv.bar_count.min(spectrum.len());
                                for i in 0..effective_bars {
                                    let bin_start = i * bins_per_bar;
                                    let bin_end =
                                        ((i + 1) * bins_per_bar).min(spectrum.len());
                                    let avg = if bin_end > bin_start {
                                        spectrum[bin_start..bin_end].iter().sum::<f32>()
                                            / (bin_end - bin_start) as f32
                                    } else {
                                        0.0
                                    };
                                    // Boost for visibility; typical music peaks around 0.3-0.5
                                    let target = (avg * 4.0).min(1.0);
                                    sv.smoothed_heights[i] = sv.smoothed_heights[i]
                                        * sv.smoothing
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

    /// Add a child to a parent node and invalidate the parent's children
    /// dependency so that hierarchy changes automatically mark the parent as
    /// potentially affected.
    ///
    /// This is a convenience wrapper around [`SceneGraph::try_add_child`] that
    /// also emits a [`DependencyInput::Children`] invalidation for the parent.
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) -> bool {
        let ok = self.scene.try_add_child(parent, child);
        if ok {
            self.invalidate(vec![DependencyInput::Children(parent)]);
        }
        ok
    }

    /// Remove a child from a parent node and invalidate the parent's children
    /// dependency so that hierarchy changes automatically mark the parent as
    /// potentially affected.
    ///
    /// This is a convenience wrapper around [`SceneGraph::remove_child`] that
    /// also emits a [`DependencyInput::Children`] invalidation for the parent.
    /// Invalidation only happens if the child was actually attached to the
    /// parent, so no-op removals do not dirty the parent.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        let was_attached = self
            .scene
            .get_node(child)
            .map(|n| n.parent == Some(parent))
            .unwrap_or(false);
        self.scene.remove_child(parent, child);
        if was_attached {
            self.invalidate(vec![DependencyInput::Children(parent)]);
        }
    }

    /// Expand a dirty set to include all ancestors and descendants of dirty
    /// nodes, plus reverse mask users.
    ///
    /// Ancestors are included because a parent's transform/layout context is
    /// required to render a dirty child correctly. Descendants are included
    /// because a change to a parent (e.g., a transform or layout change) may
    /// affect its children. Mask nodes referenced by any included node (and
    /// their subtrees) are included. Nodes that **use** a dirty node as their
    /// mask are also included so mask edits recompose masked content.
    pub fn expand_dirty_set(&self, dirty: &std::collections::HashSet<NodeId>) -> std::collections::HashSet<NodeId> {
        let mut expanded = dirty.clone();

        // Add ancestors (transform/layout context). Do **not** treat ancestors as
        // seeds for descendant expansion — that would dirty all siblings.
        for &node_id in dirty {
            let mut current = node_id;
            while let Some(node) = self.scene.get_node(current) {
                if let Some(parent_id) = node.parent {
                    if !expanded.insert(parent_id) {
                        break;
                    }
                    current = parent_id;
                } else {
                    break;
                }
            }
        }

        // Seeds for descendant expansion: original dirty nodes plus content that
        // uses a dirty node as its mask. Ancestors are NOT seeds — otherwise
        // every sibling of a dirty child would be re-evaluated.
        let mut stack: Vec<NodeId> = dirty.iter().copied().collect();
        for (id, node) in self.scene.iter_alive() {
            if let Some(mask_id) = node.mask_node {
                if dirty.contains(&mask_id)
                    && expanded.insert(id) {
                        stack.push(id);
                    }
            }
        }

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

        // Drop stale ids that no longer resolve.
        expanded.retain(|id| self.scene.get_node(*id).is_some());
        expanded
    }

    /// Evict static-cache entries for `ids` and their ancestors, and clear nested
    /// composition surface caches when a composition node is on the chain.
    fn evict_caches_for(&mut self, ids: &std::collections::HashSet<NodeId>) {
        for &id in ids {
            let mut current = Some(id);
            while let Some(cid) = current {
                self.static_cache.remove(&cid);
                if let Some(node) = self.scene.get_node(cid) {
                    if let Some(comp) = node
                        .element
                        .as_any()
                        .downcast_ref::<crate::node::composition::CompositionNode>()
                    {
                        if let Ok(mut guard) = comp.surface_cache.lock() {
                            *guard = None;
                        }
                    }
                    current = node.parent;
                } else {
                    break;
                }
            }
        }
    }

    /// Record that public API mutated `ids` in a way that can change pixels or layout.
    ///
    /// - Marks each live id dirty (merged into `dirty_nodes`).
    /// - Evicts static-cache entries for the ids and their ancestors.
    /// - Clears nested composition surface caches when a composition node is affected.
    ///
    /// Call this from scripting/property mutations so correctness never depends on
    /// manual `clear_static_cache`.
    pub fn note_mutation(&mut self, ids: impl IntoIterator<Item = NodeId>) {
        let mut seeds: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        for id in ids {
            if self.scene.get_node(id).is_some() {
                seeds.insert(id);
            }
        }
        if seeds.is_empty() {
            return;
        }
        self.evict_caches_for(&seeds);
        self.dirty_nodes.extend(seeds);
    }

    /// Convenience: mark a single node mutated (pixels/layout).
    pub fn note_node_mutation(&mut self, id: NodeId) {
        self.note_mutation([id]);
    }

    /// Whether the dependency graph has been built via [`compile`].
    ///
    /// Incremental render requires a compiled graph; otherwise the pipeline
    /// falls back to a full frame (plan 03 safe default).
    pub fn is_compiled(&self) -> bool {
        !self.compiler.is_empty()
    }

    /// Drop every static-cache entry. Prefer [`note_mutation`] for targeted eviction.
    pub fn clear_all_static_cache(&mut self) {
        self.static_cache.clear();
    }

    /// Triggers `post_layout` on all active nodes.
    ///
    /// This is called after the Layout Engine has computed the final boxes, allowing elements
    /// to adjust their internal state (e.g., text resizing) based on the final layout.
    pub fn run_post_layout(&mut self, global_time: f64) {
        for node in self.scene.nodes.iter_mut().flatten() {
            if (node.last_visit_time - global_time).abs() < 0.0001 {
                node.element.post_layout(node.layout_rect);
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
    /// `input_hashes` is updated to reflect each node's current structural inputs.
    ///
    /// After the graph is built, the scene is scored against `style_policy`.
    /// Any violations are emitted as `tracing` logs (errors/warnings/info)
    /// and stored in `self.last_policy_score` so callers can inspect them before
    /// export.
    pub fn compile(&mut self) {
        self.compiler.clear();
        self.dirty_nodes.clear();

        for (node_id, node) in self.scene.iter_alive() {
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

            // Children collection: the parent depends on its children so that
            // adding/removing a child can invalidate the parent without a manual
            // recompile. Record this for every node so that a childless parent
            // still gets marked dirty when its first child is added.
            self.compiler.record(node_id, DependencyInput::Children(node_id));

            // Snapshot the current structural input hash for cache invalidation.
            let hash = node.compute_input_hash();
            self.input_hashes.insert(node_id, hash);
        }

        // Prune hashes for destroyed nodes so the map does not grow without
        // bound as node IDs are reused.
        self.input_hashes.retain(|&id, _| self.scene.get_node(id).is_some());

        // Run the style policy scorer over the timeline so violations are
        // emitted as warnings/errors before export. Export compiles again
        // after the HTML frontend has, so an unchanged violation list logs once.
        let score = self.score_style_policy_for_compile();
        let messages = policy_messages(&score);
        if self.last_policy_score.as_ref().map(policy_messages).as_ref() != Some(&messages) {
            for (severity, message) in messages {
                match severity {
                    IssueSeverity::Error => tracing::error!("{}", message),
                    IssueSeverity::Warning => tracing::warn!("{}", message),
                    IssueSeverity::Info => tracing::info!("{}", message),
                }
            }
        }
        self.last_policy_score = Some(score);
    }

    /// Score the scene against `style_policy` over the full timeline duration.
    ///
    /// Used internally by `compile()` to surface production-rule violations
    /// before export. Returns the computed `PolicyScore`.
    fn score_style_policy_for_compile(&self) -> PolicyScore {
        let duration_minutes = crate::style_policy::total_duration_minutes(self);
        let duration = duration_minutes * 60.0;
        let time_range = if duration > 0.0 { (0.0, duration) } else { (0.0, 1.0) };
        self.score_style_policy(time_range, self.style_policy.samples.max(2))
    }

    /// Compute the transitive set of nodes affected by the given inputs
    /// without marking anything dirty.
    ///
    /// This mirrors the dependency traversal performed by [`invalidate`] but
    /// does not compare input hashes or store a dirty set. It is useful for
    /// introspection and for previewing which nodes would be recomputed before
    /// committing to an an invalidation.
    pub fn affected_nodes(&self, inputs: Vec<DependencyInput>) -> std::collections::HashSet<NodeId> {
        self.compiler.affected_nodes(&inputs)
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
            if let Some(node) = self.scene.get_node(*node_id) {
                let current_hash = node.compute_input_hash();
                match self.input_hashes.get(node_id).copied() {
                    None => {
                        // Never compiled; treat as dirty and snapshot the hash
                        // so future invalidations have a baseline.
                        self.input_hashes.insert(*node_id, current_hash);
                        dirty.insert(*node_id);
                    }
                    Some(stored_hash) if stored_hash != current_hash => {
                        dirty.insert(*node_id);
                    }
                    _ => {}
                }
            }
        }

        // Evict static/comp caches for hash-dirtied nodes (plan 03).
        // Property mutations that don't change structural hashes must go through
        // `note_mutation` (e.g. set_color) rather than force-dirtying every Node input.
        self.evict_caches_for(&dirty);
        // Merge with any dirties previously recorded via note_mutation.
        self.dirty_nodes.extend(dirty.iter().copied());
        dirty
    }

    /// Score the current scene against the configured `StylePolicy`.
    ///
    /// The scorer samples the scene over `time_range` using `samples` points.
    /// Results include per-dimension sub-scores and a list of violations.
    /// Time-varying dimensions such as motion require at least two samples;
    /// values below two are clamped to two.
    pub fn score_style_policy(
        &self,
        time_range: (f64, f64),
        samples: usize,
    ) -> PolicyScore {
        StylePolicyScorer::new().score(self, &self.style_policy, time_range, samples.max(2))
    }

}

/// The log lines `compile()` emits for a score, with their severities.
fn policy_messages(score: &PolicyScore) -> Vec<(IssueSeverity, String)> {
    score
        .violations
        .iter()
        .map(|violation| {
            let node = match violation.node_id {
                Some(id) => format!(" (node {})", id),
                None => String::new(),
            };
            let message = format!(
                "StylePolicy violation [{}]: {}{}",
                violation.rule, violation.description, node
            );
            (violation.severity, message)
        })
        .collect()
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
            30.0,
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
            30.0,
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
            30.0,
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
            30.0,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
            None,
        );

        let mut box_node = BoxNode::new();
        box_node.style.size.width = taffy::style::Dimension::length(1080.0);
        box_node.style.size.height = taffy::style::Dimension::length(1920.0);
        let root = director.scene.add_node(Box::new(box_node));
        director.push_scene_root_at(root, 0.0, 2.0, Some("main".into()));

        let snapshot = crate::render_manifest::layout_snapshot(&mut director, 0.5);

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
