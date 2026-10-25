# Purpose

- Paint frames of a `director_scene::SceneDocument`. This crate is the phase 3 renderer of `docs/plans/10-native-platform-overhaul.md`.

## Ownership

- `src/lib.rs` owns `Renderer`, `FramePool`, `FrameSession`, `Frame` and `RenderError`.
- `src/export.rs` owns `Renderer::export`, `Renderer::export_with` and `frame_times`.
- `src/frame.rs` owns the frame driver: `evaluate` advances the `Director` (dirty set, update, layout, post-layout) and resolves a `FramePlan` of scene layers and transition pairs; `paint` draws a plan through `paint::Painter`; `render_sampled` averages shutter samples for motion blur.
- `src/paint/mod.rs` owns the node traversal (`Painter`: transforms, blend, filter and mask layers, z-index order) and the static layer cache (`LayerCache`). `src/paint/depth.rs` owns preserve-3d depth sorting and per-pixel occlusion. `src/paint/kinds.rs` owns per-kind knowledge: which kinds and channels vary with time, and the kinds this crate paints itself.
- `tests/common/mod.rs` holds the parity reference: `CoreSession` paints a `Director` through core `render_sampled_at_time`, and every parity test compares this crate against it.
- `tests/rhai_examples.rs` paints every `examples/**/*.rhai` movie (each registered composition with default props) on the core path, on a live `FrameSession` and from its exported document, and requires all three to match.
- `tests/html_examples.rs` owns parity between the document render and the compiled `Director` on the core path for HTML: every example, a paint fixture, SMIL, looped video, a sampled capture with path and embedded video, text, SVG and nested boxes, and a Paper shader, plus byte equality of `render_frames` and a warm `FramePool` with fresh sessions, and per-frame MD5 equality of a document export with the core export. Video fixtures need FFmpeg on the path.
- `examples/export_bench.rs` times a 1080p HTML export on the core path against `Renderer::export_with`, plus an encode-only floor (prepainted frames and premixed silence piped through the single-pass encoder), render-only, one-call `render_frames`, single-session and live-session splits. It writes `core.mp4`, `director-render.mp4` and `encode-only.mp4` to its output folder. `DIRECTOR_BENCH_WORKERS` overrides the pool size.
- `tests/core_nodes.rs` owns the same parity for core Lottie (with an override image) and audio scenes: the document re-exports unchanged, the mix is sample-equal and frames match, including a spectrum visualizer and an audio binding.

## Local Contracts

- `Renderer` holds an `Arc<SceneDocument>`, the asset loader, an `ImportRegistry` (core kinds by default, `with_registry` for frontend kinds such as `director_html::register_import`) and a render mode (export by default).
- `Renderer::session` imports the document into a fresh `Director` through `director_core::scene_import`. A `FrameSession` owns that `Director`, its layout engine and raster surfaces, and reuses them across frames.
- `FrameSession::render(time)` and `render_into(time, &mut Frame)` paint through `frame::render_sampled`, matching core export for motion blur, transitions, track opacity and clip fades, output scale and incremental dirty frames. Transition and accumulation rasters match the target surface and allocate on first use. It returns RGBA8 at the document output size (`width * output_scale_x`, rounded): straight alpha for a transparent background, and a direct premultiplied read for opaque frames, whose bytes are identical.
- `render_into` reuses the frame's pixel buffer. Use it on every repeated paint: a fresh 8 MB buffer per 1080p frame costs more than painting a static scene.
- `FramePool` holds one lazily imported `FrameSession` per rayon worker and keeps them warm across calls, so each keeps its static layer cache and incremental frame state. `render_into(times, &mut Vec<Frame>)` resizes the frames to `times`, gives each worker one contiguous run, reuses every pixel buffer and returns the first error.
- `FrameSession` is `Send` because it exclusively owns its `Director`, layout engine and CPU raster surfaces; a pool drives each session from one worker at a time.
- `Renderer::pool_with(workers)` sizes a pool explicitly. `Renderer::pool_for(frames)` gives one session per `FRAMES_PER_SESSION` (15) frames, capped at half the rayon workers so x264 keeps cores.
- `Painter` paints without core `render_recursive`. Each `FrameSession` owns one `LayerCache` across frames. A static node rasterizes its subtree with the local transform baked in and reuses it on a full pass while its `StaticCacheKey` matches (layout size, local transform, pivot, inherited opacity, and time when the subtree varies with time). An incremental pass reuses clean layers and never serves a dirty node.
- Compositions paint their nested timeline through `Painter`, with a nested `LayerCache` and surface kept per composition node. Every other kind paints through its own `Element::render`; HTML captures still traverse their sampled graph through core `render_recursive`.
- `Renderer::export(path, mode)` writes H.264 with AAC (VP9 with Opus for a transparent document) in one FFmpeg process through `director_core::video_wrapper::Encoder::with_audio`, with export frame times from `frame_times`. A scoped writer thread mixes the whole track from a separately imported `Director` and spawns FFmpeg while a `pool_for` pool paints its first batch, then encodes each batch while the next paints; two frame batches circulate. A failed paint or write calls `Encoder::abort`, so nothing reaches `path`. `export_with` takes an explicit worker count.
- `Renderer::render_frames(times)` paints through a fresh `FramePool` and returns frames in the order of `times`. Warm pool frames equal fresh-session frames byte for byte.
- The renderer paints on the CPU raster backend.

## Work Guidance

- Export is bound by x264: piping frames and the final lookahead drain take about 380 ms of a 3 s 1080p static export, and FFmpeg starts after the audio import and mix. Further gains come from starting FFmpeg sooner or from a faster encoder preset, ahead of paint tuning.
- The frame driver, traversal and layer cache live here and mirror the core path, which stays the parity reference. Keep `paint` and core `systems::renderer` in step until the core frame path is deleted. Next: switch core export, preview and the CLI to this crate, give captures a nested-graph hook so they paint through `Painter`, then delete the core frame path.

## Verification

- `cargo test -p director-render --release` (parity tests plus `paint::tests` for layer reuse)
- `cargo test -p director-core --release --test visual_suite` paints every golden document through `Renderer` and compares it with the golden.

## Child DOX Index

- None.
