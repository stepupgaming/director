//! `<text3d>` children of `<model>`: extruded type meshed by `director-3d`
//! and lit as part of the model's scene, its scene document round trip and
//! its compile errors. GPU tests skip on a machine without a Vulkan adapter.

use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;
use director_scene::{NodeBody, SceneDocument};
use std::path::PathBuf;
use std::sync::Arc;

const W: i32 = 320;
const H: i32 = 120;

fn font_path(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../director-core/assets/fonts")
        .join(name)
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

fn gpu() -> bool {
    let ok = director_3d::available();
    if !ok {
        eprintln!("skipped: no Vulkan adapter for director-3d");
    }
    ok
}

fn options() -> CompileOptions {
    CompileOptions {
        width: W,
        height: H,
        duration: 2.0,
        ..Default::default()
    }
}

/// A full-frame model over a blue page holding `inner`.
fn page(model: &str, inner: &str) -> String {
    format!(
        "<style>body{{margin:0}}#main{{background:rgb(0,0,255)}}model{{display:block;width:{W}px;height:{H}px}}</style>\
         <div id='main' data-composition-id='main' data-width='{W}' data-height='{H}' data-duration='2'>\
         <model id='m' {model}>{inner}</model></div>"
    )
}

/// One line of type taking `attrs`.
fn line(text: &str, attrs: &str) -> String {
    page("", &format!("<text3d {attrs}>{text}</text3d>"))
}

fn build(source: &str) -> CompiledHtml {
    compile(source, options()).unwrap_or_else(|e| panic!("{source}: {e}"))
}

fn frame(c: &mut CompiledHtml, time: f64) -> Vec<u8> {
    let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((W, H), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; (W * H * 4) as usize];
    assert!(surface.read_pixels(&info, &mut pixels, W as usize * 4, (0, 0)));
    pixels
}

fn shot(source: &str) -> Vec<u8> {
    frame(&mut build(source), 0.0)
}

/// Pixels whose channels differ by more than 2.
fn changed(a: &[u8], b: &[u8]) -> usize {
    a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| (0..3).any(|k| p[k].abs_diff(q[k]) > 2)).count()
}

/// Pixels the type covers: at least halfway from the blue page.
fn silhouette(p: &[u8]) -> Vec<bool> {
    p.chunks(4).map(|p| p[0] as i32 + p[1] as i32 + (255 - p[2] as i32) > 192).collect()
}

fn count(mask: &[bool]) -> usize {
    mask.iter().filter(|m| **m).count()
}

fn iou(a: &[bool], b: &[bool]) -> f32 {
    let both = a.iter().zip(b).filter(|(a, b)| **a && **b).count();
    let either = a.iter().zip(b).filter(|(a, b)| **a || **b).count();
    both as f32 / either.max(1) as f32
}

/// The smallest pixel box holding the mask: left, top, right, bottom.
fn bounds(mask: &[bool]) -> [i32; 4] {
    let mut b = [W, H, -1, -1];
    for (i, m) in mask.iter().enumerate() {
        if *m {
            let (x, y) = (i as i32 % W, i as i32 / W);
            b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
        }
    }
    b
}

/// Skia's 2D fill of `text` in Inter at weight 700, its ink box stretched
/// over `box`.
fn flat(text: &str, ink: [i32; 4]) -> Vec<bool> {
    use skia_safe::{surfaces, Color, Font, FontMgr, Paint, Rect};
    let bytes = std::fs::read(font_path("Inter-Bold.ttf")).unwrap();
    let face = FontMgr::new().new_from_data(skia_safe::Data::new_copy(&bytes), None).unwrap();
    let bold = [director_core::element::FontVariation { axis: "wght".into(), value: 700.0 }];
    let mut font = Font::from_typeface(director_core::node::text::vary_typeface(face, &bold), 100.0);
    font.set_hinting(skia_safe::FontHinting::None);
    font.set_subpixel(true);
    let (_, glyphs) = font.measure_str(text, None);
    let target = Rect::new(ink[0] as f32, ink[1] as f32, ink[2] as f32, ink[3] as f32);
    let mut surface = surfaces::raster_n32_premul((W, H)).unwrap();
    let canvas = surface.canvas();
    canvas.clear(Color::TRANSPARENT);
    canvas.translate((target.left, target.top));
    canvas.scale((target.width() / glyphs.width(), target.height() / glyphs.height()));
    canvas.translate((-glyphs.left, -glyphs.top));
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    canvas.draw_str(text, (0.0, 0.0), &font, &paint);
    let pixmap = surface.peek_pixels().unwrap();
    pixmap.bytes().unwrap().chunks(4).map(|p| p[3] > 127).collect()
}

