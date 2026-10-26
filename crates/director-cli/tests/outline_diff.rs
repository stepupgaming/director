use std::path::{Path, PathBuf};
use std::process::Command;

fn cli(args: &[&str]) -> (i32, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_director-engine")).args(args).output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|error| panic!("{error}: {stdout} {}", String::from_utf8_lossy(&output.stderr)));
    (output.status.code().unwrap(), json)
}

fn scene(dir: &Path, background: &str) -> PathBuf {
    let html = dir.join("scene.html");
    std::fs::write(
        &html,
        format!("<style>body{{margin:0}}#box{{width:32px;height:32px;background:{background}}}</style><main data-composition-id='main' data-width='32' data-height='32' data-duration='.6'><div id='box'></div></main>"),
    )
    .unwrap();
    html
}

#[test]
fn outline_prints_the_named_tree() {
    let dir = tempfile::tempdir().unwrap();
    let html = scene(dir.path(), "red");
    let (code, outline) = cli(&["outline", html.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(outline["revision"], 0);
    assert_eq!((outline["width"].as_i64(), outline["height"].as_i64()), (Some(32), Some(32)));
    assert!(outline["clips"].as_array().is_some_and(|clips| !clips.is_empty()));
    assert!(outline.to_string().contains(r#""name":"box""#), "{outline}");
}

#[test]
fn diff_prints_the_patch_between_inputs() {
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (before, after) = (scene(one.path(), "red"), scene(two.path(), "blue"));
    let (code, diff) = cli(&["diff", before.to_str().unwrap(), after.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert_eq!(diff["equal"], false);
    assert_eq!(diff["patchable"], true);
    assert_eq!(diff["changed_nodes"][0]["name"], "box", "{diff}");
    assert!(!diff["patch"].as_array().unwrap().is_empty());
}

#[test]
fn load_failures_print_the_error_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.html");
    let (code, error) = cli(&["outline", missing.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert_eq!(error["error"]["code"], "load");
    assert!(error["error"]["message"].is_string());
    let html = scene(dir.path(), "red");
    let (code, error) = cli(&["diff", html.to_str().unwrap(), missing.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert_eq!(error["error"]["code"], "load");
}
