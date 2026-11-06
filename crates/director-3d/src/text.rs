//! Extruded type: one line of text as a solid mesh with chamfered edges.
//!
//! Skia shapes the line (kerning and ligatures through HarfBuzz), gives each
//! glyph's outline and merges overlapping contours through its path ops.
//! Each contour is flattened to a polygon within `TOLERANCE` of the curve
//! and turned so the filled side lies on its left. Lyon triangulates the
//! front and back faces from the contours moved inward by the bevel, and
//! quads join them to the outline around the sides: a 45 degree chamfer at
//! each face and a straight wall between. Normals stay smooth across curves
//! and split at corners sharper than `CREASE` degrees.

use std::hash::{Hash, Hasher};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use lyon_tessellation::{geom::point, path::Path as Outline, BuffersBuilder, FillOptions, FillTessellator, FillVertex, VertexBuffers};
use skia_safe::shaper::run_handler::{Buffer, RunInfo};
use skia_safe::shaper::RunHandler;
use skia_safe::{path::Verb, Font, FontHinting, FontMgr, GlyphId, Path, PathBuilder, Point, Shaper};

use crate::node::Asset;

/// Font size Skia shapes and outlines at, in its units.
const EM: f32 = 256.0;
/// Most distance a flattened curve strays from the outline, as a fraction of the em.
const TOLERANCE: f32 = 0.0015;
/// Corners sharper than this many degrees keep split normals.
const CREASE: f32 = 35.0;

/// A line of extruded type placed in a model's world space.
#[derive(Clone, Debug)]
pub struct Text3d {
    /// The TrueType or OpenType file the glyphs come from.
    pub font: Asset,
    pub text: String,
    /// CSS font weight, set on the font's `wght` axis when it has one.
    pub weight: f32,
    /// CSS font stretch in percent, set on the font's `wdth` axis when it has one.
    pub stretch: f32,
    /// Em height in metres.
    pub size: f32,
    /// Metres from the front face to the back face.
    pub depth: f32,
    /// Metres each chamfer cuts into the face and down the side.
    pub bevel: f32,
    /// Metres along X, Y and Z.
    pub position: [f32; 3],
    /// Degrees about X, then Y, then Z.
    pub rotation: [f32; 3],
    /// sRGB colour of the front and back faces, each channel 0 to 1.
    pub color: [f32; 3],
    /// sRGB colour of the sides and chamfers. `None` takes `color`.
    pub side_color: Option<[f32; 3]>,
    pub metallic: f32,
    pub roughness: f32,
}

/// A hash of every text's font, string and placement, which keys the
/// studio's stages and frames.
pub(crate) fn texts_hash(texts: &[Text3d]) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    for t in texts {
        t.font.hash.hash(&mut hasher);
        t.text.hash(&mut hasher);
        let numbers = [t.weight, t.stretch, t.size, t.depth, t.bevel, t.metallic, t.roughness]
            .into_iter()
            .chain(t.position)
            .chain(t.rotation)
            .chain(t.color)
            .chain(t.side_color.into_iter().flatten());
        for n in numbers {
            n.to_bits().hash(&mut hasher);
        }
        t.side_color.is_some().hash(&mut hasher);
    }
    hasher.finish()
}

/// Triangles with one normal per vertex, in the text's own metres.
#[derive(Clone, Debug, Default)]
pub struct Part {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl Part {
    /// Adds a triangle wound counter-clockwise as seen from the side its
    /// normals face.
    fn triangle(&mut self, corners: [([f32; 3], [f32; 3]); 3]) {
        let [a, b, c] = corners.map(|(p, _)| glam(p));
        let facing = (b - a).cross(c - a).dot(glam(corners[0].1) + glam(corners[1].1) + glam(corners[2].1));
        let order = if facing < 0.0 { [0, 2, 1] } else { [0, 1, 2] };
        for i in order {
            self.indices.push(self.positions.len() as u32);
            self.positions.push(corners[i].0);
            self.normals.push(corners[i].1);
        }
    }

