use std::process::Command;

#[test]
fn native_html_export_keeps_audio_and_video_duration_aligned() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("timing.html");
    let output = temp.path().join("timing.mp4");
    let tone = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/assets/tone.wav");
    std::fs::copy(tone, temp.path().join("tone.wav")).unwrap();
    std::fs::write(&input, "<div data-composition-id='timing' data-no-timeline data-width='32' data-height='32' data-fps='30' data-duration='0.6' style='background:red'><audio src='tone.wav' data-start='0' data-duration='0.6'></audio></div>").unwrap();
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
fn a_scene_without_audio_tracks_exports_no_audio_stream() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("silent.html");
    let output = temp.path().join("silent.mp4");
    std::fs::write(&input, "<div data-composition-id='silent' data-no-timeline data-width='32' data-height='32' data-fps='30' data-duration='0.2' style='background:red'></div>").unwrap();
    for args in [&["html", "render"][..], &["render"][..]] {
        let rendered = Command::new(env!("CARGO_BIN_EXE_director-engine"))
            .args(args)
            .arg(&input)
            .arg("-o")
            .arg(&output)
            .output()
            .expect("run native exporter");
        assert!(rendered.status.success(), "{}", String::from_utf8_lossy(&rendered.stderr));
        let probe = Command::new("ffprobe")
            .args(["-v", "error", "-show_entries", "stream=codec_type", "-of", "csv=p=0"])
            .arg(&output)
            .output()
            .expect("ffprobe must be installed for export verification");
        let kinds = String::from_utf8_lossy(&probe.stdout).split_whitespace().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(kinds, ["video"], "{args:?}");
        std::fs::remove_file(&output).unwrap();
    }
}

#[test]
fn export_tags_bt709_primaries_and_transfer() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("colour.html");
    let output = temp.path().join("colour.mp4");
    std::fs::write(&input, "<div data-composition-id='colour' data-no-timeline data-width='32' data-height='32' data-fps='30' data-duration='0.2' style='background:#2060a0'></div>").unwrap();
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
        .args(["-v", "error", "-select_streams", "v:0", "-show_streams", "-of", "json"])
        .arg(&output)
        .output()
        .expect("ffprobe must be installed for export verification");
    let meta: serde_json::Value = serde_json::from_slice(&probe.stdout).unwrap();
    let video = &meta["streams"][0];
    for key in ["color_space", "color_transfer", "color_primaries"] {
        assert_eq!(video[key], "bt709", "{key}");
    }
}

#[test]
fn export_samples_are_full_range_bt709_like_their_tags() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("samples.html");
    let output = temp.path().join("samples.mp4");
    std::fs::write(&input, "<style>html,body{margin:0}</style><div data-composition-id='samples' data-no-timeline data-width='32' data-height='32' data-fps='30' data-duration='0.2' style='position:relative;width:32px;height:32px;background:#000'><div style='position:absolute;left:0;top:0;width:16px;height:32px;background:#f00'></div></div>").unwrap();
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
    // Decode as full-range BT.709 whatever the tags say. Limited-range samples
    // lift black to 16, and BT.601 samples lift red's green to about 25.
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&output)
        .args(["-frames:v", "1", "-vf", "scale=in_color_matrix=bt709:in_range=full,format=rgb24"])
        .args(["-f", "rawvideo", "-"])
        .output()
        .expect("ffmpeg must be installed for export verification");
    assert!(
        decoded.status.success(),
        "{}",
        String::from_utf8_lossy(&decoded.stderr)
    );
    let frame = decoded.stdout;
    assert_eq!(frame.len(), 32 * 32 * 3);
    let pixel = |x: usize, y: usize| -> [u8; 3] {
        let i = (y * 32 + x) * 3;
        [frame[i], frame[i + 1], frame[i + 2]]
    };
    let red = pixel(4, 16);
    assert!(
        red[0] >= 245 && red[1] <= 8 && red[2] <= 8,
        "red {red:?}, expected about [255, 0, 0]"
    );
    let black = pixel(28, 16);
    assert!(black.iter().all(|&c| c <= 6), "black {black:?}, expected about [0, 0, 0]");
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
