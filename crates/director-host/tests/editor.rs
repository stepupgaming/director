use director_host::{Command, Editor, EditorOptions, ErrorCode, Event, NodeSelector, PatchOp, Session, SharedFrames, Source};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

fn html_input(dir: &Path, background: &str) -> PathBuf {
    std::fs::write(
        dir.join("style.css"),
        format!("body{{margin:0}}#box{{width:32px;height:32px;background:{background}}}"),
    )
    .unwrap();
    let input = dir.join("scene.html");
    std::fs::write(&input, "<link rel='stylesheet' href='style.css'><main data-composition-id='main' data-width='32' data-height='32' data-duration='.6'><div id='box'></div></main>").unwrap();
    input
}

fn next(events: &Receiver<Event>) -> Event {
    events.recv_timeout(Duration::from_secs(60)).expect("editor event")
}

fn open(input: PathBuf, frames: Option<PathBuf>) -> (Editor, Receiver<Event>) {
    let (editor, events) = Editor::open(Source::new(input), EditorOptions { frames, setup: None }).unwrap();
    let Event::Opened { revision, width, height, capacity, .. } = next(&events) else {
        panic!("first event opens");
    };
    assert_eq!((revision, width, height), (0, 32, 32));
    assert!(capacity >= 32 * 32 * 4);
    (editor, events)
}

fn center(frames: &SharedFrames) -> [u8; 4] {
    let frame = frames.latest().expect("a painted frame");
    frame.rgba[(16 * 32 + 16) * 4..][..4].try_into().unwrap()
}

#[test]
fn seeks_paint_into_the_ring_and_edits_repaint_the_last_seek() {
    let dir = tempfile::tempdir().unwrap();
    let (mut editor, events) = open(html_input(dir.path(), "red"), None);
    let seek = editor.send(Command::Seek { time: 0.1 });
    let Event::Frame { generation, revision, slot, width, .. } = next(&events) else {
        panic!("seek paints");
    };
    assert_eq!((generation, revision, width), (seek, 0, 32));
    assert_eq!(editor.frames().latest().unwrap().slot, slot);
    assert_eq!(center(editor.frames()), [255, 0, 0, 255]);

    let patch = editor.send(Command::Patch {
        base_revision: Some(0),
        ops: vec![PatchOp::Set {
            node: NodeSelector::Name("box".into()),
            path: "/body/hidden".into(),
            value: json!(true),
        }],
    });
    assert!(matches!(next(&events), Event::Patched { generation, revision: 1 } if generation == patch));
    assert!(matches!(next(&events), Event::Frame { generation, revision: 1, .. } if generation == patch));
    assert_ne!(center(editor.frames()), [255, 0, 0, 255]);

    let stale = editor.send(Command::Patch {
        base_revision: Some(0),
        ops: Vec::new(),
    });
    let Event::Error { generation, error } = next(&events) else {
        panic!("stale patch fails");
    };
    assert_eq!((generation, error.code), (stale, ErrorCode::Conflict));
}

#[test]
fn a_newer_seek_cancels_queued_ones() {
    let dir = tempfile::tempdir().unwrap();
    let (mut editor, events) = open(html_input(dir.path(), "red"), None);
    let generations: Vec<u64> = (0..6).map(|step| editor.send(Command::Seek { time: step as f64 * 0.1 })).collect();
    let last = *generations.last().unwrap();
    for expected in &generations {
        match next(&events) {
            Event::Frame { generation, .. } => assert_eq!(generation, *expected),
            Event::Cancelled { generation } => assert!(generation == *expected && generation != last),
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(editor.frames().latest().unwrap().generation, last);
}

#[test]
fn another_process_maps_the_ring_file_and_source_edits_reload() {
    let dir = tempfile::tempdir().unwrap();
    let input = html_input(dir.path(), "red");
    let ring = dir.path().join("frames.ring");
    let (mut editor, events) = open(input, Some(ring.clone()));
    let reader = SharedFrames::open(&ring).unwrap();
    editor.send(Command::Seek { time: 0.2 });
    assert!(matches!(next(&events), Event::Frame { .. }));
    assert_eq!(center(&reader), [255, 0, 0, 255]);

    html_input(dir.path(), "blue");
    let outline = editor.send(Command::Outline);
    assert!(matches!(next(&events), Event::Reloaded { generation, revision: 1 } if generation == outline));
    assert!(matches!(next(&events), Event::Frame { revision: 1, .. }));
    assert_eq!(center(&reader), [0, 0, 255, 255]);
    let Event::Outline { outline, .. } = next(&events) else {
        panic!("outline follows the reload");
    };
    assert_eq!(outline.revision, 1);

    editor.send(Command::Inspect { time: 0.2 });
    let Event::Inspected { inspection, .. } = next(&events) else {
        panic!("inspect");
    };
    assert_eq!(inspection["revision"], 1);
    editor.send(Command::Seek { time: -1.0 });
    let Event::Error { error, .. } = next(&events) else {
        panic!("negative seek fails");
    };
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn audio_windows_match_the_export_mix() {
    let dir = tempfile::tempdir().unwrap();
    let input = html_input(dir.path(), "red");
    let wav = Session::open(&Source::new(input.clone())).unwrap().audio_wav().unwrap();
    let (mut editor, events) = open(input, None);
    editor.send(Command::Audio { start: 0.2, duration: 0.2 });
    let event = next(&events);
    let encoded = serde_json::to_value(&event).unwrap();
    assert_eq!(encoded["event"], "audio");
    assert!(encoded["samples_f32le"].is_string());
    let Event::Audio { start, sample_rate, samples, .. } = event else {
        panic!("audio window");
    };
    assert_eq!((start, sample_rate), (0.2, 48000));
    let floats: Vec<f32> = wav[44..].as_chunks::<4>().0.iter().map(|bytes| f32::from_le_bytes(*bytes)).collect();
    assert_eq!(samples, floats[6 * 1600 * 2..12 * 1600 * 2]);
    editor.send(Command::Audio { start: 0.0, duration: 0.0 });
    assert!(matches!(next(&events), Event::Error { error, .. } if error.code == ErrorCode::InvalidArgument));
}
