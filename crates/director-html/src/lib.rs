//! Compile HTML and CSS to native Director scene data.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
mod animation;
mod assets;
mod background_box;
mod background_tiles;
mod box_edges;
mod calc_len;
mod capture;
mod compiler;
mod css;
mod decoration_marks;
mod decoration_value;
mod element;
mod gradient;
mod grid_template;
mod object_pos;
mod project;
mod shader;
mod style;
mod transform_origin;
pub use capture::{
    compile_capture, compile_capture_file, Capture, CapturedFrame, CapturedMedia, CapturedNode,
    CapturedShader, CapturedShaderImage, CAPTURE_FORMAT,
};
pub use compiler::{compile, compile_file};
pub use project::{ElementEdit, NativeProject, ProjectSource, PROJECT_FORMAT};

pub struct CompiledHtml {
    pub director: director_core::Director,
    pub element_ids: std::collections::BTreeMap<String, director_core::types::NodeId>,
    pub source_kind: String,
    pub sample_rate: Option<f64>,
    pub node_count: usize,
    pub(crate) source_inspection: Option<std::sync::Arc<SourceInspection>>,
    pub(crate) assets: std::sync::Arc<assets::AssetStore>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InspectedElement {
    pub id: String,
    pub tag: String,
    pub bounds: [f32; 4],
    pub text: Option<String>,
    pub children: usize,
}

pub struct SourceInspection {
    pub(crate) samples: Vec<Vec<InspectedElement>>,
    pub(crate) sample_rate: f64,
}

impl SourceInspection {
    pub fn at(&self, time: f64) -> &[InspectedElement] {
        let index =
            ((time.max(0.0) * self.sample_rate).floor() as usize).min(self.samples.len() - 1);
        &self.samples[index]
    }
}

impl CompiledHtml {
    pub fn dependency_paths(&self) -> Vec<PathBuf> {
        self.assets.dependencies()
    }
    pub fn source_inspection(&self) -> Option<std::sync::Arc<SourceInspection>> {
        self.source_inspection.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub context: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("{0}")]
    Input(String),
    #[error("HTML compilation failed: {0:?}")]
    Diagnostics(Vec<Diagnostic>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileOptions {
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub duration: f64,
    #[serde(skip, default = "default_base_dir")]
    pub base_dir: PathBuf,
    pub composition: Option<String>,
}

fn default_base_dir() -> PathBuf {
    PathBuf::from(".")
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 30.0,
            duration: 5.0,
            base_dir: PathBuf::from("."),
            composition: None,
        }
    }
}
