# Purpose

- Paint and export frames of a `director_scene::SceneDocument` and of a live `Director`. This crate is the phase 3 renderer of `docs/plans/10-native-platform-overhaul.md` and the only frame path: core export, preview, stills, tests and the CLI all paint through it.

## Ownership

- `src/lib.rs` owns `Renderer`, `FramePool`, `FrameSession`, `Frame` and `RenderError`.
- `src/export.rs` owns `ExportOptions`, `Renderer::export`, `Renderer::export_with`, `Renderer::export_on`, `Renderer::export_options`, `Renderer::frame_times`, `export_director`, `frame_times` and the single export session that paints on a GPU context.
- `src/frame.rs` owns the frame driver. `FrameState` holds the layout engine and the accumulation and transition rasters across frames; `FrameState::render(director, time, target)` evaluates the `Director` (dirty set, update, layout, post-layout) into a private frame plan of scene layers and transition pairs, paints it through `paint::Painter`, and averages shutter samples for motion blur. `render_frame` paints one frame with a fresh state. `paint_root` paints one subtree on a full pass.
- `src/live.rs` owns the serial live export (`render_export` and `render_export_at_scale`, the parity and bench reference), the `GpuContext` alias (a Skia `DirectContext` under `vulkan`, `()` otherwise), and output scaling (`validate_scale`, `output_dimensions`, `set_output_scale`).
- `src/paint/mod.rs` owns the node traversal (`Painter`: transforms, blend, filter and mask layers, z-index order), static layer reuse and the debug overlays. `src/paint/depth.rs` owns preserve-3d depth sorting and per-pixel occlusion. `src/paint/kinds.rs` owns per-kind knowledge: which kinds and channels vary with time, and the kinds this crate paints itself.
- `tests/common/mod.rs` holds the parity helpers: a live `FrameSession` over the original `Director` is the reference for its exported document.
- `tests/rhai_examples.rs` paints every `examples/**/*.rhai` movie (each registered composition with default props) on a live `FrameSession` and from its exported document, and requires both to match.
- `tests/html_examples.rs` owns parity between the document render and the live compiled `Director` for HTML, including `export_director` at half scale with alpha against the live export: every example, a paint fixture, SMIL, looped video, a sampled capture with path and embedded video, text, SVG and nested boxes, and a Paper shader, plus byte equality of `render_frames` and a warm `FramePool` with fresh sessions, and per-frame MD5 equality of a document export with the live `render_export`. Video fixtures need FFmpeg on the path.
- `tests/incremental_render.rs` owns incremental frames through `FrameState`: dirty nodes, their ancestors and descendants re-evaluate, clean siblings skip, a clean static subtree rasterizes once, and a full frame updates every node.
- `examples/export_bench.rs` times a 1080p HTML export through the serial live `render_export` against `Renderer::export_with`, plus an encode-only floor (prepainted frames and premixed silence piped through the single-pass encoder), render-only, one-call `render_frames`, single-session and live-session splits. It writes `serial.mp4`, `director-render.mp4` and `encode-only.mp4` to its output folder. `DIRECTOR_BENCH_WORKERS` overrides the pool size.
- `tests/core_nodes.rs` owns the same parity for core Lottie (with an override image) and audio scenes: the document re-exports unchanged, the mix is sample-equal and frames match, including a spectrum visualizer and an audio binding.

## Local Contracts

