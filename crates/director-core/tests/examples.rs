//! Example Script Validation Tests
//!
//! This module validates all example scripts in the `examples/` directory.
//! It ensures scripts:
//! - Parse without syntax errors
//! - Execute without runtime errors
//! - Return a valid MovieHandle **or** register named compositions whose
//!   builders produce MovieHandles with default props
//!
//! The walker runs every `examples/**/*.rhai` file. `SKIPPED_EXAMPLES` is the
//! only skip list, and each entry carries a reason. The list is empty: the
//! live HTTP demo is gone, and encoder probes only record a mode flag.
//!
//! Assets load through `WorkspaceAssetLoader`, rooted at the workspace, so
//! these tests do not call `set_current_dir`.

use director_core::{
    scripting::{register_rhai_api_with_compositions, MovieHandle},
    video_wrapper::RenderMode,
    AssetLoader,
};
use rhai::{Dynamic, Engine};
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Repo-relative scripts the walker does not execute.
/// Add a path and a reason when a script cannot run offline.
const SKIPPED_EXAMPLES: &[(&str, &str)] = &[];

/// Loads assets from the workspace root.
///
/// `DefaultAssetLoader` reads relative paths from the process working
/// directory. Cargo runs this file's tests on parallel threads, so changing
/// that directory races. This loader keeps resolution local to the test.
struct WorkspaceAssetLoader {
    root: PathBuf,
}

impl AssetLoader for WorkspaceAssetLoader {
    fn load_bytes(&self, path: &str) -> anyhow::Result<Vec<u8>> {
        let rel = Path::new(path);
        let candidates = if rel.is_absolute() {
            vec![rel.to_path_buf()]
        } else {
            vec![
                self.root.join(rel),
                self.root.join("assets").join(rel),
                self.root.join("crates/director-core/assets").join(rel),
            ]
        };
        for candidate in &candidates {
            if let Ok(bytes) = fs::read(candidate) {
                return Ok(bytes);
            }
        }
        anyhow::bail!("Asset not found: {path}");
    }

    fn load_font_fallback(&self) -> Option<Vec<u8>> {
        if let Ok(path) = std::env::var("DIRECTOR_EMOJI_FONT") {
            if let Ok(bytes) = fs::read(path) {
                return Some(bytes);
            }
        }
        self.load_bytes("fonts/emoji.ttf").ok()
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|parent| parent.parent())
        .expect("director-core lives two levels under the workspace root")
        .to_path_buf()
}

fn repo_relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn skip_reason(relative: &str) -> Option<&'static str> {
    SKIPPED_EXAMPLES
        .iter()
        .find(|(path, _)| *path == relative)
        .map(|(_, reason)| *reason)
}

/// Get all .rhai files from examples directory recursively.
fn collect_example_scripts(examples_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut scripts = Vec::new();
    collect_rhai_files(examples_dir, &mut scripts)?;
    scripts.sort();
    Ok(scripts)
}

fn collect_rhai_files(dir: &Path, scripts: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|e| format!("failed to read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read {}: {e}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rhai_files(&path, scripts)?;
        } else if path.extension().is_some_and(|ext| ext == "rhai") {
            scripts.push(path);
        }
    }
    Ok(())
}

/// Validate that all example scripts parse and execute successfully.
#[test]
fn validate_all_example_scripts() {
    let root = workspace_root();
    let examples_dir = root.join("examples");
    let scripts = collect_example_scripts(&examples_dir).unwrap_or_else(|e| panic!("{e}"));
    assert!(!scripts.is_empty(), "No example scripts found!");

    let mut passed = 0;
    let mut skipped = 0;
    let mut failed = Vec::new();

    for script_path in &scripts {
        let label = repo_relative(script_path, &root);
        if let Some(reason) = skip_reason(&label) {
            skipped += 1;
            println!("skip {label}: {reason}");
            continue;
        }
        match run_script_file(script_path, &label) {
            Ok(()) => passed += 1,
            Err(e) => failed.push(e),
        }
    }

    println!("\n=== Example Script Validation ===");
    println!("Passed: {passed}/{}", scripts.len());
    if skipped > 0 {
        println!("Skipped: {skipped}");
    }

    if !failed.is_empty() {
        panic!(
            "{} example script(s) failed:\n{}",
            failed.len(),
            failed.join("\n")
        );
    }

    println!("All {passed} example scripts validated successfully");
}