    fn quad(&mut self, corners: [([f32; 3], [f32; 3]); 4]) {
        self.triangle([corners[0], corners[1], corners[2]]);
        self.triangle([corners[0], corners[2], corners[3]]);
    }

    /// The part as a Bevy triangle list with planar UVs.
    pub(crate) fn mesh(&self, size: f32) -> Mesh {
        let uvs: Vec<[f32; 2]> = self.positions.iter().map(|p| [p[0] / size, -p[1] / size]).collect();
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.clone());
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        mesh.insert_indices(Indices::U32(self.indices.clone()));
        mesh
    }

    /// Volume enclosed by the triangles, positive for outward normals.
    pub fn volume(&self) -> f32 {
        self.indices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [0, 1, 2].map(|i| glam(self.positions[t[i] as usize]));
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }
}

/// Extruded type: the front and back faces, and the sides with their chamfers.
#[derive(Clone, Debug, Default)]
pub struct Solid {
    pub faces: Part,
    pub sides: Part,
    /// Area of the glyph outlines in square metres.
    pub area: f32,
    /// Advance width of the line in metres.
    pub advance: f32,
}

fn glam(p: [f32; 3]) -> bevy::math::Vec3 {
    bevy::math::Vec3::from(p)
}

/// Collects shaped glyphs and their pen positions, run by run.
#[derive(Default)]
struct Runs {
    glyphs: Vec<GlyphId>,
    positions: Vec<Point>,
    pen: Point,
    out: Vec<(Font, Vec<GlyphId>, Vec<Point>)>,
}

impl RunHandler for Runs {
    fn begin_line(&mut self) {}
    fn run_info(&mut self, _: &RunInfo) {}
    fn commit_run_info(&mut self) {}
    fn run_buffer(&mut self, info: &RunInfo) -> Buffer<'_> {
        self.glyphs = vec![0; info.glyph_count];
        self.positions = vec![Point::default(); info.glyph_count];
        Buffer::new(&mut self.glyphs, &mut self.positions, self.pen)
    }
    fn commit_run_buffer(&mut self, info: &RunInfo) {
        self.out.push((info.font.clone(), std::mem::take(&mut self.glyphs), std::mem::take(&mut self.positions)));
        self.pen += info.advance;
    }
    fn commit_line(&mut self) {}
}

/// The outline of `text` at `EM` in Skia's y-down units with overlaps
/// merged, its advance width and the font's cap height.
fn outline(text: &Text3d) -> Result<(Path, f32, f32), String> {
    let typeface = FontMgr::new()
        .new_from_data(skia_safe::Data::new_copy(&text.font.bytes), None)
        .ok_or("the font file is not decodable by Skia")?;
    let typeface = director_core::node::text::vary_typeface(
        typeface,
        &[
            director_core::element::FontVariation { axis: "wght".into(), value: text.weight },
            director_core::element::FontVariation { axis: "wdth".into(), value: text.stretch },
        ],
    );
    let mut font = Font::from_typeface(typeface, EM);
    font.set_hinting(FontHinting::None);
    font.set_linear_metrics(true);
    font.set_subpixel(true);
    let line: String = text.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return Err("text3d has no text".into());
    }
    let mut runs = Runs::default();
    Shaper::new(FontMgr::new()).shape(&line, &font, true, f32::MAX, &mut runs);
    let mut builder = PathBuilder::new();
    for (font, glyphs, positions) in &runs.out {
        for (glyph, at) in glyphs.iter().zip(positions) {
            if let Some(path) = font.get_path(*glyph) {
                builder.add_path_with_offset(&path, *at, None);
            }
        }
    }
    let path = builder.detach();
    let path = path.simplify().unwrap_or(path);
    let cap = font.metrics().1.cap_height;
    let cap = if cap > 0.0 { cap } else { -font.metrics().1.ascent * 0.7 };
    Ok((path, runs.pen.x, cap))
}

