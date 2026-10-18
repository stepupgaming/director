use director_core::systems::renderer::render_frame;
use director_html::{
    compile, compile_capture, Capture, CapturedFrame, CapturedMedia, CapturedNode, CompileOptions,
    CAPTURE_FORMAT,
};
use std::collections::BTreeMap;

#[test]
fn native_video_source_start_and_reverse_seeks() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("clip.mkv");
    let result = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=64x64:r=30:d=0.5",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=64x64:r=30:d=0.5",
            "-filter_complex",
            "[0:v][1:v]concat=n=2:v=1:a=0",
            "-c:v",
            "ffv1",
        ])
        .arg(&path)
        .output()
        .expect("FFmpeg must be installed for native media compiler tests");
    assert!(
        result.status.success(),
        "fixture generation failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let html="<style>body{margin:0}video{width:64px;height:64px}</style><video muted src='clip.mkv' data-media-start='0.2'></video>";
    let mut compiled = compile(
        html,
        CompileOptions {
            width: 64,
            height: 64,
            duration: 1.0,
            base_dir: temp.path().into(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    let info = skia_safe::ImageInfo::new(
        (64, 64),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut frames = Vec::new();
    for time in [0.0, 0.6, 0.0] {
        render_frame(&mut compiled.director, time, surface.canvas()).unwrap();
        let mut pixels = vec![0; 64 * 64 * 4];
        assert!(surface.read_pixels(&info, &mut pixels, 64 * 4, (0, 0)));
        frames.push(pixels);
    }
    let at = (32 * 64 + 32) * 4;
    assert!(frames[0][at] > 245 && frames[0][at + 2] < 5);
    assert!(frames[1][at] < 5 && frames[1][at + 2] > 245);
    assert_eq!(frames[0], frames[2]);
}

#[test]
fn captured_video_seeks_across_different_samples() {
    let bytes = ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=64x64:r=30:d=0.5",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x00ff00:s=64x64:r=30:d=0.5",
            "-filter_complex",
            "[0:v][1:v]concat=n=2:v=1:a=0",
            "-c:v",
            "ffv1",
        ],
        "clip.mkv",
    );
    let src = "clip.mkv";
    let mut assets = BTreeMap::new();
    assets.insert(src.into(), data_uri(&bytes));
    let video = |fit: &str| CapturedNode {
        tag: "video".into(),
        id: "picture".into(),
        rect: [0.0, 0.0, 64.0, 64.0],
        styles: BTreeMap::from([("object-fit".into(), fit.into())]),
        text: None,
        svg: None,
        src: Some(src.into()),
        shader: None,
        media: Some(CapturedMedia {
            muted: true,
            looped: false,
            hidden: false,
            volume: 1.0,
            media_start: 0.2,
            playback_rate: 1.0,
            start: 0.0,
            end: 1.0,
        }),
        children: Vec::new(),
    };
    let mut compiled = compile_capture(
        capture(
            assets,
            vec![
                frame(0.0, video("fill")),
                frame(0.5, video("cover")),
            ],
        ),
        std::path::Path::new("."),
    )
    .unwrap();
    assert!(compiled
        .director
        .audio_mixer
        .tracks
        .iter()
        .flatten()
        .next()
        .is_none());
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    let info = skia_safe::ImageInfo::new(
        (64, 64),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut frames = Vec::new();
    for time in [0.0, 0.6, 0.0] {
        render_frame(&mut compiled.director, time, surface.canvas()).unwrap();
        let mut pixels = vec![0; 64 * 64 * 4];
        assert!(surface.read_pixels(&info, &mut pixels, 64 * 4, (0, 0)));
        frames.push(pixels);
    }
    let at = (32 * 64 + 32) * 4;
    assert!(frames[0][at] > 245 && frames[0][at + 1] < 5 && frames[0][at + 2] < 5);
    assert!(frames[1][at] < 5 && frames[1][at + 1] > 245 && frames[1][at + 2] < 5);
    assert_eq!(frames[0], frames[2]);
}

#[test]
fn captured_audio_is_one_track_across_samples() {
    let bytes = ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=0.5",
            "-c:a",
            "pcm_s16le",
        ],
        "tone.wav",
    );
    let src = "tone.wav";
    let mut assets = BTreeMap::new();
    assets.insert(src.into(), data_uri(&bytes));
    let audio = CapturedNode {
        tag: "audio".into(),
        id: "bed".into(),
        rect: [0.0, 0.0, 0.0, 0.0],
        styles: BTreeMap::new(),
        text: None,
        svg: None,
        src: Some(src.into()),
        shader: None,
        media: Some(CapturedMedia {
            muted: false,
            looped: true,
            hidden: false,
            volume: 0.5,
            media_start: 0.25,
            playback_rate: 1.0,
            start: 0.0,
            end: 1.0,
        }),
        children: Vec::new(),
    };
    let mut expected = director_core::audio::load_audio_bytes(&bytes, 48000).unwrap();
    expected.drain(..((0.25 * 96000.0) as usize).min(expected.len()));
    let compiled = compile_capture(
        capture(
            assets,
            vec![
                frame(0.0, stack("rgb(255, 0, 0)", audio.clone())),
                frame(0.5, stack("rgb(0, 0, 255)", audio)),
            ],
        ),
        std::path::Path::new("."),
    )
    .unwrap();
    let tracks: Vec<_> = compiled
        .director
        .audio_mixer
        .tracks
        .iter()
        .flatten()
        .collect();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].samples, expected);
    assert_eq!(tracks[0].start_time, 0.0);
    assert_eq!(tracks[0].duration, Some(1.0));
    assert!(tracks[0].loop_audio);
    assert_eq!(tracks[0].volume.current_value, 0.5);
}