/// Lit pixels whose hue leans to one channel.
fn leaning(p: &[u8], channel: usize) -> usize {
    p.chunks(4)
        .filter(|p| (0..3).filter(|k| *k != channel).all(|k| p[channel] as i32 > p[k] as i32 + 40))
        .count()
}

#[test]
fn type_paints_the_glyph_outlines_over_a_transparent_sky() {
    if !gpu() {
        return;
    }
    let text = "R&D 8";
    // A closer camera than the framing one fills the frame with the line.
    let pixels = shot(&page(
        "style='--camera-distance:0.55'",
        &format!("<text3d font-weight='700' depth='0.02' bevel='0'>{text}</text3d>"),
    ));
    let mask = silhouette(&pixels);
    let ink = bounds(&mask);
    let reference = flat(text, ink);
    let overlap = iou(&mask, &reference);
    eprintln!("type covers {} px in {ink:?}, iou with Skia's fill {overlap:.3}", count(&mask));
    assert!(ink[2] - ink[0] > W / 2, "the line fills the frame: {ink:?}");
    assert!(overlap > 0.85, "the silhouette follows the outlines: {overlap:.3}");
    let corner = &pixels[..4];
    assert!(corner[2] > 250 && corner[0] < 6 && corner[1] < 6, "the sky stays transparent: {corner:?}");
}

#[test]
fn side_color_paints_the_walls_and_chamfers() {
    if !gpu() {
        return;
    }
    let attrs = "rotation='0 55 0' depth='0.6' color='rgb(220,30,30)'";
    let plain = shot(&line("IH", attrs));
    let sided = shot(&line("IH", &format!("{attrs} side-color='rgb(30,200,30)'")));
    let (red, green) = (leaning(&plain, 0), leaning(&plain, 1));
    eprintln!("one colour: {red} red, {green} green");
    assert!(red > 500 && green == 0, "faces and walls share the colour");
    let (red, green) = (leaning(&sided, 0), leaning(&sided, 1));
    eprintln!("with side-color: {red} red, {green} green");
    assert!(red > 300 && green > 300, "faces stay red and walls turn green");
}

#[test]
fn a_family_and_weight_resolve_like_the_font_file() {
    if !gpu() {
        return;
    }
    let by_family = shot(&line("Hg", "font-family='Nope, Inter' font-weight='bold'"));
    let by_file = shot(&line("Hg", &format!("font='{}' font-weight='700'", font_path("Inter-Bold.ttf"))));
    assert!(count(&silhouette(&by_family)) > 2000);
    assert_eq!(changed(&by_family, &by_file), 0, "Inter at bold is Inter-Bold.ttf at 700");
    // Heavier strokes fill more of the same framed box.
    let fill = |weight: u32| {
        let mask = silhouette(&shot(&line("Hm", &format!("font-family='Archivo' font-weight='{weight}'"))));
        let [l, t, r, b] = bounds(&mask);
        count(&mask) as f32 / ((r - l) * (b - t)) as f32
    };
    let (thin, black) = (fill(200), fill(900));
    eprintln!("Archivo ink fraction at 200 {thin:.3}, at 900 {black:.3}");
    assert!(black > thin * 1.4, "the weight sets the wght axis");
}

#[test]
fn the_camera_pitches_over_the_top_and_closes_in() {
    if !gpu() {
        return;
    }
    let at = |style: &str| {
        let p = shot(&page(&format!("style='{style}'"), "<text3d>LP</text3d>"));
        silhouette(&p)
    };
    let front = at("--camera-pitch:0deg");
    // A full turn comes back to the front, and half a turn sees the back
    // upside down: the front's silhouette flipped top to bottom.
    let turned = iou(&front, &at("--camera-pitch:360deg"));
    let flipped: Vec<bool> = (0..W * H).map(|i| front[((H - 1 - i / W) * W + i % W) as usize]).collect();
    let back = at("--camera-pitch:180deg");
    let over = iou(&flipped, &back);
    let same = iou(&front, &back);
    eprintln!("pitch 360 iou {turned:.3}, pitch 180 against the flipped front {over:.3}, against the front {same:.3}");
    assert!(turned > 0.98 && over > 0.85 && same < 0.6, "{turned:.3} {over:.3} {same:.3}");
    // A close camera keeps the swung line in front of its near plane: the
    // type grows past the frame and keeps more ink than the framed shot.
    let close = at("--camera-distance:0.4;--camera-yaw:40deg");
    let [_, t, _, b] = bounds(&close);
    eprintln!("close and yawed: {} px against {} framed, rows {t}..{b}", count(&close), count(&front));
    assert!(t <= 1 || b >= H - 1, "the close type runs off the frame: rows {t}..{b}");
    assert!(count(&close) > count(&front) * 3, "the near plane clips nothing");
}