/// Flattens `path` into closed polygons within `tolerance` of each curve.
fn flatten(path: &Path, tolerance: f32) -> Vec<Vec<Point>> {
    fn steps(bend: f32, tolerance: f32) -> usize {
        ((bend / tolerance).sqrt().ceil() as usize).clamp(1, 64)
    }
    let mut contours = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let mut iter = skia_safe::path::Iter::new(path, true);
    while let Some((verb, p)) = iter.next() {
        match verb {
            Verb::Move => {
                contours.push(std::mem::take(&mut current));
                current.push(p[0]);
            }
            Verb::Line => current.push(p[1]),
            Verb::Quad | Verb::Conic => {
                let w = if verb == Verb::Conic { iter.conic_weight().unwrap_or(1.0) } else { 1.0 };
                let n = steps((p[0] - p[1] * 2.0 + p[2]).length() * w.max(1.0) / 4.0, tolerance);
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let (a, b, c) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * w, t * t);
                    current.push((p[0] * a + p[1] * b + p[2] * c) * (1.0 / (a + b + c)));
                }
            }
            Verb::Cubic => {
                let bend = (p[0] - p[1] * 2.0 + p[2]).length().max((p[1] - p[2] * 2.0 + p[3]).length());
                let n = steps(bend * 0.75, tolerance);
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    current.push(p[0] * (u * u * u) + p[1] * (3.0 * u * u * t) + p[2] * (3.0 * u * t * t) + p[3] * (t * t * t));
                }
            }
            Verb::Close => contours.push(std::mem::take(&mut current)),
            _ => {}
        }
    }
    contours.push(current);
    let mut out = Vec::new();
    for contour in contours {
        let mut clean: Vec<Point> = Vec::with_capacity(contour.len());
        for p in contour {
            if clean.last().is_none_or(|q| (p - *q).length() > tolerance * 0.01) {
                clean.push(p);
            }
        }
        while clean.len() > 1 && (clean[0] - clean[clean.len() - 1]).length() <= tolerance * 0.01 {
            clean.pop();
        }
        if clean.len() >= 3 && signed_area(&clean).abs() > tolerance * tolerance {
            out.push(clean);
        }
    }
    out
}

/// Twice the signed area, positive for a counter-clockwise loop in y-up axes.
fn signed_area(points: &[Point]) -> f32 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f32>()
        / 2.0
}

/// The interior normal of the edge from `a` to `b`: its left side.
fn left(a: Point, b: Point) -> Point {
    let d = b - a;
    let l = d.length().max(1e-12);
    Point::new(-d.y / l, d.x / l)
}

