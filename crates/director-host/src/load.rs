//! Loading every authoring input into a live `Director`: Rhai, HTML, capture
//! JSON, portable native projects and versioned core projects.

use crate::script::{eval_script, resolve_movie, ResolveOpts};
use director_core::{scripting::MovieHandle, video_wrapper::RenderMode};
use director_html::{CompileOptions, CompiledHtml, NativeProject, CAPTURE_FORMAT, PROJECT_FORMAT};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// An input path plus the settings HTML compiles with and the Rhai composition to build.
#[derive(Debug, Clone)]
pub struct Source {
    /// Rhai, HTML, capture JSON, native .dirproj or core project JSON
    pub input: PathBuf,
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub duration: f64,
    pub composition: Option<String>,
    /// Rhai composition props, inline JSON or a file path.
    pub props: Option<String>,
}

impl Source {
    pub fn new(input: PathBuf) -> Self {
        let options = CompileOptions::default();
        Self {
            input,
            width: options.width,
            height: options.height,
            fps: options.fps,
            duration: options.duration,
            composition: None,
            props: None,
        }
    }
    pub fn html_options(&self) -> CompileOptions {
        CompileOptions {
            width: self.width,
            height: self.height,
            fps: self.fps,
            duration: self.duration,
            composition: self.composition.clone(),
            ..Default::default()
        }
    }

    pub fn load_html(&self) -> Result<CompiledHtml, director_html::CompileError> {
        if self.props.is_some() {
            return Err(director_html::CompileError::Input(
                "HTML/project edits use project edit, not Rhai --props".into(),
            ));
        }
        if self
            .input
            .extension()
            .is_some_and(|e| e == "json" || e == "dirproj")
        {
            let bytes = std::fs::read(&self.input).map_err(|e| {
                director_html::CompileError::Input(format!("input {}: {e}", self.input.display()))
            })?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|e| director_html::CompileError::Input(format!("input JSON: {e}")))?;
            self.compile_json(value)
        } else {
            director_html::compile_file(&self.input, self.html_options())
        }
    }

    fn compile_json(
        &self,
        value: serde_json::Value,
    ) -> Result<CompiledHtml, director_html::CompileError> {
        if self.props.is_some() {
            return Err(director_html::CompileError::Input(
                "HTML/project edits use project edit, not Rhai --props".into(),
            ));
        }
        match value.get("format").and_then(|v| v.as_str()) {
            Some(PROJECT_FORMAT) => {
                if self.composition.is_some() {
                    return Err(director_html::CompileError::Input(
                        "native projects retain their composition; repack to change it".into(),
                    ));
                }
                let project: NativeProject = serde_json::from_value(value).map_err(|e| {
                    director_html::CompileError::Input(format!("project JSON: {e}"))
                })?;
                project.compile()
            }
            Some(CAPTURE_FORMAT) => {
                if self.composition.is_some() {
                    return Err(director_html::CompileError::Input(
                        "captures retain their composition".into(),
                    ));
                }
                let capture = serde_json::from_value(value).map_err(|e| {
                    director_html::CompileError::Input(format!("capture JSON: {e}"))
                })?;
                director_html::compile_capture(
                    capture,
                    self.input.parent().unwrap_or(std::path::Path::new(".")),
                )
            }
            format => Err(director_html::CompileError::Input(format!(
                "unsupported native input format {format:?}"
            ))),
        }
    }

    pub fn load(&self, mode: RenderMode) -> Result<Loaded, String> {
        if self.input.extension().is_some_and(|e| e == "rhai") {
            let evaluated = eval_script(&self.input, mode)?;
            let names = evaluated.registry.names();
            let movie = resolve_movie(
                &evaluated,
                ResolveOpts {
                    composition: self.composition.as_deref(),
                    props_spec: self.props.as_deref(),
                },
            )?;
            return Ok(Loaded {
                movie,
                compositions: names,
                source_kind: "rhai".into(),
                sample_rate: None,
                element_ids: BTreeMap::new(),
                source_inspection: None,
                dependency_paths: Vec::new(),
            });
        }
        let mut native_json = None;
        // Core projects have a versioned node contract, not a native authoring format.
        if self.input.extension().is_some_and(|e| e == "json") {
            let bytes = std::fs::read(&self.input).map_err(|e| e.to_string())?;
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| format!("input JSON: {e}"))?;
            if value.get("format").is_none() && value.get("version").is_some() {
                if self.composition.is_some() || self.props.is_some() {
                    return Err("core projects have no composition builders or --props".into());
                }
                let director = director_core::Director::load_project_json(&self.input)
                    .map_err(|e| e.to_string())?;
                return Ok(Loaded {
                    movie: MovieHandle {
                        director: Arc::new(Mutex::new(director)),
                    },
                    compositions: Vec::new(),
                    source_kind: "core-project".into(),
                    sample_rate: None,
                    element_ids: BTreeMap::new(),
                    source_inspection: None,
                    dependency_paths: Vec::new(),
                });
            }
            native_json = Some(value);
        }
        let compiled = match native_json {
            Some(value) => self.compile_json(value),
            None => self.load_html(),
        }
        .map_err(|e| e.to_string())?;
        Ok(Loaded::from_compiled(compiled, self.composition.iter().cloned().collect()))
    }
}

/// A loaded input: the live movie plus what its frontend knows about it.
pub struct Loaded {
    pub movie: MovieHandle,
    pub compositions: Vec<String>,
    pub source_kind: String,
    pub sample_rate: Option<f64>,
    pub element_ids: BTreeMap<String, director_core::types::NodeId>,
    pub source_inspection: Option<Arc<director_html::SourceInspection>>,
    pub dependency_paths: Vec<PathBuf>,
}

impl Loaded {
    /// Wrap compiled HTML, a capture or a native project with the composition names it was built from.
    pub fn from_compiled(compiled: CompiledHtml, compositions: Vec<String>) -> Self {
        let source_inspection = compiled.source_inspection();
        let dependency_paths = compiled.dependency_paths();
        Self {
            movie: MovieHandle {
                director: Arc::new(Mutex::new(compiled.director)),
            },
            compositions,
            source_kind: compiled.source_kind,
            sample_rate: compiled.sample_rate,
            element_ids: compiled.element_ids,
            source_inspection,
            dependency_paths,
        }
    }
}
