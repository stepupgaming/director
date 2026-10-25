use criterion::{criterion_group, criterion_main, Criterion};
use director_core::animation::{Animated, EasingType};
use director_core::node::BoxNode;
use director_core::render_manifest::{build_render_manifest, layout_snapshot};
use director_core::systems::layout::LayoutEngine;
use director_render::{paint_root, FrameState};
use director_core::types::{Color, NodeId};
use director_core::video_wrapper::RenderMode;
use director_core::{DefaultAssetLoader, Director};
use skia_safe::{AlphaType, ColorSpace, ColorType, ImageInfo};
use std::sync::Arc;
use std::time::{Duration, Instant};
use taffy::prelude::*;

/// Build a representative Director scene with a root node and a modest tree
/// of children so the benchmark exercises layout and node iteration.
fn build_director(node_count: usize) -> Director {
    let mut director = Director::new(
        1080,
        1920,
        30.0,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    );

    let root = director.scene.add_node(Box::new(BoxNode::new()));
    director.push_scene_root_at(root, 0.0, 10.0, Some("main".into()));

    // Add a flat hierarchy of child boxes nodes under the root.
    for _ in 0..node_count {
        let child = director.scene.add_node(Box::new(BoxNode::new()));
        let _ = director.scene.try_add_child(root, child);
    }

    director
}

fn bench_layout_snapshot_vs_clone(c: &mut Criterion) {
    let node_counts = [10, 50, 100];

    for count in node_counts {
        let group_name = format!("nodes_{}", count);
        let mut group = c.benchmark_group(&group_name);

        // Lightweight layout_snapshot path.
        group.bench_function("layout_snapshot", |b| {
            let mut director = build_director(count);
            b.iter(|| {
                let _snapshot = layout_snapshot(&mut director, 0.5);
            })
        });

        // Expensive clone + full render manifest path.
        group.bench_function("clone_and_build_render_manifest", |b| {
            let director = build_director(count);
            b.iter(|| {
                let mut cloned = director.clone();
                let _manifest = build_render_manifest(&mut cloned, 0.5);
            })
        });

        group.finish();
    }
}

/// Colored card grids model dashboards and dense infographic scenes without
/// introducing font, video, or GPU variability into the timing comparison.
fn build_frame_scene(side: usize, animated: bool, cached: bool) -> (Director, NodeId) {
    let mut director = Director::new(
        640,
        360,
        30.0,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    );
    let mut root = BoxNode::new();
    root.style.size = Size {
        width: Dimension::percent(1.0),
        height: Dimension::percent(1.0),
    };
    root.style.flex_direction = FlexDirection::Column;
    let root_id = director.scene.add_node(Box::new(root));
    director.push_scene_root(root_id, 10000.0);
    for row in 0..side {
        let mut strip = BoxNode::new();
        strip.style.size = Size {
            width: Dimension::percent(1.0),
            height: Dimension::length(360.0 / side as f32),
        };
        let strip_id = director.scene.add_node(Box::new(strip));
        director.scene.add_child(root_id, strip_id);
        director.scene.get_node_mut(strip_id).unwrap().is_static = cached;
        for col in 0..side {
            let mut card = BoxNode::new();
            card.style.size = Size {
                width: Dimension::length(640.0 / side as f32),
                height: Dimension::percent(1.0),
            };
            card.bg_color = Some(Animated::new(Color::new(
                (col + 1) as f32 / (side + 1) as f32,
                (row + 1) as f32 / (side + 1) as f32,
                0.4,
                1.0,
            )));
            let id = director.scene.add_node(Box::new(card));
            director.scene.add_child(strip_id, id);
            if animated {
                director
                    .scene
                    .get_node_mut(id)
                    .unwrap()
                    .transform
                    .translate_x
                    .add_segment(0.0, 3.0, 3.0, EasingType::Linear);
            }
        }
    }
    director.compile();
    (director, root_id)
}

fn frame_surface() -> skia_safe::Surface {
    skia_safe::surfaces::raster(
        &ImageInfo::new(
            (640, 360),
            ColorType::RGBA8888,
            AlphaType::Premul,
            Some(ColorSpace::new_srgb()),
        ),
        None,
        None,
    )
    .expect("profile surface")
}

