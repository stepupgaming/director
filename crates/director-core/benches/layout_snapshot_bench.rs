use criterion::{criterion_group, criterion_main, Criterion};
use director_core::render_manifest::{build_render_manifest, layout_snapshot};
use director_core::node::BoxNode;
use director_core::video_wrapper::RenderMode;
use director_core::{DefaultAssetLoader, Director};
use std::sync::Arc;

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

criterion_group!(benches, bench_layout_snapshot_vs_clone);
criterion_main!(benches);
