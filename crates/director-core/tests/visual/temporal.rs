// Temporal Snapshot Visual Regression Tests
//
// Rhai fixtures in `fixtures/temporal/` are evaluated and captured at
// multiple times via `// @snapshot: t1, t2, ...` annotations.
//
// These differ from `templates.rs` fixtures in that they intentionally
// contain animations so that each snapshot time produces a visibly
// distinct frame.

use std::env;
use std::path::PathBuf;

#[test]
fn all_temporal_fixtures() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let fixtures_dir = PathBuf::from(&manifest_dir).join("tests/visual/fixtures/temporal");

    let mut fixture_count = 0;
    let mut failures = Vec::new();

    for entry in std::fs::read_dir(&fixtures_dir).expect("Failed to read fixtures dir") {
        let entry = entry.expect("Failed to read dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rhai") {
            let rel_path = path
                .strip_prefix(&manifest_dir)
                .expect("fixture must be inside manifest dir");
            match crate::visual::run_rhai_visual_fixture(rel_path.to_str().unwrap(), "temporal") {
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
            "{} of {} temporal fixture(s) failed:\n\n",
            failures.len(),
            fixture_count + failures.len()
        );
        for f in &failures {
            msg.push_str(&format!("  - {}\n", f));
        }
        panic!("{}", msg);
    }

    println!("Verified {} temporal fixture(s)", fixture_count);
}
