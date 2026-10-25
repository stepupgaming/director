//! One open project: a revisioned scene document plus warm render state.

use crate::error::{ErrorCode, HostError};
use crate::load::Source;
use crate::patch::{self, PatchOp};
use director_core::render_manifest::build_render_manifest;
use director_core::scene_export::{export_scene, node_ref};
use director_core::scene_import::{import_scene, ImportRegistry};
use director_core::video_wrapper::RenderMode;
use director_core::{AssetLoader, Director};
use director_html::SourceInspection;
use director_render::{output_dimensions, ExportOptions, Frame, FrameSession, GpuContext, RenderError, Renderer};
use director_scene::{NodeRef, SceneDocument};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// Core node kinds plus the `director-html` kinds.
pub fn import_registry() -> ImportRegistry {
    let mut registry = ImportRegistry::core();
    director_html::register_import(&mut registry);
    registry
}

/// An open project. Each accepted patch replaces the document and advances
/// the revision; a rejected patch leaves both unchanged.
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
    /// Evaluates `inspect`, imported on first use after each revision.
    inspector: Option<Director>,
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
        let director = loaded
            .movie
            .director
            .lock()
            .map_err(|_| HostError::new(ErrorCode::Load, "the loaded Director lock is poisoned"))?;
        let document = export_scene(&director).map_err(|error| HostError::new(ErrorCode::Load, error.to_string()))?;
        let mut session = Self::from_document(document, director.assets.loader.clone())?;
        session.names = loaded.element_ids.iter().map(|(name, id)| (name.clone(), node_ref(*id))).collect();
        session.source_kind = loaded.source_kind.clone();
        session.missing_fonts = director.missing_required_fonts.clone();
        session.sampled = loaded.source_inspection.clone();
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
        })
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
        self.frames = None;
        self.revision += 1;
        Ok(self.revision)
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
            duration: clips.iter().map(|clip| clip.start + clip.duration).fold(0.0, f64::max),
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
        let inspector = match &mut self.inspector {
            Some(inspector) => inspector,
            slot => slot.insert(
                import_scene(&self.document, self.loader.clone(), &self.registry, RenderMode::Export)
                    .map_err(|error| HostError::new(ErrorCode::Render, error.to_string()))?,
            ),
        };
        let manifest = build_render_manifest(inspector, time);
        Ok(serde_json::json!({
            "revision": self.revision,
            "source_kind": self.source_kind,
            "names": self.names,
            "manifest": manifest,
            "sampled_elements": self.sampled.as_ref().map(|sampled| sampled.at(time)),
            "missing_required_fonts": self.missing_fonts,
        }))
    }

    /// Paint the frame at `time` seconds on a session kept warm across calls
    /// until the next accepted patch.
    pub fn render_frame(&mut self, time: f64) -> Result<Frame, HostError> {
        check_time(time)?;
        let render = |error: RenderError| HostError::new(ErrorCode::Render, error.to_string());
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
}

fn check_time(time: f64) -> Result<(), HostError> {
    if time.is_finite() && time >= 0.0 {
        Ok(())
    } else {
        Err(HostError::new(ErrorCode::InvalidArgument, "time must be finite and nonnegative"))
    }
}
