use director_host::{ErrorCode, ExportSettings, NodeSelector, OutlineNode, PatchOp, Session, Source};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

fn open() -> Session {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/html/native-motion.html");
    Session::open(&Source::new(path)).expect("open native-motion")
}

fn hide(name: &str) -> PatchOp {
    PatchOp::Set {
        node: NodeSelector::Name(name.into()),
        path: "/body/hidden".into(),
        value: json!(true),
    }
}

fn named<'a>(nodes: &'a [OutlineNode], name: &str) -> Option<&'a OutlineNode> {
    nodes
        .iter()
        .find_map(|node| (node.name.as_deref() == Some(name)).then_some(node).or_else(|| named(&node.children, name)))
}

#[test]
fn open_outlines_named_nodes_and_clips() {
    let session = open();
    assert_eq!(session.revision(), 0);
    assert!(session.names().contains_key("pulse"));
    let outline = session.outline();
    assert_eq!((outline.width, outline.height), (1920, 1080));
    assert!(!outline.clips.is_empty());
    assert!((outline.duration - 3.0).abs() < 1e-9);
    assert!(named(&outline.roots, "native-icon").is_some());
}

#[test]
fn accepted_patch_advances_revision_and_repaints() {
    let mut session = open();
    let before = session.render_frame(2.0).unwrap();
    assert_eq!(session.patch(0, &[hide("pulse")]).unwrap(), 1);
    let after = session.render_frame(2.0).unwrap();
    assert_eq!((after.width, after.height), (1920, 1080));
    assert_ne!(before.rgba, after.rgba);
    let inspection = session.inspect(2.0).unwrap();
    assert_eq!(inspection["revision"], 1);
    assert!(inspection["manifest"].is_object());
}

#[test]
fn stale_patch_conflicts() {
    let mut session = open();
    session.patch(0, &[hide("pulse")]).unwrap();
    let error = session.patch(0, &[hide("native-icon")]).unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(session.revision(), 1);
}

#[test]
fn rejected_patch_leaves_the_document_unchanged() {
    let mut session = open();
    let document = session.document().clone();
    let bad_type = PatchOp::Set {
        node: NodeSelector::Name("pulse".into()),
        path: "/z_index".into(),
        value: json!("high"),
    };
    let error = session.patch(0, &[hide("pulse"), bad_type]).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidPatch);
    assert!(error.message.starts_with("op 1:"), "{}", error.message);
    assert_eq!(session.revision(), 0);
    assert_eq!(**session.document(), *document);

    let identity = PatchOp::Set {
        node: NodeSelector::Name("pulse".into()),
        path: "/id".into(),
        value: json!({"index": 0, "generation": 0}),
    };
    assert_eq!(session.patch(0, &[identity]).unwrap_err().code, ErrorCode::InvalidPatch);
    let version = PatchOp::SetDocument {
        path: "/version".into(),
        value: json!(1),
    };
    assert_eq!(session.patch(0, &[version]).unwrap_err().code, ErrorCode::InvalidPatch);
    assert_eq!(session.patch(0, &[hide("missing")]).unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn remove_drops_the_subtree_and_its_names() {
    let mut session = open();
    let node = PatchOp::Remove {
        node: NodeSelector::Name("pulse".into()),
    };
    session.patch(0, &[node]).unwrap();
    assert!(!session.names().contains_key("pulse"));
    assert!(named(&session.outline().roots, "pulse").is_none());
    session.render_frame(1.0).unwrap();

    let root = session.outline().clips[0].scene_root;
    let error = session
        .patch(1, &[PatchOp::Remove {
            node: NodeSelector::Ref(root),
        }])
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidPatch);
}

#[test]
fn document_ops_retime_the_timeline() {
    let mut session = open();
    let shorten = PatchOp::SetDocument {
        path: "/timeline/tracks/0/clips/0/duration".into(),
        value: json!(1.5),
    };
    session.patch(0, &[shorten]).unwrap();
    assert!((session.outline().duration - 1.5).abs() < 1e-9);
}

#[test]
fn cancelled_export_writes_nothing() {
    let session = open();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cancelled.mp4");
    let mut settings = ExportSettings::default();
    settings.options.cancel = Some(Arc::new(AtomicBool::new(true)));
    let error = session.export(&path, &settings, None).unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert!(!path.exists());
}

#[test]
fn export_reports_progress_to_the_last_frame() {
    let mut session = open();
    let shorten = PatchOp::SetDocument {
        path: "/timeline/tracks/0/clips/0/duration".into(),
        value: json!(0.5),
    };
    session.patch(0, &[shorten]).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("draft.mp4");
    let encoded = Arc::new(AtomicUsize::new(0));
    let mut settings = ExportSettings {
        scale: 0.25,
        ..ExportSettings::default()
    };
    settings.options.draft = true;
    settings.options.mode = director_core::video_wrapper::EncoderMode::Software;
    let seen = encoded.clone();
    settings.options.progress = Some(Arc::new(move |done, total| {
        assert!(done <= total);
        seen.store(done, Ordering::Release);
    }));
    session.export(&path, &settings, None).unwrap();
    assert_eq!(encoded.load(Ordering::Acquire), 15);
    assert!(std::fs::metadata(&path).unwrap().len() > 0);
}

#[test]
fn errors_print_one_envelope() {
    let mut session = open();
    let error = session.render_frame(f64::NAN).unwrap_err();
    assert_eq!(
        error.envelope(),
        json!({"error": {"code": "invalid_argument", "message": "time must be finite and nonnegative"}})
    );
    let missing = Session::open(&Source::new(PathBuf::from("missing.html"))).err().unwrap();
    assert_eq!(missing.code, ErrorCode::Load);
}

