//! Compile HTML and CSS to native Director scene data.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
mod animation;
mod capture;
mod compiler;
mod css;
mod element;
mod gradient;
mod style;
pub use capture::{
    compile_capture, compile_capture_file, Capture, CapturedFrame, CapturedNode, CAPTURE_FORMAT,
};
pub use compiler::{compile, compile_file};

pub struct CompiledHtml {
    pub director: director_core::Director,
    pub element_ids: std::collections::BTreeMap<String, director_core::types::NodeId>,
    pub source_kind: String,
    pub sample_rate: Option<f64>,
    pub node_count: usize,
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

#[derive(Debug, Clone)]
pub struct CompileOptions {
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub duration: f64,
    pub base_dir: PathBuf,
    pub composition: Option<String>,
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