#[test]
fn path_traced_type_keeps_its_silhouette() {
    if !gpu() || !director_3d::traceable() {
        eprintln!("skipped: no ray tracing Vulkan adapter");
        return;
    }
    let attrs = "font-weight='800' rotation='-10 20 0'";
    let raster = silhouette(&shot(&line("3D", attrs)));
    let traced = silhouette(&shot(&page("paths='16'", &format!("<text3d {attrs}>3D</text3d>"))));
    let overlap = iou(&raster, &traced);
    eprintln!("raster {} px, traced {} px, iou {overlap:.3}", count(&raster), count(&traced));
    assert!(count(&traced) > 2000 && overlap > 0.9, "the traced mesh covers the raster one: {overlap:.3}");
}

#[test]
fn the_scene_document_keeps_the_type() {
    if !gpu() {
        return;
    }
    let attrs = "font-weight='600' size='0.5' depth='0.1' bevel='0.01' position='0 0.1 0' rotation='10 -20 0' \
                 color='#ffcc00' side-color='#202020' metallic='1' roughness='0.3'";
    let mut compiled = build(&line("Type  in\n depth", attrs));
    let document = export_scene(&compiled.director).unwrap();
    let parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed, document);
    let model = parsed
        .nodes
        .iter()
        .find_map(|node| match &node.body {
            NodeBody::Html(html) => html.content.as_ref().and_then(|c| match &c.body {
                NodeBody::Model(model) => Some(model.clone()),
                _ => None,
            }),
            _ => None,
        })
        .expect("a model body");
    assert_eq!(model.texts.len(), 1);
    let t = &model.texts[0];
    assert_eq!(t.text, "Type in depth", "white space collapses");
    assert_eq!((t.weight, t.size, t.depth, t.bevel), (600.0, 0.5, 0.1, 0.01));
    assert_eq!((t.position, t.rotation, t.metallic, t.roughness), ([0.0, 0.1, 0.0], [10.0, -20.0, 0.0], 1.0, 0.3));
    assert_eq!(t.color, [1.0, 0.8, 0.0]);
    assert!(t.side_color.is_some());
    assert_eq!(parsed.asset(&t.font).expect("font asset").media_type, "font/ttf");
    assert_eq!(parsed.asset(&model.asset).expect("scene asset").media_type, "model/gltf-binary");

    let mut registry = director_core::scene_import::ImportRegistry::core();
    director_html::register_import(&mut registry);
    let renderer = director_render::Renderer::new(Arc::new(parsed), Arc::new(director_core::DefaultAssetLoader))
        .with_registry(registry);
    let imported = renderer.render_frame(0.0).unwrap();
    let live = frame(&mut compiled, 0.0);
    assert!(count(&silhouette(&live)) > 300);
    assert_eq!(changed(&imported.rgba, &live), 0);
}

#[test]
fn malformed_type_fails_to_compile() {
    let error = |source: &str| compile(source, options()).err().expect("a compile error").to_string();
    let e = error(&page("", ""));
    assert!(e.contains("model without src, <splat> or <text3d> children"), "{e}");
    let e = error(&line("  ", ""));
    assert!(e.contains("text3d has no text"), "{e}");
    let e = error(&line("A", "font-family='Nope'"));
    assert!(e.contains("text3d font-family 'Nope' names no font Director has"), "{e}");
    let glb = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../director-3d/tests/fixtures/model.glb");
    let e = error(&line("A", &format!("font='{}'", glb.canonicalize().unwrap().to_string_lossy().replace('\\', "/"))));
    assert!(e.contains("text3d font must be a TrueType or OpenType file"), "{e}");
    let e = error(&line("A", "font-weight='heavy'"));
    assert!(e.contains("text3d font-weight 'heavy' must be normal, bold or a number from 1 to 1000"), "{e}");
    let e = error(&line("A", "depth='0.1' bevel='0.06'"));
    assert!(e.contains("text3d bevel 0.06 must be at most 0.05"), "{e}");
    let e = error(&line("A", "size='0'"));
    assert!(e.contains("text3d size must be above 0"), "{e}");
    let e = error(&line("A", "rotation='1 2'"));
    assert!(e.contains("text3d rotation '1 2' must be three numbers in degrees"), "{e}");
    let e = error(&line("A", "roughness='2'"));
    assert!(e.contains("text3d roughness '2' must be a number from 0 to 1"), "{e}");
}
