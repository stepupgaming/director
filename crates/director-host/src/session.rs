//! One open project: a revisioned scene document plus warm render state.

use crate::error::{ErrorCode, HostError};
use crate::load::{Loaded, Source};
use crate::patch::{self, PatchOp};
use director_core::audio::AudioMixer;
use director_core::render_manifest::{build_render_manifest, RenderManifest};
use director_core::scene_export::{export_scene, node_ref};
use director_core::scene_import::{import_scene, ImportRegistry};
use director_core::timeline::{audio_samples_per_video_frame, duration_to_frame_count};
use director_core::video_wrapper::RenderMode;
use director_core::{AssetLoader, Director};
use director_html::{ElementEdit, NativeProject, SourceInspection};
use director_render::{output_dimensions, ExportOptions, Frame, FrameSession, GpuContext, RenderError, Renderer};
use director_scene::{NodeRef, SceneDocument};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// A source file and its modification time and length when it was last read.
type Fingerprint = (PathBuf, Option<(SystemTime, u64)>);

/// Core node kinds plus the `director-html` kinds.
pub fn import_registry() -> ImportRegistry {
    let mut registry = ImportRegistry::core();
    director_html::register_import(&mut registry);
    registry
}

/// An open project. Each accepted patch, reload or project edit replaces the
/// document and advances the revision; a rejected one leaves both unchanged.
/// With the `vulkan` feature a session that owns a GPU context stays on the
/// thread that created the context.
pub struct Session {
    document: Arc<SceneDocument>,
    revision: u64,
    names: BTreeMap<String, NodeRef>,
    loader: Arc<dyn AssetLoader>,
    registry: ImportRegistry,
    source_kind: String,
    missing_fonts: Vec<String>,
    sampled: Option<Arc<SourceInspection>>,
    /// Paints `render_frame`, imported on first use after each revision.
    frames: Option<FrameSession>,
    /// Evaluates `inspect` and the audio mix, imported on first use after each revision.
    inspector: Option<Director>,
    /// The input `reload` reads again; `None` for a session over a document.
    source: Option<Source>,
    compositions: Vec<String>,
    sample_rate: Option<f64>,
    dependencies: Vec<PathBuf>,
    fingerprints: Vec<Fingerprint>,
    reload_error: Option<HostError>,
    /// The timeline mix of the current revision as a WAV file.
    audio: Option<Arc<[u8]>>,
    /// The edited native project behind the current revision, until saved or reloaded.
    project: Option<NativeProject>,
    #[cfg(feature = "vulkan")]
    gpu: Option<Gpu>,
}

/// A GPU context the session owns and the warm frame target painted on it.
#[cfg(feature = "vulkan")]
struct Gpu {
    context: GpuContext,
    frames: Option<director_render::TargetSession>,
}

/// Export settings beyond the document.
#[derive(Clone)]
pub struct ExportSettings {
    /// Raster scale in `(0, 1]`; layout sizes stay put.
    pub scale: f32,
    /// Leave uncovered pixels clear and write VP9 with alpha. `None` keeps the document setting.
    pub alpha: Option<bool>,
    pub options: ExportOptions,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            scale: 1.0,
            alpha: None,
            options: ExportOptions::default(),
        }
    }
}

/// The node tree and timeline of one revision.
#[derive(Clone, Debug, Serialize)]
pub struct Outline {
    pub revision: u64,
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub duration: f64,
    pub clips: Vec<OutlineClip>,
    pub roots: Vec<OutlineNode>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OutlineClip {
    pub track: usize,
    pub name: Option<String>,
    pub start: f64,
    pub duration: f64,
    pub scene_root: NodeRef,
}

#[derive(Clone, Debug, Serialize)]
pub struct OutlineNode {
    pub id: NodeRef,
    /// Authored name, such as an HTML `id`.
    pub name: Option<String>,
    pub kind: String,
    pub children: Vec<OutlineNode>,
}

impl Session {
    /// Load `source` in export mode and hold its scene document at revision 0.
    pub fn open(source: &Source) -> Result<Self, HostError> {
        let loaded = source.load(RenderMode::Export).map_err(|error| HostError::new(ErrorCode::Load, error))?;
        let (document, loader, missing_fonts) = export_loaded(&loaded)?;
        let mut session = Self::from_document(document, loader)?;
        session.adopt(loaded, missing_fonts);
        session.source = Some(source.clone());
        session.fingerprints = session.fingerprints();
        Ok(session)
    }

