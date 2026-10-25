//! Command-line input flags. Loading lives in `director_host::load`.

use clap::{Args, ValueEnum};
use director_core::video_wrapper::RenderMode;
use director_host::Source;
use director_html::{CompileOptions, CompiledHtml};
use std::path::PathBuf;

pub use director_host::Loaded as LoadedInput;

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
        Self::from(Source::new(input))
    }

    /// The host source these flags describe.
    pub fn source(&self) -> Source {
        Source {
            input: self.input.clone(),
            width: self.width,
            height: self.height,
            fps: self.fps,
            duration: self.duration,
            composition: self.composition.clone(),
            props: self.props.clone(),
        }
    }

    pub fn html_options(&self) -> CompileOptions {
        self.source().html_options()
    }

    pub fn load_html(&self) -> Result<CompiledHtml, director_html::CompileError> {
        self.source().load_html()
    }

    pub fn load(&self, mode: RenderMode) -> Result<LoadedInput, String> {
        self.source().load(mode)
    }
}

impl From<Source> for Input {
    fn from(source: Source) -> Self {
        Self {
            input: source.input,
            width: source.width,
            height: source.height,
            fps: source.fps,
            duration: source.duration,
            composition: source.composition,
            props: source.props,
        }
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
