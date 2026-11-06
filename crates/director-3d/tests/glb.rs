//! GLB inspection: clips, duration and cameras read at compile time.

#[test]
fn the_fixture_reports_two_clips_and_one_camera() {
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/model.glb")).unwrap();
    let info = director_3d::inspect(&bytes).unwrap();
    assert_eq!((info.clips, info.cameras), (2, 1));
    assert!((info.duration - 61.0 / 30.0).abs() < 1e-3, "{}", info.duration);
}

#[test]
fn other_bytes_are_rejected() {
    assert!(director_3d::inspect(b"not a glb").unwrap_err().contains("glTF binary"));
    assert!(director_3d::Asset::environment(b"PNG".to_vec(), None).is_err());
}
