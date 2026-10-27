use crate::batch;

use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_director-engine"))
        .args(args)
        .output()
        .unwrap()
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn rejected_repair_does_not_write() {
    let dir = tempfile::tempdir().unwrap();
    let html = dir.path().join("scene.html");
    std::fs::write(
        &html,
        "<style>body{margin:0}#box{width:32px;height:32px;background:red}</style><main id='stage' data-composition-id='main' data-width='32' data-height='32' data-duration='.6'><div id='box'></div></main>",
    )
    .unwrap();
    let project = dir.path().join("scene.dirproj");
    let packed = cli(&["project", "pack", path(&html), "-o", path(&project)]);
    assert!(
        packed.status.success(),
        "pack: {}",
        String::from_utf8_lossy(&packed.stderr)
    );
    let before = std::fs::read(&project).unwrap();
    let patch = dir.path().join("patch.json");
    std::fs::write(&patch, r#"{"missing":{"text":"oops"}}"#).unwrap();
    let rejected = dir.path().join("rejected.dirproj");
    let repaired = cli(&[
        "project",
        "repair",
        path(&project),
        "--patch",
        path(&patch),
        "-o",
        path(&rejected),
    ]);
    assert!(
        !repaired.status.success(),
        "repair should fail: {}",
        String::from_utf8_lossy(&repaired.stdout)
    );
    assert!(
        String::from_utf8_lossy(&repaired.stderr).contains("missing"),
        "stderr: {}",
        String::from_utf8_lossy(&repaired.stderr)
    );
    assert!(
        repaired.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&repaired.stdout)
    );
    assert_eq!(std::fs::read(&project).unwrap(), before);
    assert!(!rejected.exists());
}

#[test]
fn batch_report_with_one_failure_and_one_success_is_nonzero() {
    let report = batch::batch_report(&[
        batch::BatchItem {
            name: "good.rhai".into(),
            output: "out/good.mp4".into(),
            error: None,
            exit_code: 0,
        },
        batch::BatchItem {
            name: "bad.rhai".into(),
            output: "out/bad.mp4".into(),
            error: Some("script error".into()),
            exit_code: 1,
        },
    ]);
    assert_ne!(report.exit_code, 0);
    assert!(
        report
            .text
            .lines()
            .any(|line| line.starts_with("ok ") && line.contains("good.rhai")),
        "{}",
        report.text
    );
    assert!(
        report
            .text
            .lines()
            .any(|line| line.starts_with("fail ") && line.contains("bad.rhai")),
        "{}",
        report.text
    );
    assert_eq!(report.text.lines().count(), 2, "{}", report.text);
}

#[test]
fn batch_report_json_matches_text_exit_code_and_items() {
    let items = [
        batch::BatchItem {
            name: "good.rhai".into(),
            output: "out\\good.mp4".into(),
            error: None,
            exit_code: 0,
        },
        batch::BatchItem {
            name: "bad \"shot\".rhai".into(),
            output: "out/bad.mp4".into(),
            error: Some("script\nerror".into()),
            exit_code: 1,
        },
    ];
    let text = batch::batch_report(&items);
    let json_report = batch::batch_report_json(&items);
    assert_eq!(json_report.exit_code, text.exit_code);
    assert_ne!(json_report.exit_code, 0);
    assert_eq!(
        text.text,
        "ok good.rhai out\\good.mp4\nfail bad \"shot\".rhai script error\n"
    );
    assert_eq!(
        json_report.json,
        r#"{"exit_code":1,"items":[{"input":"good.rhai","output":"out\\good.mp4","status":"ok","error":null,"exit_code":0},{"input":"bad \"shot\".rhai","output":"out/bad.mp4","status":"fail","error":"script\nerror","exit_code":1}]}"#
    );
}

#[test]
fn batch_report_json_error_code_zero_matches_text_exit() {
    let errored = [batch::BatchItem {
        name: "bad.rhai".into(),
        output: "out/bad.mp4".into(),
        error: Some("x".into()),
        exit_code: 0,
    }];
    let text = batch::batch_report(&errored);
    let json_report = batch::batch_report_json(&errored);
    assert_eq!(text.exit_code, 1);
    assert_eq!(json_report.exit_code, text.exit_code);
    assert_eq!(
        json_report.json,
        r#"{"exit_code":1,"items":[{"input":"bad.rhai","output":"out/bad.mp4","status":"fail","error":"x","exit_code":0}]}"#
    );
    assert_eq!(text.text, "fail bad.rhai x\n");

    let empty: [batch::BatchItem; 0] = [];
    let empty_text = batch::batch_report(&empty);
    let empty_json = batch::batch_report_json(&empty);
    assert_eq!(empty_json.exit_code, empty_text.exit_code);
    assert_eq!(empty_json.exit_code, 0);
    assert_eq!(empty_json.json, r#"{"exit_code":0,"items":[]}"#);
    assert_eq!(empty_text.text, "");
}

#[test]
fn batch_plan_file_names_are_stable() {
    let inputs = vec![
        PathBuf::from("shots/good.rhai"),
        PathBuf::from("shots/bad.rhai"),
        PathBuf::from("other/good.html"),
    ];
    let first = batch::plan_batch(&inputs, Path::new("out"));
    let second = batch::plan_batch(&inputs, Path::new("out"));
    let names: Vec<_> = first.iter().map(|item| item.output.clone()).collect();
    assert_eq!(
        names,
        vec![
            PathBuf::from("out").join("good.mp4"),
            PathBuf::from("out").join("bad.mp4"),
            PathBuf::from("out").join("good-2.mp4"),
        ]
    );
    assert_eq!(
        names,
        second
            .iter()
            .map(|item| item.output.clone())
            .collect::<Vec<_>>()
    );
}
