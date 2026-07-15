//! Offline dependency-graph inspector.
//!
//! Compiles a Rhai script without rendering and prints the dependency graph
//! JSON to stdout. Useful for debugging which nodes depend on which inputs.

use clap::{Parser, Subcommand};
use director_core::scripting::register_rhai_api_with_default_mode;
use director_core::video_wrapper::RenderMode;
use director_core::DefaultAssetLoader;
use rhai::Engine;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "director-inspect", about = "Inspect a Director project without rendering")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Compile a Rhai script and print the dependency graph JSON.
    InspectDeps {
        /// Path to the Rhai script to compile and inspect.
        path: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::InspectDeps { path } => inspect_deps(&path),
    };

    match result {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

fn inspect_deps(path: &PathBuf) -> Result<String, String> {
    let script = fs::read_to_string(path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;

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
    director.compile();

    director.compiler.to_json().map_err(|e| format!("failed to serialize dependency graph: {e}"))
}
