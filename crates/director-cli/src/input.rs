use crate::run::{eval_script, resolve_movie, ResolveOpts};
use clap::{Args, ValueEnum};
use director_core::{scripting::MovieHandle, video_wrapper::RenderMode};
use director_html::{CompileOptions, CompiledHtml, NativeProject, CAPTURE_FORMAT, PROJECT_FORMAT};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Debug, Args, Clone)]
pub struct Input {
    /// Rhai, HTML, capture JSON, native .dirproj or core project JSON
    pub input: PathBuf,
    #[arg(long, default_value_t = 1920)]
    pub width: i32,
    #[arg(long, default_value_t = 1080)]
    pub height: i32,
    #[arg(long, default_value_t = 30.0)]
    pub fps: f64,
    #[arg(long, default_value_t = 5.0)]
    pub duration: f64,
    #[arg(long)]
    pub composition: Option<String>,
    #[arg(long)]
    pub props: Option<String>,
}

impl Input {
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

    pub fn load(&self, mode: RenderMode) -> Result<LoadedInput, String> {
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
            return Ok(LoadedInput {
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
                return Ok(LoadedInput {
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
        let source_inspection = compiled.source_inspection();
        let dependency_paths = compiled.dependency_paths();
        Ok(LoadedInput {
            movie: MovieHandle {
                director: Arc::new(Mutex::new(compiled.director)),
            },
            compositions: self.composition.iter().cloned().collect(),
            source_kind: compiled.source_kind,
            sample_rate: compiled.sample_rate,
            element_ids: compiled.element_ids,
            source_inspection,
            dependency_paths,
        })
    }
}

#[derive(Debug, ValueEnum, Clone, Copy, Default)]
pub enum Encoder {
    #[default]
    Software,
    Nvenc,
}

impl Encoder {
    pub fn mode(self) -> director_core::video_wrapper::EncoderMode {
        match self {
            Self::Software => director_core::video_wrapper::EncoderMode::Software,
            Self::Nvenc => director_core::video_wrapper::EncoderMode::Nvenc,
        }
    }
    pub fn codec(self) -> &'static str {
        director_core::video_wrapper::get_encoder_codec(self.mode())
    }
}

pub struct LoadedInput {
    pub movie: MovieHandle,
    pub compositions: Vec<String>,
    pub source_kind: String,
    pub sample_rate: Option<f64>,
    pub element_ids: BTreeMap<String, director_core::types::NodeId>,
    pub source_inspection: Option<Arc<director_html::SourceInspection>>,
    pub dependency_paths: Vec<PathBuf>,
}

impl LoadedInput {
    pub fn inspect(&self, time: f64) -> Result<serde_json::Value, String> {
        if !time.is_finite() || time < 0.0 {
            return Err("inspection time must be finite and nonnegative".into());
        }
        let mut director = self
            .movie
            .director
            .lock()
            .map_err(|_| "Director lock poisoned")?;
        let manifest = director_core::render_manifest::build_render_manifest(&mut director, time);
        Ok(serde_json::json!({
            "source_kind":self.source_kind, "sample_rate":self.sample_rate,
            "element_ids":self.element_ids, "manifest":manifest,
            "sampled_elements":self.source_inspection.as_ref().map(|s| s.at(time)),
            "missing_required_fonts":director.missing_required_fonts,
            "audio_tracks":director.audio_mixer.tracks.iter().flatten().map(|t| serde_json::json!({"start":t.start_time,"duration":t.duration,"stereo_sample_frames":t.samples.len()/2})).collect::<Vec<_>>()
        }))
    }
}