    /// Hold `document` at revision 0. Assets resolve through `loader`. The
    /// document must import.
    pub fn from_document(document: SceneDocument, loader: Arc<dyn AssetLoader>) -> Result<Self, HostError> {
        let registry = import_registry();
        let inspector = import_scene(&document, loader.clone(), &registry, RenderMode::Export)
            .map_err(|error| HostError::new(ErrorCode::Load, error.to_string()))?;
        Ok(Self {
            document: Arc::new(document),
            revision: 0,
            names: BTreeMap::new(),
            loader,
            registry,
            source_kind: "scene".into(),
            missing_fonts: Vec::new(),
            sampled: None,
            frames: None,
            inspector: Some(inspector),
            source: None,
            compositions: Vec::new(),
            sample_rate: None,
            dependencies: Vec::new(),
            fingerprints: Vec::new(),
            reload_error: None,
            audio: None,
            project: None,
            #[cfg(feature = "vulkan")]
            gpu: None,
        })
    }

    /// Paint `render_frame` on `context` from now on. The session then stays
    /// on the context's thread.
    #[cfg(feature = "vulkan")]
    pub fn use_gpu(&mut self, context: GpuContext) {
        self.gpu = Some(Gpu { context, frames: None });
    }

    /// `vulkan` when frames paint on an owned GPU context, else `cpu`.
    pub fn backend(&self) -> &'static str {
        #[cfg(feature = "vulkan")]
        if self.gpu.is_some() {
            return "vulkan";
        }
        "cpu"
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn document(&self) -> &Arc<SceneDocument> {
        &self.document
    }

    /// Authored names and the nodes they select.
    pub fn names(&self) -> &BTreeMap<String, NodeRef> {
        &self.names
    }

    /// The loader's source kind, such as `rhai` or `html`, or `scene` for a document.
    pub fn source_kind(&self) -> &str {
        &self.source_kind
    }

    /// The input the session loaded, if it opened one.
    pub fn source(&self) -> Option<&Source> {
        self.source.as_ref()
    }

    /// Rhai compositions the input registers, or the HTML composition it selected.
    pub fn compositions(&self) -> &[String] {
        &self.compositions
    }

    /// The audio sample rate a capture declares.
    pub fn sample_rate(&self) -> Option<f64> {
        self.sample_rate
    }

    /// End of the last clip in seconds.
    pub fn duration(&self) -> f64 {
        timeline_duration(&self.document)
    }

    /// Required bundled fonts the system lacks. Export refuses while any are missing.
    pub fn missing_fonts(&self) -> &[String] {
        &self.missing_fonts
    }

    /// Why the last reload failed, until one succeeds.
    pub fn reload_error(&self) -> Option<&HostError> {
        self.reload_error.as_ref()
    }

    /// The edited native project behind the current revision, until it is saved or reloaded.
    pub fn project(&self) -> Option<&NativeProject> {
        self.project.as_ref()
    }

    /// Whether the input or a file it depends on changed since the last load or reload attempt.
    pub fn source_changed(&self) -> bool {
        self.source.is_some() && self.fingerprints() != self.fingerprints
    }

    /// Load the input again as the next revision. A failure keeps the current
    /// document and is reported by [`Session::reload_error`] until a reload
    /// succeeds. Unsaved project edits are dropped.
    pub fn reload(&mut self) -> Result<u64, HostError> {
        let Some(source) = &self.source else {
            return Err(HostError::new(ErrorCode::InvalidArgument, "the session has no input to reload"));
        };
        let attempted = self.fingerprints();
        let loaded = source.load(RenderMode::Export).map_err(|error| HostError::new(ErrorCode::Load, error));
        match loaded.and_then(|loaded| self.replace(loaded)) {
            Ok(revision) => {
                self.reload_error = None;
                self.project = None;
                self.fingerprints = self.fingerprints();
                Ok(revision)
            }
            Err(error) => {
                // Retry when a source changes again.
                self.fingerprints = attempted;
                self.reload_error = Some(error.clone());
                Err(error)
            }
        }
    }