fn html_input(dir: &std::path::Path, background: &str) -> PathBuf {
    std::fs::write(
        dir.join("style.css"),
        format!("body{{margin:0}}#box{{width:32px;height:32px;background:{background}}}"),
    )
    .unwrap();
    let input = dir.join("scene.html");
    std::fs::write(&input, "<link rel='stylesheet' href='style.css'><main data-composition-id='main' data-width='32' data-height='32' data-duration='.6'><div id='box'></div></main>").unwrap();
    input
}

#[test]
fn reload_follows_dependencies_and_keeps_the_document_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&Source::new(html_input(dir.path(), "red"))).unwrap();
    assert!(!session.source_changed());
    std::fs::write(dir.path().join("style.css"), "#box{perspective:0}").unwrap();
    assert!(session.source_changed());
    let document = session.document().clone();
    let error = session.reload().unwrap_err();
    assert_eq!(error.code, ErrorCode::Load);
    assert_eq!(session.revision(), 0);
    assert_eq!(**session.document(), *document);
    assert!(session.reload_error().unwrap().message.contains("perspective"));
    assert!(!session.source_changed());
    html_input(dir.path(), "blue");
    assert!(session.source_changed());
    assert_eq!(session.reload().unwrap(), 1);
    assert!(session.reload_error().is_none());
    let frame = session.render_frame(0.1).unwrap();
    assert_eq!(&frame.rgba[(16 * 32 + 16) * 4..][..4], &[0, 0, 255, 255]);
}

#[test]
fn audio_wav_is_cached_until_the_next_revision() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(&Source::new(html_input(dir.path(), "red"))).unwrap();
    let audio = session.audio_wav().unwrap();
    assert_eq!(&audio[..4], b"RIFF");
    assert_eq!(audio.len(), 44 + 18 * 1600 * 2 * 4);
    assert!(Arc::ptr_eq(&audio, &session.audio_wav().unwrap()));
    session.reload().unwrap();
    assert!(!Arc::ptr_eq(&audio, &session.audio_wav().unwrap()));
}

#[test]
fn project_edits_compile_before_they_apply_and_save_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let html = html_input(dir.path(), "red");
    let project = dir.path().join("scene.dirproj");
    director_html::NativeProject::from_file(&html, Source::new(html.clone()).html_options())
        .unwrap()
        .save(&project)
        .unwrap();
    let mut session = Session::open(&Source::new(project.clone())).unwrap();
    let edit = |id: &str| -> std::collections::BTreeMap<String, director_html::ElementEdit> {
        serde_json::from_value(json!({ id: {"styles": {"background-color": "blue"}} })).unwrap()
    };
    let error = session.edit_project(edit("missing")).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidPatch);
    assert!(error.message.contains("missing"), "{}", error.message);
    assert_eq!(session.revision(), 0);
    assert!(session.project().is_none());
    assert_eq!(session.edit_project(edit("box")).unwrap(), 1);
    let frame = session.render_frame(0.1).unwrap();
    assert_eq!(&frame.rgba[(16 * 32 + 16) * 4..][..4], &[0, 0, 255, 255]);
    let saved = dir.path().join("edited.dirproj");
    session.save_project(&saved).unwrap();
    let mut reopened = Session::open(&Source::new(saved)).unwrap();
    assert_eq!(reopened.render_frame(0.1).unwrap(), frame);
    let html_edit = Session::open(&Source::new(html)).unwrap().edit_project(edit("box")).unwrap_err();
    assert_eq!(html_edit.code, ErrorCode::InvalidArgument);
}

#[test]
fn diff_of_equal_inputs_is_empty() {
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let before = Session::open(&Source::new(html_input(one.path(), "red"))).unwrap();
    let after = Session::open(&Source::new(html_input(two.path(), "red"))).unwrap();
    let diff = director_host::diff(&before, &after);
    assert!(diff.equal, "{}", json!(diff));
    assert!(diff.patchable);
    assert!(diff.patch.is_empty());
}

#[test]
fn diff_patch_turns_before_into_after() {
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut before = Session::open(&Source::new(html_input(one.path(), "red"))).unwrap();
    let mut after = Session::open(&Source::new(html_input(two.path(), "blue"))).unwrap();
    let diff = director_host::diff(&before, &after);
    assert!(!diff.equal);
    assert!(diff.added_nodes.is_empty() && diff.removed_nodes.is_empty() && diff.moved_nodes.is_empty());
    let changed = diff.changed_nodes.iter().find(|node| node.name.as_deref() == Some("box"));
    assert!(changed.is_some_and(|node| !node.changes.is_empty()), "{}", json!(diff));
    assert!(diff.patchable);
    before.patch(0, &diff.patch).unwrap();
    assert_eq!(**before.document(), **after.document());
    assert_eq!(before.render_frame(0.1).unwrap(), after.render_frame(0.1).unwrap());
}

#[test]
fn diff_reports_added_and_removed_nodes_by_name() {
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let before = Session::open(&Source::new(html_input(one.path(), "red"))).unwrap();
    let input = html_input(two.path(), "red");
    std::fs::write(&input, "<link rel='stylesheet' href='style.css'><main data-composition-id='main' data-width='32' data-height='32' data-duration='.6'><div id='dot'></div></main>").unwrap();
    let after = Session::open(&Source::new(input)).unwrap();
    let diff = director_host::diff(&before, &after);
    let names = |nodes: &[director_host::DiffNode]| nodes.iter().filter_map(|node| node.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&diff.removed_nodes), ["box"], "{}", json!(diff));
    assert_eq!(names(&diff.added_nodes), ["dot"]);
    assert!(!diff.patchable);
    assert!(diff.patch.is_empty());
}
