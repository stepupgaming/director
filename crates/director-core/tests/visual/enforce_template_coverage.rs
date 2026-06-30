// API Coverage Enforcer
//
// Guarantees that every template registered in the Rhai engine has a
// corresponding `.rhai` visual regression fixture in
// `tests/visual/fixtures/templates/`.
//
// When you add a new template, add its name to EXPECTED_TEMPLATES below.
// Adding a new template without a fixture will break CI.

use std::collections::HashSet;
use std::path::PathBuf;

/// Expected template names (without the `add_` prefix).
/// Must stay in sync with `crates/director-core/src/scripting/api/templates.rs`.
const EXPECTED_TEMPLATES: &[&str] = &[
    "title_card",
    "lower_third",
    "split_screen",
    "reaction_overlay",
    "bento_grid",
    "watermark",
    "subscribe_button",
    "progress_bar",
    "countdown_timer",
    "quote_card",
    "social_handle_overlay",
];

#[test]
fn every_template_has_visual_fixture() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let fixtures_dir = PathBuf::from(&manifest_dir).join("tests/visual/fixtures/templates");

    let expected: HashSet<String> = EXPECTED_TEMPLATES.iter().map(|s| s.to_string()).collect();

    // Discover fixture files
    let mut fixture_names = HashSet::new();
    if fixtures_dir.exists() {
        for entry in std::fs::read_dir(&fixtures_dir).expect("Failed to read fixtures dir") {
            let entry = entry.expect("Failed to read dir entry");
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("rhai") {
                let stem = path
                    .file_stem()
                    .expect("fixture must have a filename")
                    .to_string_lossy()
                    .to_string();
                fixture_names.insert(stem);
            }
        }
    }

    // Every expected template must have a matching fixture
    let missing: Vec<String> = expected
        .difference(&fixture_names)
        .cloned()
        .collect();

    // Every fixture must correspond to an expected template
    let orphaned: Vec<String> = fixture_names
        .difference(&expected)
        .cloned()
        .collect();

    if !missing.is_empty() || !orphaned.is_empty() {
        let mut msg = String::from("Template coverage mismatch!\n\n");
        if !missing.is_empty() {
            msg.push_str("Templates missing fixtures:\n");
            for m in &missing {
                msg.push_str(&format!(
                    "  - {} (needs tests/visual/fixtures/templates/{}.rhai)\n",
                    m, m
                ));
            }
            msg.push('\n');
        }
        if !orphaned.is_empty() {
            msg.push_str("Orphaned fixtures (no matching expected template):\n");
            for o in &orphaned {
                msg.push_str(&format!(
                    "  - tests/visual/fixtures/templates/{}.rhai\n",
                    o
                ));
            }
            msg.push('\n');
        }
        msg.push_str("Update EXPECTED_TEMPLATES in enforce_template_coverage.rs when adding/removing templates.");
        panic!("{}", msg);
    }

    println!(
        "Coverage OK: {} expected templates, {} fixture files",
        expected.len(),
        fixture_names.len()
    );
}
