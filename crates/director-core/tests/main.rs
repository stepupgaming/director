//! Every integration test of this crate, built as one test binary so they
//! link Skia once. Each `tests/<name>.rs` is a module below, and
//! `every_test_file_is_a_module` fails when a file is left out. One file's
//! tests: `cargo test -p director-core --release --test integration <name>::`.

pub mod visual;

mod active_speaker_camera_api;
mod animation_eval;
mod api_showcase;
mod audio;
mod audio_api;
mod blend_mode_loud;
mod compiler;
mod composition;
mod determinism_media;
mod effect_uv_probe;
mod examples;
mod fail_loud;
mod fail_loud_catalog;
mod identity_mutation;
mod inspect_api;
mod layout;
mod manifest_api;
mod masking;
mod object_position;
mod pipeline_equivalence;
mod project_roundtrip;
mod quick_wins;
mod scene_document;
mod smart_caption;
mod smart_caption_api;
mod static_reuse;
mod style_policy_api;
mod time_timeline;
mod transforms;
mod transitions;
mod typography;
mod video;
mod visual_suite;

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
