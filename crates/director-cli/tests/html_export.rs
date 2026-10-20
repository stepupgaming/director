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

#[test]
fn alpha_export_keeps_empty_pixels_transparent_and_alpha_straight() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("alpha.html");
    let output = temp.path().join("alpha.webm");
    std::fs::write(&input, "<style>html,body{margin:0;background:transparent}</style><div data-composition-id='alpha' data-no-timeline data-width='32' data-height='32' data-fps='30' data-duration='0.2' style='position:relative;width:32px;height:32px'><div style='position:absolute;left:0;top:0;width:16px;height:32px;background:rgba(255,0,0,0.5)'></div></div>").unwrap();
    let rendered = Command::new(env!("CARGO_BIN_EXE_director-engine"))
        .arg("render")
        .arg(&input)
        .args(["--width", "32", "--height", "32", "--fps", "30", "--duration", "0.2", "--alpha", "-o"])
        .arg(&output)
        .output()
        .expect("run native exporter");
    assert!(
        rendered.status.success(),
        "{}",
        String::from_utf8_lossy(&rendered.stderr)
    );
    // The native VP9 decoder drops the alpha plane; libvpx keeps it.
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-c:v", "libvpx-vp9", "-i"])
        .arg(&output)
        .args(["-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .output()
        .expect("ffmpeg must be installed for export verification");
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    let frame = decoded.stdout;
    assert_eq!(frame.len(), 32 * 32 * 4);
    let pixel = |x: usize, y: usize| -> [u8; 4] {
        let i = (y * 32 + x) * 4;
        [frame[i], frame[i + 1], frame[i + 2], frame[i + 3]]
    };
    let empty = pixel(28, 16);
    assert!(empty[3] <= 2, "empty pixel alpha {empty:?}, expected 0");
    let half = pixel(4, 16);
    assert!((half[3] as i32 - 128).abs() <= 8, "box alpha {half:?}, expected about 128");
    assert!(half[0] >= 240, "box red {half:?}, expected straight 255 red");
}
