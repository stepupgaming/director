use std::process::{Command, Output};

fn asr(cache: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_director-engine")).arg("asr").arg("--cache").arg(cache).args(args).output().unwrap()
}

#[test]
#[cfg(feature = "asr")]
fn models_lists_every_model_and_graph_set() {
    let cache = tempfile::tempdir().unwrap();
    let out = asr(cache.path(), &["models"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let ids: Vec<_> = doc["models"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["bark-3.0", "bark-3.0-plus", "bark-3.0-clean", "bark-3.0-pro"]);
    assert!(doc["models"].as_array().unwrap().iter().all(|m| m["status"] == "missing" && m["license"] == "CC-BY-NC-4.0"));
    assert_eq!(doc["models"][3]["supported"], false);
    let graphs: Vec<_> = doc["graphs"].as_array().unwrap().iter().map(|g| g["id"].as_str().unwrap()).collect();
    assert_eq!(graphs, ["exact", "int8"]);
}

#[test]
#[cfg(feature = "asr")]
fn transcribe_without_a_model_names_the_fetch_command() {
    let cache = tempfile::tempdir().unwrap();
    let wav = cache.path().join("tone.wav");
    let made = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=1"])
        .arg(&wav)
        .status()
        .unwrap();
    assert!(made.success());
    let out = asr(cache.path(), &["transcribe", wav.to_str().unwrap(), "--model", "bark-3.0-clean", "--format", "srt"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("director-engine asr fetch bark-3.0-clean"), "{stderr}");

    let out = asr(cache.path(), &["transcribe", wav.to_str().unwrap(), "--model", "bark-3.0-pro"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("safetensors"));
}

#[test]
#[cfg(feature = "asr")]
fn fetch_from_a_wrong_file_installs_nothing() {
    let cache = tempfile::tempdir().unwrap();
    let fake = cache.path().join("model.bark");
    std::fs::write(&fake, b"not a model").unwrap();
    let out = asr(cache.path(), &["fetch", "bark-3.0", "--from", fake.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("holds 11 bytes"));
    assert!(!cache.path().join("bark-3.0").join("model.bark").exists());
}

#[test]
#[cfg(not(feature = "asr"))]
fn a_build_without_the_feature_says_how_to_get_it() {
    let cache = tempfile::tempdir().unwrap();
    let out = asr(cache.path(), &["models"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--features asr"));
}