- `Renderer` holds an `Arc<SceneDocument>`, the asset loader, an `ImportRegistry` (core kinds by default, `with_registry` for frontend kinds such as `director_html::register_import`) and a render mode (export by default).
- `Renderer::session` imports the document into a fresh `Director` through `director_core::scene_import`. A `FrameSession` owns that `Director`, a `FrameState` and its raster surface, and reuses them across frames.
- `FrameSession::render(time)` and `render_into(time, &mut Frame)` paint through `FrameState::render`, matching the live export for motion blur, transitions, track opacity and clip fades, output scale and incremental dirty frames. Transition and accumulation rasters match the target surface and allocate on first use. It returns RGBA8 at the document output size (`width * output_scale_x`, rounded): straight alpha for a transparent background, and a direct premultiplied read for opaque frames, whose bytes are identical.
- `render_into` reuses the frame's pixel buffer. Use it on every repeated paint: a fresh 8 MB buffer per 1080p frame costs more than painting a static scene.
- `FramePool` holds one lazily imported `FrameSession` per rayon worker and keeps them warm across calls, so each keeps its static layer cache and incremental frame state. `render_into(times, &mut Vec<Frame>)` resizes the frames to `times`, gives each worker one contiguous run, reuses every pixel buffer and returns the first error.
- `FrameSession` is `Send` because it exclusively owns its `Director`, layout engine and CPU raster surfaces; a pool drives each session from one worker at a time.
- `Renderer::pool_with(workers)` sizes a pool explicitly. `Renderer::pool_for(frames)` gives one session per `FRAMES_PER_SESSION` (15) frames, capped at half the rayon workers so x264 keeps cores.
- The static layer cache is `Director::static_cache`, so `Director::note_mutation` evicts edited nodes for live edits and each session keeps its cache across frames. A static node rasterizes its subtree with the local transform baked in and reuses it on a full pass while its `StaticCacheKey` matches (layout size, local transform, pivot, inherited opacity, and time when the subtree varies with time). An incremental pass reuses clean layers and never serves a dirty node.
- Compositions paint their nested timeline through `Painter` into `CompositionNode::surface_cache`, using the inner `Director::static_cache`. An element that returns a graph from `Element::sampled_graph` (HTML captures) paints that graph through `Painter` with a fresh cache. Every other kind paints through its own `Element::render`.
- `Director::debug_overlays` draws node outlines and `id z:` labels after every frame.
- `Renderer::export(path, mode)` writes H.264 with AAC (VP9 with Opus for a transparent document) in one FFmpeg process through `director_core::video_wrapper::Encoder::with_audio_input`. FFmpeg settings and export frame times come from the document alone (`Renderer::frame_times`, the document fps, size, transparency and audio sample rate); `tests/common` asserts `Renderer::frame_times` equals `frame_times` of the import for every parity scene. A scoped writer thread spawns FFmpeg, then imports a separate `Director`, mixes the whole track and sends it while a `pool_for` pool paints its first batch, then encodes each batch while the next paints; two frame batches circulate. A failed paint or write calls `Encoder::abort`, so nothing reaches `path`. `export_with` takes an explicit worker count. `export_options(path, options, gpu)` takes `ExportOptions` (encoder mode, draft, workers, a cancel flag checked before each batch, which ends the export with `RenderError::Cancelled` and leaves nothing at the path, and a progress callback called on the writer thread with encoded and total frames) and paints on `gpu` when given. `export_on(path, mode, context)` paints through one session on the calling thread, into a render target of a Vulkan `GpuContext` (flushed before each readback) or a CPU raster for `None`, sending single frames with four in flight; `document_export_decodes_like_the_live_export` checks its raster form against the live export.
- `export_director(director, registry, path, alpha, scale, draft, gpu)` exports a live `Director` through its scene document and the same pipeline, with the raster scale, transparent background and draft x264 of `render_export_at_scale`. A `gpu` context paints through `export_on`; `None` uses a `pool_for` pool. It checks required fonts, imports with the caller's registry, and restores the output scale and background. The CLI exports every render through it.
- `Renderer::render_frames(times)` paints through a fresh `FramePool` and returns frames in the order of `times`. Warm pool frames equal fresh-session frames byte for byte.
- `FrameSession` and `FramePool` paint on the CPU raster backend. `export_on` and the live export render on a Vulkan target when the caller supplies a `GpuContext` (`vulkan` feature), flushing before each readback.

## Work Guidance

- CPU export is bound by x264: piping frames and the final lookahead drain take about 330 ms of a 3 s 1080p static export. GPU export is bound by the synchronous Vulkan readback, about 3 ms per 1080p frame under x264 load. skia-safe 0.153 exposes no async readback (`AsyncReadResult`), so a faster GPU export needs a smaller readback (GPU YUV conversion) or several contexts.
- `director-core` dev-depends on this crate, so core integration tests paint through it. Core `#[cfg(test)]` unit tests cannot; put frame tests in this crate's `tests/`.

## Verification

- `cargo test -p director-render --release` (parity tests, incremental frames, and `paint::tests` for layer reuse)
- `cargo test -p director-core --release --test visual_suite` paints every golden document through `Renderer` and compares it with the golden.

## Child DOX Index

- None.