/// Builds the solid of `text` in metres, centred on its advance width, its
/// cap height and its depth.
pub fn solid(text: &Text3d) -> Result<Solid, String> {
    let (path, advance, cap) = outline(text)?;
    let scale = text.size / EM;
    let (cx, cy) = (advance / 2.0, cap / 2.0);
    // Polygons in y-up metres about the centre, each with its filled side on its left.
    let mut loops: Vec<Vec<Point>> = Vec::new();
    for contour in flatten(&path, TOLERANCE * EM) {
        let mut poly: Vec<Point> = contour.iter().map(|p| Point::new((p.x - cx) * scale, (-p.y - cy) * scale)).collect();
        if !fill_on_left(&path, &poly, scale, cx, cy) {
            poly.reverse();
        }
        loops.push(poly);
    }
    if loops.is_empty() {
        return Err(format!("text3d '{}' has no visible glyphs in its font", text.text));
    }
    let area: f32 = loops.iter().map(|l| signed_area(l)).sum();
    let half = text.depth / 2.0;
    let bevel = text.bevel.clamp(0.0, half);
    let crease = CREASE.to_radians().cos();
    let mut solid = Solid {
        area,
        advance: advance * scale,
        ..Solid::default()
    };
    let mut insets = Vec::with_capacity(loops.len());
    for poly in &loops {
        let n = poly.len();
        // Outward normals of each edge, and each vertex's normal on either side.
        let outward: Vec<Point> = (0..n).map(|i| -left(poly[i], poly[(i + 1) % n])).collect();
        let smooth = |i: usize, edge: usize| -> Point {
            let (before, after) = (outward[(i + n - 1) % n], outward[i % n]);
            if before.dot(after) >= crease {
                let m = before + after;
                m * (1.0 / m.length().max(1e-12))
            } else {
                outward[edge]
            }
        };
        let inset: Vec<Point> = (0..n)
            .map(|i| {
                let (n0, n1) = (-outward[(i + n - 1) % n], -outward[i]);
                let m = n0 + n1;
                let m = if m.length() < 1e-6 { n0 } else { m * (1.0 / m.length()) };
                poly[i] + m * (bevel / m.dot(n0).max(0.25))
            })
            .collect();
        for i in 0..n {
            let j = (i + 1) % n;
            let (na, nb) = (smooth(i, i), smooth(j, i));
            let flat = |p: Point, z: f32| [p.x, p.y, z];
            let tilt = |o: Point, z: f32| {
                let v = glam([o.x, o.y, z]).normalize();
                [v.x, v.y, v.z]
            };
            let wall = |o: Point| [o.x, o.y, 0.0];
            if bevel > 0.0 {
                solid.sides.quad([
                    (flat(inset[i], half), tilt(na, 1.0)),
                    (flat(poly[i], half - bevel), tilt(na, 1.0)),
                    (flat(poly[j], half - bevel), tilt(nb, 1.0)),
                    (flat(inset[j], half), tilt(nb, 1.0)),
                ]);
                solid.sides.quad([
                    (flat(poly[i], bevel - half), tilt(na, -1.0)),
                    (flat(inset[i], -half), tilt(na, -1.0)),
                    (flat(inset[j], -half), tilt(nb, -1.0)),
                    (flat(poly[j], bevel - half), tilt(nb, -1.0)),
                ]);
            }
            if half - bevel > 0.0 {
                solid.sides.quad([
                    (flat(poly[i], half - bevel), wall(na)),
                    (flat(poly[i], bevel - half), wall(na)),
                    (flat(poly[j], bevel - half), wall(nb)),
                    (flat(poly[j], half - bevel), wall(nb)),
                ]);
            }
        }
        insets.push(inset);
    }
    let mut builder = Outline::builder();
    for inset in &insets {
        builder.begin(point(inset[0].x, inset[0].y));
        for p in &inset[1..] {
            builder.line_to(point(p.x, p.y));
        }
        builder.end(true);
    }
    let shape = builder.build();
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &shape,
            &FillOptions::non_zero().with_tolerance(TOLERANCE * text.size),
            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| v.position().to_array()),
        )
        .map_err(|e| format!("text3d '{}' could not be triangulated: {e:?}", text.text))?;
    for t in buffers.indices.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|i| buffers.vertices[t[i] as usize]);
        for (z, nz) in [(half, 1.0), (-half, -1.0)] {
            solid.faces.triangle([a, b, c].map(|p| ([p[0], p[1], z], [0.0, 0.0, nz])));
        }
    }
    Ok(solid)
}

/// Whether the filled side of `poly` lies on its left, voted by the
/// outline's own fill rule just inside its three longest edges.
fn fill_on_left(path: &Path, poly: &[Point], scale: f32, cx: f32, cy: f32) -> bool {
    let n = poly.len();
    let mut edges: Vec<usize> = (0..n).collect();
    edges.sort_by(|&a, &b| {
        let la = (poly[(a + 1) % n] - poly[a]).length();
        let lb = (poly[(b + 1) % n] - poly[b]).length();
        lb.total_cmp(&la)
    });
    let mut votes = 0i32;
    for &i in edges.iter().take(3) {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let step = ((b - a).length() * 0.25).min(TOLERANCE * EM * scale * 2.0);
        let probe = (a + b) * 0.5 + left(a, b) * step;
        // Back to Skia's y-down units at `EM`.
        let skia = Point::new(probe.x / scale + cx, -probe.y / scale - cy);
        votes += if path.contains(skia) { 1 } else { -1 };
    }
    votes > 0
}
