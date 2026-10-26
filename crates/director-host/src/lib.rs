//! Host session API for Director.
//!
//! A [`Session`] opens any authoring input into a revisioned
//! `director_scene::SceneDocument`, applies atomic [`PatchOp`]s by authored
//! name or node id, outlines and diffs documents, inspects layout, paints frames and exports with
//! cancellation and progress. The CLI, an MCP server and a future editor link
//! this crate. Every failure is a [`HostError`] with a stable [`ErrorCode`].

mod diff;
pub mod error;
pub mod load;
pub mod patch;
pub mod script;
mod session;

pub use diff::{diff, Change, Diff, DiffNode, NodeChanges};
pub use error::{ErrorCode, HostError};
pub use load::{Loaded, Source};
pub use patch::{NodeSelector, Patch, PatchOp};
pub use session::{import_registry, ExportSettings, Outline, OutlineClip, OutlineNode, Session};