    /// Apply element-ID keyed edits to the native project input and hold the
    /// edited project as the next revision. Edits that do not compile leave
    /// the session unchanged. [`Session::save_project`] writes the result.
    pub fn edit_project(&mut self, edits: BTreeMap<String, ElementEdit>) -> Result<u64, HostError> {
        let mut project = match &self.project {
            Some(project) => project.clone(),
            None => {
                let source = self
                    .source
                    .as_ref()
                    .ok_or_else(|| HostError::new(ErrorCode::InvalidArgument, "project edits need a native project input"))?;
                NativeProject::load(&source.input).map_err(|error| {
                    HostError::new(ErrorCode::InvalidArgument, format!("project edits need a native project input: {error}"))
                })?
            }
        };
        let invalid = |error: director_html::CompileError| HostError::new(ErrorCode::InvalidPatch, error.to_string());
        project.apply_edits(edits).map_err(invalid)?;
        let compiled = project.compile().map_err(invalid)?;
        let revision = self.replace(Loaded::from_compiled(compiled, Vec::new()))?;
        self.project = Some(project);
        Ok(revision)
    }

    /// Write the edited native project to `path` atomically.
    pub fn save_project(&self, path: &Path) -> Result<(), HostError> {
        let project = self
            .project
            .as_ref()
            .ok_or_else(|| HostError::new(ErrorCode::InvalidArgument, "the session holds no project edits"))?;
        project
            .save(path)
            .map_err(|error| HostError::new(ErrorCode::Export, format!("save {}: {error}", path.display())))
    }

    /// Apply `ops` written against `base_revision`. The patched document must
    /// import before it replaces the current one. Returns the new revision;
    /// an empty patch keeps the current one.
    pub fn patch(&mut self, base_revision: u64, ops: &[PatchOp]) -> Result<u64, HostError> {
        if base_revision != self.revision {
            return Err(HostError::new(
                ErrorCode::Conflict,
                format!("the patch targets revision {base_revision}; the session is at revision {}", self.revision),
            ));
        }
        if ops.is_empty() {
            return Ok(self.revision);
        }
        let document = patch::apply(&self.document, &self.names, ops)?;
        let inspector = import_scene(&document, self.loader.clone(), &self.registry, RenderMode::Export).map_err(|error| {
            HostError::new(ErrorCode::InvalidPatch, format!("the patched document does not import: {error}"))
        })?;
        self.names.retain(|_, node| document.nodes.iter().any(|record| record.id == *node));
        self.document = Arc::new(document);
        self.inspector = Some(inspector);
        Ok(self.advance())
    }

    /// The node tree and clips of the current revision.
    pub fn outline(&self) -> Outline {
        let document = &self.document;
        let named: BTreeMap<(u32, u32), &str> = self
            .names
            .iter()
            .map(|(name, node)| ((node.index, node.generation), name.as_str()))
            .collect();
        let records: BTreeMap<(u32, u32), _> = document.nodes.iter().map(|record| ((record.id.index, record.id.generation), record)).collect();
        fn build(
            id: NodeRef,
            records: &BTreeMap<(u32, u32), &director_scene::NodeRecord>,
            named: &BTreeMap<(u32, u32), &str>,
        ) -> Option<OutlineNode> {
            let record = records.get(&(id.index, id.generation))?;
            Some(OutlineNode {
                id,
                name: named.get(&(id.index, id.generation)).map(|name| name.to_string()),
                kind: record.kind.clone(),
                children: record.children.iter().filter_map(|child| build(*child, records, named)).collect(),
            })
        }
        let clips: Vec<OutlineClip> = document
            .timeline
            .tracks
            .iter()
            .enumerate()
            .flat_map(|(track, entry)| {
                entry.clips.iter().map(move |clip| OutlineClip {
                    track,
                    name: clip.name.clone(),
                    start: clip.start_time,
                    duration: clip.duration,
                    scene_root: clip.scene_root,
                })
            })
            .collect();
        Outline {
            revision: self.revision,
            width: document.width,
            height: document.height,
            fps: document.fps,
            duration: timeline_duration(document),
            roots: document
                .nodes
                .iter()
                .filter(|record| record.parent.is_none())
                .filter_map(|record| build(record.id, &records, &named))
                .collect(),
            clips,
        }
    }

