//! `breakdown`: the shot breakdown of a reference video as `breakdown.json`.

use clap::Parser;
use director_breakdown::{analyze, schema, ShotOptions};
use std::path::PathBuf;

#[derive(Parser, Debug)]
pub struct BreakdownArgs {
    /// Video to analyse
    #[arg(required_unless_present = "schema")]
    pub video: Option<PathBuf>,
    /// Directory to write breakdown.json into; without it the document prints on stdout
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,
    /// Shots shorter than this merge into a neighbour, in seconds
    #[arg(long, default_value_t = 0.35)]
    pub min_shot: f64,
    /// Score a hard cut must exceed
    #[arg(long, default_value_t = 0.30)]
    pub cut_threshold: f64,
    /// Skip dissolve detection
    #[arg(long)]
    pub no_dissolve: bool,
    /// Print the JSON schema of breakdown.json and exit
    #[arg(long)]
    pub schema: bool,
}

/// Exit 0 on success, 1 when the video cannot be read, 2 when the output cannot be written.
pub fn run(args: BreakdownArgs) -> i32 {
    if args.schema {
        println!("{}", serde_json::to_string_pretty(&schema()).expect("schema serializes"));
        return 0;
    }
    if !(args.min_shot.is_finite() && args.min_shot >= 0.0 && args.cut_threshold.is_finite()) {
        eprintln!("--min-shot and --cut-threshold must be finite, and --min-shot non-negative");
        return 1;
    }
    let video = args.video.expect("clap requires the video");
    let options = ShotOptions { min_shot_s: args.min_shot, cut_threshold: args.cut_threshold, dissolve: !args.no_dissolve };
    let (doc, timings) = match analyze(&video, &options) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("breakdown: {e}");
            return 1;
        }
    };
    let json = serde_json::to_string_pretty(&doc).expect("breakdown serializes");
    let Some(dir) = args.output else {
        println!("{json}");
        return 0;
    };
    let path = dir.join("breakdown.json");
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, json)) {
        eprintln!("breakdown: writing {}: {e}", path.display());
        return 2;
    }
    let summary = serde_json::json!({
        "output": path.display().to_string(),
        "shots": doc.shots.len(),
        "frames": doc.source.frames,
        "hints": doc.provenance.hints,
        "seconds": timings,
    });
    println!("{summary}");
    0
}
