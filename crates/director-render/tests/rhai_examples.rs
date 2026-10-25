//! Every Rhai example paints the same frames from its scene document as from
//! its live `Director`.

use director_core::scene_export::export_scene;
use director_core::scene_import::ImportRegistry;
use director_core::scripting::register_rhai_api_with_compositions;
use director_core::scripting::MovieHandle;
use director_core::video_wrapper::RenderMode;
use director_core::{AssetLoader, Director};
use director_render::{Frame, FrameSession, Renderer};
use rayon::prelude::*;
use rhai::{Dynamic, Engine};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod common;

const TIMES: [f64; 3] = [0.0, 0.75, 2.0];

/// Resolves example asset paths from the workspace root.
struct RepoLoader;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

impl RepoLoader {
    fn resolve(path: &str) -> Option<PathBuf> {
        let root = repo();
        [
            PathBuf::from(path),
            root.join(path),
            root.join("assets").join(path),
            root.join("crates/director-core/assets").join(path),
        ]
        .into_iter()
        .find(|candidate| candidate.is_file())
    }
}

impl AssetLoader for RepoLoader {
    fn load_bytes(&self, path: &str) -> anyhow::Result<Vec<u8>> {
        let file = Self::resolve(path).ok_or_else(|| anyhow::anyhow!("asset not found: {path}"))?;
        Ok(std::fs::read(file)?)
    }

    fn media_path(&self, path: &str) -> Option<PathBuf> {
        Self::resolve(path)
    }

    fn load_font_fallback(&self) -> Option<Vec<u8>> {
        self.load_bytes("fonts/emoji.ttf").ok()
    }
}

fn scripts(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            scripts(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rhai") {
            found.push(path);
        }
    }
}

fn take(movie: MovieHandle) -> Director {
    let placeholder = Director::new(1, 1, 30.0, Arc::new(RepoLoader), RenderMode::Export, None);
    std::mem::replace(&mut *movie.director.lock().unwrap(), placeholder)
}

/// The movies one evaluation of `script` builds: its returned movie, or each
/// registered composition with default props.
fn movies(script: &str, label: &str) -> Vec<(String, Director)> {
    let mut engine = Engine::new();
    let registry = register_rhai_api_with_compositions(&mut engine, Arc::new(RepoLoader), RenderMode::Export);
    let ast = engine.compile(script).unwrap_or_else(|e| panic!("{label}: {e}"));
    let result = engine.eval_ast::<Dynamic>(&ast).unwrap_or_else(|e| panic!("{label}: {e}"));
    if registry.is_empty() {
        let movie = result.try_cast::<MovieHandle>().unwrap_or_else(|| panic!("{label}: no movie"));
        return vec![(label.to_string(), take(movie))];
    }
    registry
        .entries()
        .into_iter()
        .map(|entry| {
            let name = format!("{label}#{}", entry.name);
            let movie = registry.build(&engine, &ast, Some(&entry.name), None).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, take(movie))
        })
        .collect()
}

fn output_size(director: &Director) -> (i32, i32) {
    (
        (director.width as f32 * director.output_scale_x).round() as i32,
        (director.height as f32 * director.output_scale_y).round() as i32,
    )
}

fn compare(name: &str, path: &str, expected: &[Frame], actual: impl Iterator<Item = Frame>) -> Vec<String> {
    expected
        .iter()
        .zip(actual)
        .zip(TIMES)
        .filter_map(|((expected, actual), time)| {
            let fraction = common::differing_fraction(expected, &actual);
            (fraction > common::MAX_FRACTION).then(|| format!("{name} {path} at {time}s: {fraction:.6} of pixels differ"))
        })
        .collect()
}

/// Paint one movie on a live `FrameSession` and from its document. Returns
/// failures and whether the document import was skipped.
fn check(name: &str, live: Director) -> (Vec<String>, bool) {
    let size = output_size(&live);
    let document = export_scene(&live).unwrap_or_else(|e| panic!("{name}: export failed: {e}"));
    let mut session = FrameSession::new(live, size).unwrap();
    let expected: Vec<Frame> =
        TIMES.iter().map(|&time| session.render(time).unwrap_or_else(|e| panic!("{name}: {e}"))).collect();

    let renderer = Renderer::new(Arc::new(document), Arc::new(RepoLoader))
        .with_registry(ImportRegistry::core())
        .with_render_mode(session.director().render_mode);
    match renderer.session() {
        Ok(mut imported) => {
            let frames = TIMES.iter().map(|&time| imported.render(time).unwrap_or_else(|e| panic!("{name}: {e}")));
            (compare(name, "document", &expected, frames), false)
        }
        Err(director_render::RenderError::Import(error)) if error.is_unsupported() => {
            eprintln!("{name}: document skipped: {error}");
            (Vec::new(), true)
        }
        Err(error) => panic!("{name}: document import failed: {error}"),
    }
}

#[test]
fn every_rhai_example_paints_like_its_document() {
    let root = repo();
    let mut found = Vec::new();
    scripts(&root.join("examples"), &mut found);
    found.sort();
    let results: Vec<(Vec<String>, usize)> = found
        .par_iter()
        .map(|path| {
            let label = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            let script = std::fs::read_to_string(path).unwrap();
            let mut failures = Vec::new();
            let mut skipped = 0;
            for (name, live) in movies(&script, &label) {
                let (found, document_skipped) = check(&name, live);
                failures.extend(found);
                skipped += document_skipped as usize;
            }
            (failures, skipped)
        })
        .collect();
    let failures: Vec<String> = results.iter().flat_map(|(failures, _)| failures.clone()).collect();
    let skipped: usize = results.iter().map(|(_, skipped)| skipped).sum();
    assert!(found.len() >= 50, "found {} Rhai examples", found.len());
    eprintln!("{} examples, {skipped} movies skipped the document import", found.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