    /// Layout, timeline and authored names at `time`, with the sampled
    /// browser elements of a capture.
    pub fn inspect(&mut self, time: f64) -> Result<serde_json::Value, HostError> {
        check_time(time)?;
        let inspector = self.inspector()?;
        let manifest = build_render_manifest(inspector, time);
        let audio_tracks: Vec<_> = inspector
            .audio_mixer
            .tracks
            .iter()
            .flatten()
            .map(|track| {
                serde_json::json!({
                    "start": track.start_time,
                    "duration": track.duration,
                    "stereo_sample_frames": track.samples.len() / 2,
                })
            })
            .collect();
        Ok(serde_json::json!({
            "revision": self.revision,
            "source_kind": self.source_kind,
            "sample_rate": self.sample_rate,
            "element_ids": self.names,
            "manifest": manifest,
            "sampled_elements": self.sampled.as_ref().map(|sampled| sampled.at(time)),
            "missing_required_fonts": self.missing_fonts,
            "audio_tracks": audio_tracks,
        }))
    }

    /// The render manifest of the current revision at `time`: node bounds,
    /// kinds and timeline state.
    pub fn manifest(&mut self, time: f64) -> Result<RenderManifest, HostError> {
        check_time(time)?;
        Ok(build_render_manifest(self.inspector()?, time))
    }

    /// The audio mixer of the current revision with its decoded tracks.
    pub fn audio_mixer(&mut self) -> Result<&AudioMixer, HostError> {
        Ok(&self.inspector()?.audio_mixer)
    }

    /// The timeline mix of the current revision as a float-stereo WAV, mixed
    /// in export-sized chunks on the export clock and cached until the next revision.
    pub fn audio_wav(&mut self) -> Result<Arc<[u8]>, HostError> {
        if let Some(audio) = &self.audio {
            return Ok(Arc::clone(audio));
        }
        let director = self.inspector()?;
        let rate = director.audio_mixer.sample_rate;
        let fps = director.fps;
        let frames = duration_to_frame_count(director.timeline_duration(), fps);
        let samples = audio_samples_per_video_frame(rate, fps, 1);
        let data_size = frames
            .checked_mul(samples as u64)
            .and_then(|count| count.checked_mul(8))
            .filter(|size| *size <= (u32::MAX - 36) as u64)
            .ok_or_else(|| HostError::new(ErrorCode::Render, "the audio exceeds the RIFF/WAV size limit"))?
            as u32;
        let mut wav = Vec::with_capacity(data_size as usize + 44);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(data_size + 36).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        // IEEE float, two channels, 32-bit samples.
        wav.extend_from_slice(&3u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 8).to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());
        wav.extend_from_slice(&32u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());
        for frame in 0..frames {
            for sample in director.mix_audio(samples, frame as f64 / fps) {
                wav.extend_from_slice(&sample.to_le_bytes());
            }
        }
        let audio: Arc<[u8]> = wav.into();
        self.audio = Some(Arc::clone(&audio));
        Ok(audio)
    }

    /// Paint the frame at `time` seconds on a session kept warm across calls
    /// until the next revision, on the owned GPU context when there is one.
    pub fn render_frame(&mut self, time: f64) -> Result<Frame, HostError> {
        check_time(time)?;
        let render = |error: RenderError| HostError::new(ErrorCode::Render, error.to_string());
        #[cfg(feature = "vulkan")]
        if let Some(gpu) = &mut self.gpu {
            if gpu.frames.is_none() {
                let renderer = Renderer::new(Arc::clone(&self.document), self.loader.clone()).with_registry(self.registry.clone());
                gpu.frames = Some(director_render::TargetSession::new(&renderer, Some(&mut gpu.context)).map_err(render)?);
            }
            let mut frame = Frame {
                width: 0,
                height: 0,
                rgba: Vec::new(),
            };
            let frames = gpu.frames.as_mut().expect("GPU frame session");
            frames.render_into(time, &mut frame, Some(&mut gpu.context)).map_err(render)?;
            return Ok(frame);
        }
        if self.frames.is_none() {
            self.frames = Some(self.renderer(Arc::clone(&self.document)).session().map_err(render)?);
        }
        self.frames.as_mut().expect("frame session").render(time).map_err(render)
    }

