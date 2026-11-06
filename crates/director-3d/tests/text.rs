//! Extruded type meshes, built without a GPU.

use std::collections::HashMap;

use director_3d::{solid, Asset, Part, Text3d};

fn font(name: &str) -> Asset {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../director-core/assets/fonts").join(name);
    Asset::font(std::fs::read(path).unwrap(), None).unwrap()
}

fn text(s: &str, depth: f32, bevel: f32) -> Text3d {
    Text3d {
        font: font("Inter-Bold.ttf"),
        text: s.into(),
        weight: 700.0,
        stretch: 100.0,
        size: 1.0,
        depth,
        bevel,
        position: [0.0; 3],
        rotation: [0.0; 3],
        color: [1.0; 3],
        side_color: None,
        metallic: 0.0,
        roughness: 0.4,
    }
}

/// Coverage of `s` filled by Skia in Inter at weight 700 and `px` pixels to
/// the em, in square ems.
fn skia_area(s: &str, px: f32) -> f32 {
    use skia_safe::{surfaces, Color, Font, FontMgr, Paint};
    let bytes = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../director-core/assets/fonts/Inter-Bold.ttf")).unwrap();
    let typeface = FontMgr::new().new_from_data(skia_safe::Data::new_copy(&bytes), None).unwrap();
    // Inter-Bold.ttf is variable, with Regular as its default instance.
    let bold = [director_core::element::FontVariation { axis: "wght".into(), value: 700.0 }];
    let typeface = director_core::node::text::vary_typeface(typeface, &bold);
    let mut font = Font::from_typeface(typeface, px);
    font.set_hinting(skia_safe::FontHinting::None);
    font.set_subpixel(true);
    font.set_edging(skia_safe::font::Edging::AntiAlias);
    let (w, h) = ((px * 0.8 * s.len() as f32) as i32 + 64, (px * 1.5) as i32);
    let mut surface = surfaces::raster_n32_premul((w, h)).unwrap();
    let canvas = surface.canvas();
    canvas.clear(Color::TRANSPARENT);
    canvas.draw_str(s, (32.0, px * 1.1), &font, &Paint::default());
    let pixmap = surface.peek_pixels().unwrap();
    let bytes = pixmap.bytes().unwrap();
    let alpha: f32 = bytes.chunks(4).map(|p| p[3] as f32 / 255.0).sum();
    alpha / (px * px)
}

/// Edges keyed by their end positions, counted once per triangle side.
fn open_edges(parts: &[&Part]) -> usize {
    let mut edges: HashMap<([u32; 3], [u32; 3]), i32> = HashMap::new();
    let key = |p: [f32; 3]| p.map(f32::to_bits);
    for part in parts {
        for t in part.indices.chunks(3) {
            for k in 0..3 {
                let (a, b) = (key(part.positions[t[k] as usize]), key(part.positions[t[(k + 1) % 3] as usize]));
                // A directed edge cancels the same edge the other way round.
                if a < b {
                    *edges.entry((a, b)).or_default() += 1;
                } else {
                    *edges.entry((b, a)).or_default() -= 1;
                }
            }
        }
    }
    edges.values().filter(|n| **n != 0).count()
}

#[test]
fn a_solid_encloses_the_outline_times_its_depth() {
    let flat = solid(&text("Director 3D", 0.3, 0.0)).unwrap();
    let area = skia_area("Director 3D", 400.0);
    println!("outline area {:.4} em², Skia fill {area:.4} em²", flat.area);
    assert!((flat.area - area).abs() < area * 0.01, "outline {} vs Skia {area}", flat.area);
    let volume = flat.faces.volume() + flat.sides.volume();
    println!("volume {volume:.5}, area x depth {:.5}", flat.area * 0.3);
    assert!((volume - flat.area * 0.3).abs() < flat.area * 0.3 * 1e-3);
    // A chamfer trims a little volume from each face's rim.
    let bevelled = solid(&text("Director 3D", 0.3, 0.02)).unwrap();
    let volume = bevelled.faces.volume() + bevelled.sides.volume();
    println!("bevelled volume {volume:.5}");
    assert!(volume < flat.area * 0.3 && volume > flat.area * 0.3 * 0.85);
}

#[test]
fn a_solid_is_closed_with_outward_normals() {
    for bevel in [0.0, 0.02] {
        let s = solid(&text("Bevel ag&8%", 0.25, bevel)).unwrap();
        let open = open_edges(&[&s.faces, &s.sides]);
        println!("bevel {bevel}: {} triangles, {open} open edges", (s.faces.indices.len() + s.sides.indices.len()) / 3);
        assert_eq!(open, 0, "bevel {bevel}");
        for part in [&s.faces, &s.sides] {
            for t in part.indices.chunks(3) {
                let [a, b, c] = [0, 1, 2].map(|i| bevy::math::Vec3::from(part.positions[t[i] as usize]));
                let face = (b - a).cross(c - a);
                let normal = bevy::math::Vec3::from(part.normals[t[0] as usize]);
                assert!(face.length() < 1e-9 || face.dot(normal) > 0.0, "a triangle faces away from its normal");
            }
        }
    }
}

#[test]
fn a_solid_centres_on_its_advance_cap_height_and_depth() {
    let s = solid(&text("HH", 0.4, 0.01)).unwrap();
    let positions = s.faces.positions.iter().chain(&s.sides.positions);
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in positions {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    println!("bounds {lo:?} to {hi:?}, advance {}", s.advance);
    // H stands on the baseline and reaches the cap height, with even side bearings.
    assert!((lo[1] + hi[1]).abs() < 1e-3, "cap height centred: {} {}", lo[1], hi[1]);
    assert!((lo[0] + hi[0]).abs() < 1e-3, "advance centred: {} {}", lo[0], hi[0]);
    assert!((lo[2] + 0.2).abs() < 1e-6 && (hi[2] - 0.2).abs() < 1e-6);
    assert!(hi[1] - lo[1] > 0.68 && hi[1] - lo[1] < 0.76, "Inter's cap height is 0.727 em");
}

#[test]
fn shaping_kerns_and_weights_pick_the_outline() {
    let pair = solid(&text("AV", 0.2, 0.0)).unwrap().advance;
    let apart = solid(&text("A", 0.2, 0.0)).unwrap().advance + solid(&text("V", 0.2, 0.0)).unwrap().advance;
    println!("AV {pair:.4} em against A + V {apart:.4} em");
    assert!(pair < apart - 0.02, "Inter kerns AV");
    let face = |weight: f32| {
        let mut t = text("Weight", 0.2, 0.0);
        t.font = font("Archivo-Variable.ttf");
        t.weight = weight;
        solid(&t).unwrap().area
    };
    let (thin, black) = (face(200.0), face(900.0));
    println!("Archivo area at 200 {thin:.4}, at 900 {black:.4}");
    assert!(black > thin * 1.5, "the wght axis follows the weight");
}

#[test]
fn empty_text_and_bad_fonts_fail() {
    assert!(solid(&text("   ", 0.2, 0.0)).unwrap_err().contains("no text"));
    assert!(Asset::font(b"not a font".to_vec(), None).is_err());
}