fn record_profile_pixels(name: &str, side: usize, animated: bool, cached: bool) {
    let Some(output_dir) = std::env::var_os("DIRECTOR_PROFILE_PIXELS") else {
        return;
    };
    let dir = std::path::PathBuf::from(output_dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (mut director, _) = build_frame_scene(side, animated, cached);
    let mut state = FrameState::default();
    let mut surface = frame_surface();
    let info = surface.image_info();
    for (index, time) in [0.0, 0.5, 1.0, 2.0, 3.0].into_iter().enumerate() {
        state.render(&mut director, time, &mut surface).unwrap();
        let mut pixels = vec![0; 640 * 360 * 4];
        assert!(surface.read_pixels(&info, &mut pixels, 640 * 4, (0, 0)));
        std::fs::write(dir.join(format!("{name}_{index}.rgba")), pixels).unwrap();
    }
}

fn bench_frame_pipeline(c: &mut Criterion) {
    let workloads = [
        ("cards_144", 12, false, false),
        ("animated_1024", 32, true, false),
        ("static_cached_1024", 32, false, true),
    ];
    if let Some(output_dir) = std::env::var_os("DIRECTOR_PROFILE_PIXELS") {
        let dir = std::path::PathBuf::from(output_dir);
        std::fs::create_dir_all(&dir).unwrap();
        let suite = serde_json::json!({
            "version": 1,
            "width": 640,
            "height": 360,
            "pixel_format": "rgba8888-premul-srgb",
            "fps": 30,
            "times": [0.0, 0.5, 1.0, 2.0, 3.0],
            "phases": ["update", "layout", "paint", "whole_frame"],
            "sample_size": 20,
            "warmup_seconds": 1,
            "measurement_seconds": 3,
            "workloads": workloads.iter().map(|(name, side, animated, cached)| {
                serde_json::json!({"name": name, "side": side, "animated": animated, "cached": cached})
            }).collect::<Vec<_>>()
        });
        std::fs::write(
            dir.join("suite.json"),
            serde_json::to_vec_pretty(&suite).unwrap(),
        )
        .unwrap();
    }
    for (name, side, animated, cached) in workloads {
        record_profile_pixels(name, side, animated, cached);
        let mut group = c.benchmark_group(format!("frame_pipeline/{name}"));
        group.sample_size(20);
        group.warm_up_time(Duration::from_secs(1));
        group.measurement_time(Duration::from_secs(3));
        for phase in ["update", "layout", "paint", "whole_frame"] {
            let (mut director, root) = build_frame_scene(side, animated, cached);
            let mut layout = LayoutEngine::new();
            let mut state = FrameState::default();
            let mut surface = frame_surface();
            state.render(&mut director, 0.5, &mut surface).unwrap();
            layout.compute_layout(&mut director.scene, 640, 360, 0.5);
            group.bench_function(phase, |b| {
                b.iter_custom(|iterations| {
                    let mut total = Duration::ZERO;
                    for iteration in 0..iterations {
                        let time = (iteration % 90) as f64 / 30.0;
                        if phase != "update" && phase != "whole_frame" {
                            director.update(time, None);
                        }
                        if phase == "paint" {
                            layout.compute_layout(&mut director.scene, 640, 360, time);
                            director.run_post_layout(time);
                            // The full-frame path invalidates every static entry today.
                            director.static_cache.clear();
                            surface.canvas().clear(skia_safe::Color::BLACK);
                        }
                        let start = Instant::now();
                        match phase {
                            "update" => director.update(time, None),
                            "layout" => layout.compute_layout(&mut director.scene, 640, 360, time),
                            "paint" => paint_root(&mut director, root, surface.canvas(), 1.0).unwrap(),
                            "whole_frame" => state.render(&mut director, time, &mut surface).unwrap(),
                            _ => unreachable!(),
                        }
                        total += start.elapsed();
                    }
                    total
                });
            });
        }
        group.finish();
    }
}

criterion_group!(
    benches,
    bench_layout_snapshot_vs_clone,
    bench_frame_pipeline
);
criterion_main!(benches);
