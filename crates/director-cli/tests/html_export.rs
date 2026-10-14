use std::process::Command;

#[test]
fn native_html_export_keeps_audio_and_video_duration_aligned() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("timing.html");
    let output = temp.path().join("timing.mp4");
    std::fs::write(&input, "<div data-composition-id='timing' data-no-timeline data-width='32' data-height='32' data-fps='30' data-duration='0.6' style='background:red'></div>").unwrap();
    let rendered = Command::new(env!("CARGO_BIN_EXE_director-engine"))
        .args(["html", "render"])
        .arg(input)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run native exporter");
    assert!(
        rendered.status.success(),
        "{}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    let probe = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ])
        .arg(&output)
        .output()
        .expect("ffprobe must be installed for export verification");
    assert!(
        probe.status.success(),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = meta["streams"].as_array().unwrap();
    let video = streams
        .iter()
        .find(|stream| stream["codec_type"] == "video")
        .unwrap();
    assert_eq!(video["nb_read_frames"], "18");
    for kind in ["video", "audio"] {
        let stream = streams
            .iter()
            .find(|stream| stream["codec_type"] == kind)
            .unwrap();
        let duration: f64 = stream["duration"].as_str().unwrap().parse().unwrap();
        assert!(
            (duration - 0.6).abs() < 0.05,
            "{kind} duration {duration}, expected 0.6"
        );
    }
    let duration: f64 = meta["format"]["duration"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        (duration - 0.6).abs() < 0.05,
        "MP4 duration {duration}, expected 0.6"
    );
}
