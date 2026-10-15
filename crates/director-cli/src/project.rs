use crate::input::Input;
use clap::{Args, Subcommand};
use director_html::{ElementEdit, NativeProject};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Args)]
pub struct ProjectArgs {
    #[command(subcommand)]
    command: ProjectCommand,
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    /// Bundle HTML/capture input and its fonts/media into a portable native project
    Pack {
        #[command(flatten)]
        input: Input,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Apply an element-ID keyed JSON patch, validate, then atomically save
    Edit {
        input: PathBuf,
        #[arg(long)]
        patch: PathBuf,
        /// Default: replace the input project after validation
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

pub fn run(args: ProjectArgs) {
    if let Err(error) = execute(args.command) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn execute(command: ProjectCommand) -> Result<(), String> {
    match command {
        ProjectCommand::Pack { input, output } => {
            if input.props.is_some() {
                return Err(
                    "project pack accepts HTML/captures; use project edit for overrides".into(),
                );
            }
            let project = NativeProject::from_file(&input.input, input.html_options())
                .map_err(|e| e.to_string())?;
            project.save(&output).map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::json!({"output":output,"format":project.format,"asset_references":project.assets.len(),"unique_blobs":project.blobs.len()})
            );
        }
        ProjectCommand::Edit {
            input,
            patch,
            output,
        } => {
            let bytes =
                std::fs::read(&patch).map_err(|e| format!("patch {}: {e}", patch.display()))?;
            let edits: BTreeMap<String, ElementEdit> =
                serde_json::from_slice(&bytes).map_err(|e| format!("patch JSON: {e}"))?;
            let mut project = NativeProject::load(&input).map_err(|e| e.to_string())?;
            project.apply_edits(edits).map_err(|e| e.to_string())?;
            let output = output.unwrap_or(input);
            project.save(&output).map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::json!({"output":output,"edited_elements":project.edits.keys().collect::<Vec<_>>()})
            );
        }
    }
    Ok(())
}
