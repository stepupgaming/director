use crate::input::Input;
use clap::{Args, Subcommand};
use director_core::video_wrapper::RenderMode;
use director_html::{ElementEdit, NativeProject};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

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
    /// Apply one element-ID keyed JSON patch, validate, save atomically, and print inspect JSON
    Repair {
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
        let _ = std::io::stderr().flush();
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
            let (project, saved) = validated_edit(&input, &patch, output)?;
            println!(
                "{}",
                serde_json::json!({"output":saved,"edited_elements":project.edits.keys().collect::<Vec<_>>()})
            );
        }
        ProjectCommand::Repair {
            input,
            patch,
            output,
        } => {
            let (_, saved) = validated_edit(&input, &patch, output)?;
            let inspection = Input::new(saved)
                .load(RenderMode::Export)
                .and_then(|loaded| loaded.inspect(0.0))?;
            println!("{inspection}");
        }
    }
    Ok(())
}

/// Same patch language as `project edit`. The saved file is replaced only after validation.
fn validated_edit(
    input: &Path,
    patch: &Path,
    output: Option<PathBuf>,
) -> Result<(NativeProject, PathBuf), String> {
    let bytes = std::fs::read(patch).map_err(|e| format!("patch {}: {e}", patch.display()))?;
    let edits: BTreeMap<String, ElementEdit> =
        serde_json::from_slice(&bytes).map_err(|e| format!("patch JSON: {e}"))?;
    let mut project = NativeProject::load(input).map_err(|e| e.to_string())?;
    project.apply_edits(edits).map_err(|e| e.to_string())?;
    let output = output.unwrap_or_else(|| input.to_path_buf());
    project.save(&output).map_err(|e| e.to_string())?;
    Ok((project, output))
}
