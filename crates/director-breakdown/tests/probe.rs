use director_breakdown::{media::VideoInfo, provenance};

use crate::ffmpeg;

fn tagged(name: &str, tags: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join(name);
    let pairs: Vec<String> = tags.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut args = vec!["-f", "lavfi", "-i", "color=c=gray:size=64x64:rate=10:duration=0.5", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-movflags", "use_metadata_tags"];
    for p in &pairs {
        args.extend(["-metadata", p.as_str()]);
    }
    ffmpeg(&args, &out);
    (dir, out)
}

#[test]
fn reads_tiktok_and_youtube_signatures() {
    let (_dir, path) = tagged("tt.mp4", &[("comment", "vid:v24044gl0000db505svog65l98dgm1f0"), ("aigc_info", r#"{"aigc_label_type":0}"#)]);
    let p = provenance(&VideoInfo::probe(&path).unwrap());
    assert_eq!(p.hints["platform"], vec!["TikTok/Douyin".to_string()], "{:?}", p.hints);
    assert_eq!(p.hints["tool"], vec!["FFmpeg".to_string()]);
    let aigc = p.aigc.expect("aigc_info parsed");
    assert_eq!((aigc.scheme.as_str(), aigc.label.as_str()), ("tiktok", "0"));
    assert!(!p.hints.contains_key("generator"), "{:?}", p.hints);
    assert!(p.encoder.starts_with("Lavf"));
}

#[test]
fn reads_generator_and_camera_tags() {
    let (_dir, path) = tagged("gen.mp4", &[("comment", "Generated with Remotion"), ("make", "Apple"), ("model", "iPhone 15 Pro")]);
    let p = provenance(&VideoInfo::probe(&path).unwrap());
    assert_eq!(p.hints["renderer"], vec!["Remotion".to_string()], "{:?}", p.hints);
    assert!(p.hints["camera"].iter().any(|c| c == "model=iPhone 15 Pro"), "{:?}", p.hints);
    assert!(p.evidence.iter().any(|e| e.kind == "camera_tags" && e.weight == "strong"));
}

#[test]
fn absence_proves_nothing() {
    let (_dir, path) = tagged("plain.mp4", &[]);
    let p = provenance(&VideoInfo::probe(&path).unwrap());
    assert!(p.aigc.is_none());
    assert!(!p.hints.contains_key("generator") && !p.hints.contains_key("platform"), "{:?}", p.hints);
    assert!(p.container.contains("mp4"));
}
