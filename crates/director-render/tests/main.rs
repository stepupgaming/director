//! Every integration test of this crate, built as one test binary so they
//! link Skia once. Each `tests/<name>.rs` is a module below, and
//! `every_test_file_is_a_module` fails when a file is left out. One file's
//! tests: `cargo test -p director-render --release --test integration <name>::`.

mod common;

mod core_nodes;
mod html_examples;
mod incremental_render;
mod motion_blur;
mod rhai_examples;

#[test]
fn every_test_file_is_a_module() {
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let main = std::fs::read_to_string(tests.join("main.rs")).unwrap();
    for entry in std::fs::read_dir(&tests).unwrap() {
        let path = entry.unwrap().path();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if path.extension().is_some_and(|ext| ext == "rs") && stem != "main" {
            let declared = main.lines().any(|line| line == format!("mod {stem};"));
            assert!(declared, "add `mod {stem};` to tests/main.rs");
        }
    }
}