    /// Export the current revision to `path`, painting on `gpu` when given.
    pub fn export(&self, path: &Path, settings: &ExportSettings, gpu: Option<&mut GpuContext>) -> Result<(), HostError> {
        if !self.missing_fonts.is_empty() {
            return Err(HostError::new(
                ErrorCode::MissingFont,
                format!("missing required fonts: {}", self.missing_fonts.join(", ")),
            ));
        }
        let mut document = (*self.document).clone();
        let (_, _, scale_x, scale_y) = output_dimensions(document.width, document.height, settings.scale)
            .map_err(|error| HostError::new(ErrorCode::InvalidArgument, error))?;
        (document.output_scale_x, document.output_scale_y) = (scale_x, scale_y);
        if let Some(alpha) = settings.alpha {
            document.transparent_background = alpha;
        }
        self.renderer(Arc::new(document)).export_options(path, &settings.options, gpu).map_err(|error| match error {
            RenderError::Cancelled => HostError::new(ErrorCode::Cancelled, error.to_string()),
            error => HostError::new(ErrorCode::Export, error.to_string()),
        })
    }

    fn renderer(&self, document: Arc<SceneDocument>) -> Renderer {
        Renderer::new(document, self.loader.clone()).with_registry(self.registry.clone())
    }

    pub(crate) fn inspector(&mut self) -> Result<&mut Director, HostError> {
        Ok(match &mut self.inspector {
            Some(inspector) => inspector,
            slot => slot.insert(
                import_scene(&self.document, self.loader.clone(), &self.registry, RenderMode::Export)
                    .map_err(|error| HostError::new(ErrorCode::Render, error.to_string()))?,
            ),
        })
    }

    /// Take the names and metadata of a fresh load.
    fn adopt(&mut self, loaded: Loaded, missing_fonts: Vec<String>) {
        self.names = loaded.element_ids.iter().map(|(name, id)| (name.clone(), node_ref(*id))).collect();
        self.source_kind = loaded.source_kind;
        self.missing_fonts = missing_fonts;
        self.sampled = loaded.source_inspection;
        self.compositions = loaded.compositions;
        self.sample_rate = loaded.sample_rate;
        self.dependencies = loaded.dependency_paths;
    }

    /// Replace the document with a fresh load as the next revision, once it imports.
    fn replace(&mut self, loaded: Loaded) -> Result<u64, HostError> {
        let (document, loader, missing_fonts) = export_loaded(&loaded)?;
        let inspector = import_scene(&document, loader.clone(), &self.registry, RenderMode::Export)
            .map_err(|error| HostError::new(ErrorCode::Load, error.to_string()))?;
        self.document = Arc::new(document);
        self.loader = loader;
        self.inspector = Some(inspector);
        self.adopt(loaded, missing_fonts);
        Ok(self.advance())
    }

    /// Drop the state of the previous revision and advance.
    fn advance(&mut self) -> u64 {
        self.frames = None;
        self.audio = None;
        #[cfg(feature = "vulkan")]
        if let Some(gpu) = &mut self.gpu {
            gpu.frames = None;
        }
        self.revision += 1;
        self.revision
    }

    fn fingerprints(&self) -> Vec<Fingerprint> {
        let Some(source) = &self.source else {
            return Vec::new();
        };
        std::iter::once(&source.input)
            .chain(&self.dependencies)
            .map(|path| {
                let stamp = std::fs::metadata(path)
                    .ok()
                    .and_then(|metadata| metadata.modified().ok().map(|modified| (modified, metadata.len())));
                (path.clone(), stamp)
            })
            .collect()
    }
}

/// The scene document, asset loader and missing required fonts of a fresh load.
type Exported = (SceneDocument, Arc<dyn AssetLoader>, Vec<String>);

fn export_loaded(loaded: &Loaded) -> Result<Exported, HostError> {
    let director = loaded
        .movie
        .director
        .lock()
        .map_err(|_| HostError::new(ErrorCode::Load, "the loaded Director lock is poisoned"))?;
    let document = export_scene(&director).map_err(|error| HostError::new(ErrorCode::Load, error.to_string()))?;
    Ok((document, director.assets.loader.clone(), director.missing_required_fonts.clone()))
}

fn timeline_duration(document: &SceneDocument) -> f64 {
    let clips = document.timeline.tracks.iter().flat_map(|track| &track.clips);
    clips.map(|clip| clip.start_time + clip.duration).fold(0.0, f64::max)
}

fn check_time(time: f64) -> Result<(), HostError> {
    if time.is_finite() && time >= 0.0 {
        Ok(())
    } else {
        Err(HostError::new(ErrorCode::InvalidArgument, "time must be finite and nonnegative"))
    }
}
