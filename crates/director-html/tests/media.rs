use director_core::systems::renderer::render_frame;
use director_html::{compile, CompileOptions};

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