/// Test each example script individually for better error reporting.
#[test]
fn example_basics_hello_world() {
    validate_script("examples/basics/hello_world.rhai");
}

#[test]
fn example_basics_layout_flexbox() {
    validate_script("examples/basics/layout_flexbox.rhai");
}

#[test]
fn example_basics_animation() {
    validate_script("examples/basics/animation.rhai");
}

#[test]
fn example_basics_text() {
    validate_script("examples/basics/text.rhai");
}

#[test]
fn example_features_effects() {
    validate_script("examples/features/effects.rhai");
}

#[test]
fn example_features_shaders_gallery() {
    validate_script("examples/features/shaders_gallery.rhai");
}

#[test]
fn example_features_masking() {
    validate_script("examples/features/masking.rhai");
}

#[test]
fn example_features_transitions() {
    validate_script("examples/features/transitions.rhai");
}

#[test]
fn example_features_z_index() {
    validate_script("examples/features/z_index.rhai");
}

#[test]
fn example_features_image() {
    validate_script("examples/features/image.rhai");
}

#[test]
fn example_features_smart_caption() {
    validate_script("examples/features/smart_caption.rhai");
}

#[test]
fn example_features_verify_and_repair() {
    validate_script("examples/features/verify_and_repair.rhai");
}

#[test]
fn example_features_style_policy_check() {
    validate_script("examples/features/style_policy_check.rhai");
}

#[test]
fn example_features_compositions() {
    validate_script("examples/features/compositions.rhai");
}

#[test]
fn example_features_stdlib_motion() {
    validate_script("examples/features/stdlib_motion.rhai");
}

#[test]
fn example_recipes_faceless_short() {
    validate_script("examples/recipes/faceless_short.rhai");
}

#[test]
fn example_features_smart_lower_third() {
    validate_script("examples/features/smart_lower_third.rhai");
}

#[test]
fn example_recipes_kinetic_lower_third() {
    validate_script("examples/recipes/kinetic_lower_third.rhai");
}

#[test]
fn example_recipes_image_beats() {
    validate_script("examples/recipes/image_beats.rhai");
}

#[test]
fn example_recipes_stat_count() {
    validate_script("examples/recipes/stat_count.rhai");
}

#[test]
fn example_recipes_quote_card() {
    validate_script("examples/recipes/quote_card.rhai");
}

#[test]
fn example_recipes_list_reveal() {
    validate_script("examples/recipes/list_reveal.rhai");
}

#[test]
fn example_recipes_end_card() {
    validate_script("examples/recipes/end_card.rhai");
}

#[test]
fn example_recipes_chapter_card() {
    validate_script("examples/recipes/chapter_card.rhai");
}

#[test]
fn example_recipes_countdown() {
    validate_script("examples/recipes/countdown.rhai");
}

fn validate_script(relative_path: &str) {
    let script_path = workspace_root().join(relative_path);
    run_script_file(&script_path, relative_path).unwrap_or_else(|e| panic!("{e}"));
}

fn run_script_file(script_path: &Path, label: &str) -> Result<(), String> {
    let script =
        fs::read_to_string(script_path).map_err(|e| format!("{label}: failed to read: {e}"))?;
    let result = catch_unwind(AssertUnwindSafe(|| validate_script_source(&script, label)));
    match result {
        Ok(inner) => inner,
        Err(payload) => Err(format!("{label}: panic: {}", panic_message(payload))),
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic".to_string()
}

/// Accept either a legacy Movie return or a non-empty composition registry
/// (and exercise each builder with default props).
fn validate_script_source(script: &str, label: &str) -> Result<(), String> {
    let mut engine = Engine::new();
    let loader = Arc::new(WorkspaceAssetLoader {
        root: workspace_root(),
    });
    let registry = register_rhai_api_with_compositions(&mut engine, loader, RenderMode::Preview);

    let ast = engine
        .compile(script)
        .map_err(|e| format!("{label}: compile error: {e}"))?;
    let result = engine
        .eval_ast::<Dynamic>(&ast)
        .map_err(|e| format!("{label}: {e}"))?;

    if !registry.is_empty() {
        for entry in registry.entries() {
            registry
                .build(&engine, &ast, Some(&entry.name), None)
                .map_err(|e| format!("{label}: composition '{}': {e}", entry.name))?;
        }
        return Ok(());
    }

    if result.try_cast::<MovieHandle>().is_some() {
        return Ok(());
    }

    Err(format!(
        "{label}: script must return MovieHandle or register compositions"
    ))
}
