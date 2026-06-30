// Template Visual Regression Tests — Script-Driven Fixtures
//
// All `.rhai` files in `fixtures/templates/` are automatically discovered,
// evaluated through the real Rhai scripting API, rendered, and compared
// against snapshots.
//
// To add a test for a new template, drop a `.rhai` fixture into
// `fixtures/templates/` and re-run with `UPDATE_SNAPSHOTS=1`.
//
// Fixtures may include temporal capture annotations:
//   // @snapshot: 0.0, 2.5, 5.0
// If omitted, a single snapshot at t=0.0 is captured.

use std::env;
use std::path::PathBuf;

#[test]
fn all_template_fixtures() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let fixtures_dir = PathBuf::from(&manifest_dir).join("tests/visual/fixtures/templates");

    let mut fixture_count = 0;
    let mut failures = Vec::new();

    for entry in std::fs::read_dir(&fixtures_dir).expect("Failed to read fixtures dir") {
        let entry = entry.expect("Failed to read dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rhai") {
            let rel_path = path
                .strip_prefix(&manifest_dir)
                .expect("fixture must be inside manifest dir");
            match crate::visual::run_rhai_visual_fixture(rel_path.to_str().unwrap(), "templates") {
                Ok(()) => fixture_count += 1,
                Err(msg) => failures.push(msg),
            }
        }
    }

    assert!(
        fixture_count > 0,
        "No .rhai fixtures found in {}",
        fixtures_dir.display()
    );

    if !failures.is_empty() {
        let mut msg = format!(
            "{} of {} template fixture(s) failed:\n\n",
            failures.len(),
            fixture_count + failures.len()
        );
        for f in &failures {
            msg.push_str(&format!("  - {}\n", f));
        }
        panic!("{}", msg);
    }

    println!("Verified {} template fixture(s)", fixture_count);
}
