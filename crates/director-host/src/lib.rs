//! Host session API for Director.
//!
//! A [`Session`] opens any authoring input into a revisioned
//! `director_scene::SceneDocument`, applies atomic [`PatchOp`]s by authored
//! name or node id, outlines and diffs documents, inspects layout, scores QA, paints frames and exports with
//! cancellation and progress. An [`Editor`] drives a session on its own thread
//! with frames in a [`SharedFrames`] ring. The CLI, an MCP server and a future editor link
//! this crate. Every failure is a [`HostError`] with a stable [`ErrorCode`].

mod diff;
pub mod editor;
pub mod error;
pub mod frames;
pub mod load;
pub mod patch;
pub mod qa;
pub mod script;
mod session;

pub use diff::{diff, Change, Diff, DiffNode, NodeChanges};
pub use editor::{Command, Editor, EditorOptions, Event};
pub use error::{ErrorCode, HostError};
pub use frames::{FrameStamp, SharedFrame, SharedFrames};
pub use load::{Loaded, Source};
pub use patch::{NodeSelector, Patch, PatchOp};
pub use qa::{QaAudio, QaIssue, QaOptions, QaReport};
pub use session::{import_registry, AudioWindow, ExportSettings, Outline, OutlineClip, OutlineNode, Session, MAX_AUDIO_WINDOW};
