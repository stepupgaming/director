//! # Export Module
//!
//! Video export and encoding functionality.
//!
//! ## Responsibilities
//! - **Video Encoding**: FFmpeg integration via FFMPEG.
//! - **Motion Blur**: Multi-sample frame accumulation.
//! - **Audio Mixing**: Synchronizes audio with video frames.

pub mod video;

pub use video::{
    output_dimensions, render_export, render_export_at_scale, set_output_scale, validate_scale,
};
