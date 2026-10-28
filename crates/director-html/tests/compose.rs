//! `data-composition-src` mounts a sub-composition file into a host clip:
//! template contents or a standalone page, rebased URLs, deduplicated
//! scripts, and the file's GSAP timeline on the clip's clock.

use director_html::{compile_file, CompileOptions, CompiledHtml};
use director_render::render_frame;
use std::path::PathBuf;

const W: i32 = 64;
const H: i32 = 32;

fn options() -> CompileOptions {
    CompileOptions {
        width: W,
        height: H,
        duration: 4.0,
        gsap_dir: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/node_modules/gsap/dist")),
        ..Default::default()
    }
}

const GSAP: &str = "<script src='https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/gsap.min.js'></script>";

/// A host page mounting `src` as clip `id` from 1 s to 3 s.
fn host(id: &str, src: &str) -> String {
    format!(
        "<style>body{{margin:0}}#film{{position:relative;width:{W}px;height:{H}px;background:black}}\
         #film > div{{position:absolute;inset:0}}</style>{GSAP}\
         <div id='film' data-composition-id='film' data-width='{W}' data-height='{H}' data-duration='4'>\
         <div id='el-{id}' data-composition-id='{id}' data-composition-src='{src}' data-start='1' data-duration='2'></div></div>"
    )
}

/// The page `file` inside a fresh directory holding `files`.
fn compile_in(files: &[(&str, &[u8])], file: &str) -> Result<CompiledHtml, String> {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    compile_file(&dir.path().join(file), options()).map_err(|e| e.to_string())
}

fn pixel(c: &mut CompiledHtml, time: f64, x: i32, y: i32) -> [u8; 4] {
    let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((1, 1), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut out = [0u8; 4];
    assert!(surface.read_pixels(&info, &mut out, 4, (x, y)));
    out
}

/// A sub-composition whose red box fades in over its first second.
const TEMPLATE: &str = "<!doctype html><html><head><style>#gone{color:red}</style></head><body><template>\
    <style>#shot-box{position:absolute;left:0;top:0;width:32px;height:32px;background:rgb(255,0,0);opacity:0}</style>\
    <div id='shot' data-composition-id='shot' data-width='1920' data-height='1080' data-fps='60'><div id='shot-box'></div></div>\
    <script src='https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/gsap.min.js'></script>\
    <script>(() => { window.__timelines = window.__timelines || {}; const tl = gsap.timeline({ paused: true });\
    tl.fromTo('#shot-box', { opacity: 0 }, { opacity: 1, duration: 1, ease: 'none' }, 0); window.__timelines['shot'] = tl; })();</script>\
    </template></body></html>";

#[test]
fn a_template_mounts_on_the_clip_clock() {
    let mut c = compile_in(&[("index.html", host("shot", "scenes/shot.html").as_bytes()), ("scenes/shot.html", TEMPLATE.as_bytes())], "index.html").unwrap();
    // Hidden before its clip, half faded at 1.5 s, solid at 2.5 s, gone after 3 s.
    assert_eq!(pixel(&mut c, 0.5, 8, 8)[0], 0);
    let half = pixel(&mut c, 1.5, 8, 8)[0];
    assert!((100..160).contains(&half), "{half}");
    assert_eq!(pixel(&mut c, 2.5, 8, 8)[0], 255);
    assert_eq!(pixel(&mut c, 3.5, 8, 8)[0], 0);
    // The file's root takes the host's size, not its own 1920x1080.
    assert_eq!(c.director.width, W);
}

#[test]
fn a_standalone_page_mounts_with_rebased_links() {
    let page = "<!doctype html><html><head><link rel='stylesheet' href='../look.css'>        <style>#shot-box{background-image:url('../green.png')}</style></head><body>        <div id='shot' data-composition-id='shot' data-width='1920' data-height='1080'><div id='shot-box'></div></div></body></html>";
    let look = "#shot-box{position:absolute;left:0;top:0;width:32px;height:32px}";
    let mut surface = skia_safe::surfaces::raster_n32_premul((4, 4)).unwrap();
    surface.canvas().clear(skia_safe::Color::GREEN);
    let png = surface.image_snapshot().encode(None, skia_safe::EncodedImageFormat::PNG, None).unwrap();
    let index = host("shot", "scenes/shot.html");
    let files: [(&str, &[u8]); 4] = [("index.html", index.as_bytes()), ("scenes/shot.html", page.as_bytes()), ("look.css", look.as_bytes()), ("green.png", png.as_bytes())];
    let mut c = compile_in(&files, "index.html").unwrap();
    assert_eq!(pixel(&mut c, 2.0, 8, 8), [0, 255, 0, 255]);
    assert_eq!(pixel(&mut c, 2.0, 48, 8), [0, 0, 0, 255]);
}

#[test]
fn a_template_file_compiles_on_its_own() {
    let mut c = compile_in(&[("shot.html", TEMPLATE.as_bytes())], "shot.html").unwrap();
    assert_eq!(c.director.width, 1920);
    assert_eq!(pixel(&mut c, 0.0, 8, 8)[0], 0);
    assert_eq!(pixel(&mut c, 1.5, 8, 8), [255, 0, 0, 255]);
}

#[test]
fn mounting_refuses_what_it_cannot_place() {
    let missing = host("shot", "nope.html");
    let wrong_id = host("other", "shot.html");
    let filled = host("shot", "shot.html").replace("data-duration='2'></div>", "data-duration='2'><p>x</p></div>");
    let cases = [
        (missing.as_str(), "nope.html"),
        (wrong_id.as_str(), "exactly one root with data-composition-id 'other'"),
        (filled.as_str(), "the host clip must be empty"),
    ];
    for (page, message) in cases {
        let error = compile_in(&[("index.html", page.as_bytes()), ("shot.html", TEMPLATE.as_bytes())], "index.html").err().expect(page);
        assert!(error.contains(message), "{message}: {error}");
    }
}
