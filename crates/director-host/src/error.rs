//! The one error every host operation returns, with a stable code for clients.

use schemars::JsonSchema;
use serde::Serialize;

/// What kind of failure a [`HostError`] is. Clients branch on this code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The input did not load or compile.
    Load,
    /// The patch named a base revision other than the current one.
    Conflict,
    /// A node selector or session id matched nothing.
    NotFound,
    /// A patch op or the document it produced was rejected.
    InvalidPatch,
    /// A time or option was out of range.
    InvalidArgument,
    /// A frame failed to paint.
    Render,
    /// The export failed to paint, encode or write.
    Export,
    /// The caller cancelled the operation.
    Cancelled,
    /// The scene needs fonts that are not installed or bundled.
    MissingFont,
    /// An output path resolves outside the roots the server may write to.
    OutputOutsideRoot,
}

/// A failed host operation. [`HostError::envelope`] is the JSON form every
/// client prints: `{"error":{"code","message"}}`.
#[derive(Clone, Debug, PartialEq, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct HostError {
    pub code: ErrorCode,
    pub message: String,
}

impl HostError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// `{"error":{"code","message"}}`.
    pub fn envelope(&self) -> serde_json::Value {
        serde_json::json!({ "error": self })
    }
}
