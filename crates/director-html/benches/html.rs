use criterion::{black_box, criterion_group, criterion_main, Criterion};
use director_html::{compile, CompileOptions};
const SOURCE: &str = include_str!("../../../examples/html/native-motion.html");
fn options() -> CompileOptions {
    CompileOptions {
        base_dir: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/html"),
        ..Default::default()
    }
}
fn html(c: &mut Criterion) {
    c.bench_function("html/compile_1080p", |b| {
        b.iter(|| black_box(compile(black_box(SOURCE), options()).unwrap()))
    });
    let mut compiled = compile(SOURCE, options()).unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((1920, 1080)).unwrap();
    let mut layout = director_core::systems::layout::LayoutEngine::new();
    let mut surfaces = None;
    let mut frame = 0;
    c.bench_function("html/native_frame_1080p", |b| {
        b.iter(|| {
            let time = (frame % 90) as f64 / 30.0;
            director_core::systems::renderer::render_at_time(
                &mut compiled.director,
                &mut layout,
                time,
                surface.canvas(),
                &mut surfaces,
            )
            .unwrap();
            frame += 1;
            black_box(surface.canvas());
        })
    });
}
criterion_group!(benches, html);
criterion_main!(benches);
