use std::{
    path::Path,
    process::{Command, Output},
};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_director-engine"))
        .args(args)
        .output()
        .unwrap()
}
fn success(args: &[&str]) -> Output {
    let result = cli(args);
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    result
}
fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn headless_pack_inspect_edit_still_and_export_share_one_native_input() {
    let original = tempfile::tempdir().unwrap();
    let outputs = tempfile::tempdir().unwrap();
    let html = original.path().join("scene.html");
    std::fs::write(&html,"<style>body{margin:0}#box{width:32px;height:32px;background:red}</style><main id='stage' data-composition-id='main' data-width='32' data-height='32' data-duration='.6'><div id='box'></div></main>").unwrap();
    let project = outputs.path().join("scene.dirproj");
    success(&["project", "pack", path(&html), "-o", path(&project)]);
    original.close().unwrap();
    let inspection = success(&["inspect", path(&project), "-t", ".2"]);
    let inspected: serde_json::Value = serde_json::from_slice(&inspection.stdout).unwrap();
    assert_eq!(inspected["manifest"]["width"], 32);
    assert!(inspected["element_ids"].get("box").is_some());
    let generic = outputs.path().join("generic.png");
    let html_alias = outputs.path().join("html.png");
    success(&["still", path(&project), "-t", ".2", "-o", path(&generic)]);
    success(&[
        "html",
        "still",
        path(&project),
        "-t",
        ".2",
        "-o",
        path(&html_alias),
    ]);
    assert_eq!(
        std::fs::read(&generic).unwrap(),
        std::fs::read(&html_alias).unwrap()
    );
    let patch = outputs.path().join("patch.json");
    std::fs::write(&patch, r#"{"box":{"styles":{"background-color":"blue"}}}"#).unwrap();
    success(&["project", "edit", path(&project), "--patch", path(&patch)]);
    success(&["still", path(&project), "-o", path(&generic)]);
    assert_eq!(
        image::open(&generic)
            .unwrap()
            .to_rgba8()
            .get_pixel(16, 16)
            .0,
        [0, 0, 255, 255]
    );
    let accepted = std::fs::read(&project).unwrap();
    std::fs::write(&patch, r#"{"missing":{"text":"oops"}}"#).unwrap();
    assert_eq!(
        cli(&["project", "edit", path(&project), "--patch", path(&patch)])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(std::fs::read(&project).unwrap(), accepted);
    let video = outputs.path().join("scene.mp4");
    success(&["render", path(&project), "-o", path(&video)]);
    let probed = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
            path(&video),
        ])
        .output()
        .expect("ffprobe required");
    assert!(probed.status.success());
    let metadata: serde_json::Value = serde_json::from_slice(&probed.stdout).unwrap();
    for stream in metadata["streams"].as_array().unwrap() {
        let duration = stream["duration"].as_str().unwrap().parse::<f64>().unwrap();
        assert!((duration - 0.6).abs() < 0.04);
        if stream["codec_type"] == "video" {
            assert_eq!(stream["nb_read_frames"], "18");
        }
    }
}
