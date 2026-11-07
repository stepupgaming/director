use director_render::render_frame;
use director_html::{compile, CompileOptions};
use std::path::Path;

const RED_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAYAAADED76LAAAAEklEQVR4nGP4z8DwHx9mGBkKAMLXf4EvceABAAAAAElFTkSuQmCC";

fn frame(source: &str, base_dir: &Path) -> Vec<u8> {
    let mut c = compile(
        source,
        CompileOptions {
            width: 64,
            height: 64,
            duration: 1.0,
            base_dir: base_dir.into(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    render_frame(&mut c.director, 0.0, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((64, 64), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; 64 * 64 * 4];
    assert!(surface.read_pixels(&info, &mut pixels, 64 * 4, (0, 0)));
    pixels
}

fn pixel(p: &[u8], x: usize, y: usize) -> [u8; 4] {
    p[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4].try_into().unwrap()
}

fn red(p: [u8; 4]) -> bool {
    p[0] > 220 && p[1] < 40 && p[2] < 40
}

fn blue(p: [u8; 4]) -> bool {
    p[2] > 220 && p[0] < 40 && p[1] < 40
}

/// The four corner pixels show the page and the centre shows the content.
fn assert_rounded(pixels: &[u8], what: &str) {
    for (x, y) in [(1, 1), (62, 1), (62, 62), (1, 62)] {
        assert!(blue(pixel(pixels, x, y)), "{what} corner ({x},{y}) {:?}", pixel(pixels, x, y));
    }
    assert!(red(pixel(pixels, 32, 32)), "{what} centre {:?}", pixel(pixels, 32, 32));
    assert!(red(pixel(pixels, 32, 1)), "{what} top edge {:?}", pixel(pixels, 32, 1));
}

#[test]
fn border_radius_rounds_img_content() {
    let dir = tempfile::tempdir().unwrap();
    let html = format!(
        "<style>body{{margin:0;background:#00f}}img{{display:block;width:64px;height:64px;border-radius:16px}}</style><img src=\"data:image/png;base64,{RED_PNG}\">"
    );
    assert_rounded(&frame(&html, dir.path()), "img");
}

#[test]
fn border_radius_rounds_video_content() {
    let dir = tempfile::tempdir().unwrap();
    let result = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=c=red:s=64x64:r=30:d=1", "-c:v", "ffv1"])
        .arg(dir.path().join("clip.mkv"))
        .output()
        .expect("FFmpeg must be installed for native media compiler tests");
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let html = "<style>body{margin:0;background:#00f}video{display:block;width:64px;height:64px;border-radius:16px}</style><video muted src='clip.mkv'></video>";
    assert_rounded(&frame(html, dir.path()), "video");
}

#[test]
fn replaced_content_follows_the_content_edge_curve() {
    let dir = tempfile::tempdir().unwrap();
    // A 20px radius less a 4px border and 4px padding leaves a 12px curve on the 48px content box.
    let html = format!(
        "<style>body{{margin:0;background:#00f}}img{{display:block;box-sizing:border-box;width:64px;height:64px;border:4px solid #0f0;padding:4px;border-radius:20px}}</style><img src=\"data:image/png;base64,{RED_PNG}\">"
    );
    let pixels = frame(&html, dir.path());
    assert!(!red(pixel(&pixels, 9, 9)), "content corner {:?}", pixel(&pixels, 9, 9));
    assert!(red(pixel(&pixels, 32, 9)), "content top edge {:?}", pixel(&pixels, 32, 9));
    assert!(red(pixel(&pixels, 32, 32)), "centre {:?}", pixel(&pixels, 32, 32));
    assert!(blue(pixel(&pixels, 1, 1)), "outer corner {:?}", pixel(&pixels, 1, 1));
}
