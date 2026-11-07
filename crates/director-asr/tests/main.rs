//! One integration binary for `director-asr`.

mod audio;
mod chunk;
mod store;
mod subtitle;

use std::path::Path;
use std::process::Command;

use director_asr::Word;

/// Runs ffmpeg with `args` writing `out`, panicking with its stderr on failure.
pub fn ffmpeg(args: &[&str], out: &Path) {
    let output = Command::new("ffmpeg").args(["-v", "error", "-y"]).args(args).arg(out).output().expect("ffmpeg runs");
    assert!(output.status.success(), "ffmpeg failed: {}", String::from_utf8_lossy(&output.stderr));
}

pub fn word(text: &str, start: f64, end: f64) -> Word {
    Word { text: text.to_string(), start, end, confidence: 1.0, disfluent: false }
}

pub fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

#[test]
fn every_test_file_is_a_module() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let source = std::fs::read_to_string(dir.join("main.rs")).unwrap();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "rs") && path.file_name().unwrap() != "main.rs" {
            let name = path.file_stem().unwrap().to_str().unwrap();
            assert!(source.contains(&format!("mod {name};")), "tests/{name}.rs is not declared in tests/main.rs");
        }
    }
}
