//! API manifest and JSON Schema export — sourced from [`director_core::api_catalog`].

pub use director_core::api_catalog::{api_manifest, api_manifest_json, ApiManifest};
use schemars::schema_for;

use crate::{MovieRequest, ProjectManifest};

/// JSON Schema for [`MovieRequest`] (declarative pipeline input).
pub fn export_movie_request_schema() -> String {
    let schema = schema_for!(MovieRequest);
    serde_json::to_string_pretty(&schema).expect("MovieRequest schema must serialize")
}

/// Combined export: API manifest + MovieRequest JSON Schema + ProjectManifest schema.
pub fn export_full_schema_bundle() -> String {
    let bundle = serde_json::json!({
        "api_manifest": api_manifest(),
        "movie_request_schema": schema_for!(MovieRequest),
        "project_manifest_schema": schema_for!(ProjectManifest),
    });
    serde_json::to_string_pretty(&bundle).expect("schema bundle must serialize")
}