#[test]
fn captured_hidden_audio_is_silent() {
    let bytes = ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=0.2",
            "-c:a",
            "pcm_s16le",
        ],
        "tone.wav",
    );
    let src = "tone.wav";
    let mut assets = BTreeMap::new();
    assets.insert(src.into(), data_uri(&bytes));
    let compiled = compile_capture(
        single(
            assets,
            CapturedNode {
                    tag: "audio".into(),
                    id: "bed".into(),
                    rect: [0.0, 0.0, 0.0, 0.0],
                    styles: BTreeMap::new(),
                    text: None,
                    svg: None,
                    src: Some(src.into()),
                    shader: None,
                    media: Some(CapturedMedia {
                        muted: false,
                        looped: false,
                        hidden: true,
                        volume: 0.5,
                        media_start: 0.0,
                        playback_rate: 1.0,
                        start: 0.0,
                        end: 1.0,
                    }),
                    children: Vec::new(),
                },
        ),
        std::path::Path::new("."),
    )
    .unwrap();
    let track = compiled
        .director
        .audio_mixer
        .tracks
        .iter()
        .flatten()
        .next()
        .unwrap();
    assert_eq!(track.volume.current_value, 0.0);
    assert!(!track.loop_audio);
}

#[test]
fn captured_media_keeps_the_static_refusals() {
    let message = |root: CapturedNode| {
        compile_capture(single(BTreeMap::new(), root), std::path::Path::new("."))
            .err()
            .map(|error| error.to_string())
            .unwrap_or_else(|| panic!("capture compiled"))
    };
    let video = |muted, looped| media_node("video", "picture", Some("clip.mkv"), muted, looped, 1.0);
    assert!(message(video(false, false)).contains("video audio needs an explicit audio element"));
    let bytes = ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=8x8:r=30:d=1",
            "-c:v",
            "ffv1",
        ],
        "clip.mkv",
    );
    let mut assets = BTreeMap::new();
    assets.insert("clip.mkv".into(), data_uri(&bytes));
    compile_capture(single(assets, video(true, true)), std::path::Path::new("."))
        .expect("muted looped video compiles");
    assert!(message(media_node("audio", "bed", Some("tone.wav"), false, false, 1.5))
        .contains("audio playback-rate translation unsupported"));
    assert!(message(media_node("iframe", "embed", None, false, false, 1.0))
        .contains("captured iframe requires native media translation"));
    let duplicate = CapturedNode {
        tag: "div".into(),
        id: "root".into(),
        rect: [0.0, 0.0, 8.0, 8.0],
        styles: BTreeMap::new(),
        text: None,
        svg: None,
        src: None,
        shader: None,
        media: None,
        children: vec![
            media_node("audio", "bed", Some("a.wav"), false, false, 1.0),
            media_node("audio", "bed", Some("b.wav"), false, false, 1.0),
        ],
    };
    assert!(message(duplicate).contains("duplicate captured media id 'bed'"));
    let changed = capture(
        BTreeMap::new(),
        vec![
            frame(0.0, media_node("audio", "bed", Some("a.wav"), false, false, 1.0)),
            frame(0.5, media_node("audio", "bed", Some("b.wav"), false, false, 1.0)),
        ],
    );
    let changed = match compile_capture(changed, std::path::Path::new(".")) {
        Err(error) => error,
        Ok(_) => panic!("inconsistent media compiled"),
    };
    assert!(changed
        .to_string()
        .contains("captured media 'bed' changes across samples"));
}

