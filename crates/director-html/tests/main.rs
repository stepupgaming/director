//! Every integration test of this crate, built as one test binary so they
//! link Skia once. Each `tests/<name>.rs` is a module below, and
//! `every_test_file_is_a_module` fails when a file is left out. One file's
//! tests: `cargo test -p director-html --release --test integration <name>::`.

mod animation_css;
mod authoring_slices;
mod background_layers;
mod background_position;
mod beats;
mod border_styles;
mod clip_shapes;
mod compatibility;
mod css_media;
mod css_nesting;
mod depth_nested;
mod depth_sort;
mod ellipse_clip;
mod filter_fns;
mod fixed_position;
mod font_shorthand;
mod gradient_calc;
mod gradient_hints;
mod gradient_tiles;
mod grid_lines;
mod grid_nested_named_lines;
mod gsap;
mod line_clamp;
mod media;
mod mixed_calc;
mod motion_blur;
mod motion_policy;
mod object_position_css;
mod padded_text;
mod paint_gaps;
mod project;
mod rewind;
mod rich_text;
mod scene_document;
mod shader;
mod shrink_wrap;
mod svg_markers;
mod svg_presentation;
mod tabular_nums;
mod text_indent;
mod text_stroke;
mod timeline;
mod transform_angles;
mod underline_offset;
mod white_space_more;

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
