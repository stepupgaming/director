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