fn ffmpeg(args: &[&str], name: &str) -> Vec<u8> {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join(name);
    let result = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error"])
        .args(args)
        .arg(&path)
        .output()
        .expect("FFmpeg must be installed for native media compiler tests");
    assert!(
        result.status.success(),
        "fixture generation failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::read(path).unwrap()
}

fn data_uri(bytes: &[u8]) -> String {
    format!(
        "data:application/octet-stream;base64,{}",
        base64_encode(bytes)
    )
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut chunks = data.chunks_exact(3);
    for chunk in chunks.by_ref() {
        let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | chunk[2] as u32;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
    }
    let rest = chunks.remainder();
    if rest.len() == 1 {
        let n = (rest[0] as u32) << 16;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rest.len() == 2 {
        let n = ((rest[0] as u32) << 16) | ((rest[1] as u32) << 8);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

fn media_node(
    tag: &str,
    id: &str,
    src: Option<&str>,
    muted: bool,
    looped: bool,
    playback_rate: f64,
) -> CapturedNode {
    CapturedNode {
        tag: tag.into(),
        id: id.into(),
        rect: [0.0, 0.0, 8.0, 8.0],
        styles: BTreeMap::new(),
        text: None,
        svg: None,
        src: src.map(str::to_string),
        shader: None,
        media: (tag != "iframe").then_some(CapturedMedia {
            muted,
            looped,
            hidden: false,
            volume: 1.0,
            media_start: 0.0,
            playback_rate,
            start: 0.0,
            end: 1.0,
        }),
        children: Vec::new(),
    }
}

fn stack(color: &str, audio: CapturedNode) -> CapturedNode {
    CapturedNode {
        tag: "div".into(),
        id: "root".into(),
        rect: [0.0, 0.0, 8.0, 8.0],
        styles: BTreeMap::from([("background-color".into(), color.into())]),
        text: None,
        svg: None,
        src: None,
        shader: None,
        media: None,
        children: vec![audio],
    }
}

fn frame(time: f64, root: CapturedNode) -> CapturedFrame {
    CapturedFrame { time, root }
}

fn capture(assets: BTreeMap<String, String>, frames: Vec<CapturedFrame>) -> Capture {
    scene(assets, 2.0, frames)
}

fn single(assets: BTreeMap<String, String>, root: CapturedNode) -> Capture {
    scene(assets, 1.0, vec![frame(0.0, root)])
}

fn scene(assets: BTreeMap<String, String>, sample_rate: f64, frames: Vec<CapturedFrame>) -> Capture {
    Capture {
        format: CAPTURE_FORMAT.into(),
        width: 64,
        height: 64,
        fps: 30.0,
        duration: 1.0,
        sample_rate,
        fonts: Vec::new(),
        assets,
        frames,
    }
}
