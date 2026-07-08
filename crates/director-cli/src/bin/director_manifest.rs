//! Agent-facing manifest export: API catalog, JSON schemas, project summaries, render bounds.

use clap::{Parser, Subcommand};
use director_core::render_manifest::render_manifest_json;
use director_core::scripting::register_rhai_api_with_default_mode;
use director_core::video_wrapper::RenderMode;
use director_core::DefaultAssetLoader;
use director_schema::{
    export_full_schema_bundle, export_movie_request_schema, export_project_manifest_json,
};
use rhai::Engine;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "director-manifest", about = "Export Director API and project manifests as JSON")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Full API catalog (effects, transitions, nodes, Rhai helpers).
    Api,
    /// JSON Schema bundle: API manifest + MovieRequest + ProjectManifest schemas.
    Schema,
    /// JSON Schema for MovieRequest only.
    MovieSchema,
    /// Static project summary from a MovieRequest JSON file (no render).
    FromJson {
        /// Path to MovieRequest JSON
        path: PathBuf,
    },
    /// Layout-time manifest from a Rhai script (includes per-node bounds).
    FromScript {
        /// Path to Rhai script
        path: PathBuf,
        /// Timeline instant (seconds) at which to sample layout bounds
        #[arg(long, default_value_t = 0.0)]
        time: f64,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Api => Ok(director_core::registry::api_manifest_json()),
        Command::Schema => Ok(export_full_schema_bundle()),
        Command::MovieSchema => Ok(export_movie_request_schema()),
        Command::FromJson { path } => from_json(&path),
        Command::FromScript { path, time } => from_script(&path, time),
    };

    match result {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

fn from_json(path: &PathBuf) -> Result<String, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let request: director_schema::MovieRequest =
        serde_json::from_str(&raw).map_err(|e| format!("parse MovieRequest: {e}"))?;
    Ok(export_project_manifest_json(&request))
}

fn from_script(path: &PathBuf, time: f64) -> Result<String, String> {
    let script = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut engine = Engine::new();
    register_rhai_api_with_default_mode(
        &mut engine,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
    );
    let movie = engine
        .eval::<director_core::scripting::MovieHandle>(&script)
        .map_err(|e| format!("script error: {e}"))?;
    let mut director = movie
        .director
        .lock()
        .map_err(|_| "director lock poisoned".to_string())?;
    Ok(render_manifest_json(&mut director, time))
}