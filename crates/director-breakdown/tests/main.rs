//! One integration binary for `director-breakdown`.

mod probe;
mod shots;

use std::path::Path;
use std::process::Command;

/// Runs ffmpeg with `args` writing `out`, panicking with its stderr on failure.
pub fn ffmpeg(args: &[&str], out: &Path) {
    let output = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).arg(out).output().expect("ffmpeg runs");
    assert!(output.status.success(), "ffmpeg failed: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn every_test_file_is_a_module() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let source = std::fs::read_to_string(dir.join("main.rs")).unwrap();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "rs") && path.file_name().unwrap() != "main.rs" {
            let name = path.file_stem().unwrap().to_str().unwrap();
            assert!(source.contains(&format!("mod {name};")), "tests/{name}.rs is not declared in tests/main.rs");
        }
    }
}
