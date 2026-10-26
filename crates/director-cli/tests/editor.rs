use director_host::SharedFrames;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn editor_answers_json_lines_and_paints_into_the_ring_file() {
    let dir = tempfile::tempdir().unwrap();
    let html = dir.path().join("scene.html");
    std::fs::write(
        &html,
        "<style>body{margin:0}#box{width:32px;height:32px;background:red}</style><main data-composition-id='main' data-width='32' data-height='32' data-duration='.6'><div id='box'></div></main>",
    )
    .unwrap();
    let ring = dir.path().join("frames.ring");
    let mut child = Command::new(env!("CARGO_BIN_EXE_director-engine"))
        .args(["editor", html.to_str().unwrap(), "--frames", ring.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut next = || serde_json::from_str::<serde_json::Value>(&lines.next().unwrap().unwrap()).unwrap();

    let opened = next();
    assert_eq!((opened["event"].as_str(), opened["width"].as_u64()), (Some("opened"), Some(32)));
    writeln!(stdin, r#"{{"op":"seek","time":0.1}}"#).unwrap();
    let frame = next();
    assert_eq!((frame["event"].as_str(), frame["generation"].as_u64()), (Some("frame"), Some(1)));
    let painted = SharedFrames::open(&ring).unwrap().latest().unwrap();
    assert_eq!(&painted.rgba[(16 * 32 + 16) * 4..][..4], &[255, 0, 0, 255]);

    writeln!(stdin, r#"{{"op":"warp"}}"#).unwrap();
    let error = next();
    assert_eq!((error["event"].as_str(), error["generation"].as_u64()), (Some("error"), Some(2)));
    assert_eq!(error["error"]["code"], "invalid_argument");
    writeln!(stdin, r#"{{"op":"patch","ops":[{{"op":"set","node":"box","path":"/body/hidden","value":true}}]}}"#).unwrap();
    assert_eq!(next()["event"], "patched");
    let repaint = next();
    assert_eq!((repaint["event"].as_str(), repaint["revision"].as_u64()), (Some("frame"), Some(1)));
    drop(stdin);
    assert_eq!(child.wait().unwrap().code(), Some(0));
}
