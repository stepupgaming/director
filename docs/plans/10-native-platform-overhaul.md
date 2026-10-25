# Plan 10: Native platform overhaul

> **Status:** In progress. The phase 1 gate is green locally. The phase 2 gate is green: the director-core golden corpus and the HTML examples and fixtures round-trip through the scene document. Phase 3 has started: `director-core::scene_import` loads a document back into a `Director`, every golden scene renders from its document identically, and `director-render` paints document frames, matching the compiled HTML examples, SMIL, looped video, sampled captures, Paper shaders, Lottie and spectrum scenes within the parity tolerance. Document audio mixes sample-equal to the live mixer. `director-render` owns the frame driver (evaluation into a frame plan, paint, shutter sampling and transitions) and matches the core path live and from the document across all 59 Rhai examples, the HTML examples and every golden scene.
> **Product direction:** Exceed HyperFrames, FFrames and Remotion with a library-first native video platform: HTML/CSS authoring that compiles to a typed scene document, a parallel deterministic renderer, one host session API shared by the CLI, agents and a future editor, and measured proof of speed and quality.

This file is the current-state and handoff document for the active phase. It replaces [plan 09](09-native-fidelity.md) as the active plan. Feature contracts for HTML live in [`crates/director-html/AGENTS.md`](../../crates/director-html/AGENTS.md).

## Why

A seven-part review on 2026-10-19 found working parts inside a weak structure:

- One mutable `Director` owns scene, timeline, media, caches and export. Frames render serially, and editors or servers have no session object to hold.
- The static-subtree cache never fires on HTML scenes. Decode and encode stream raw RGBA through FFmpeg pipes, and video seek is keyframe-inaccurate.
- Seven authoring formats compete. Rhai exposes about 250 functions with a catalog of about 30, and misspelled effects, transitions and properties fail silently.
- `director-html` stringifies typed Lightning CSS values and reparses them in a 2,400-line `match`.
- The repo has no golden images, clippy is red, CI runs only by hand, and benchmarks measure box grids.

Director's defensible advantages: it is the only native engine that compiles HyperFrames-style HTML, edits need no compile step, and output is browser-free and deterministic. FFrames already matches the agent CLI, so agent tooling alone does not differentiate.

## Target architecture

Build new crates beside the current core, port features over, then delete the old `Director` (strangler migration).

| Layer | Crate | Owns |
| --- | --- | --- |
| Scene document | `director-scene` | Serializable scene data with no Skia: a node data enum, property channels and keyframes, one effect and transition table, asset references, generated JSON schema, patch by stable id. |
| Frontends | `director-html`, timeline JSON, optional Rhai | Compile into the scene document. HTML/CSS is the primary front door. GSAP timelines compile to native keyframe tracks. Captures become an import path. Rhai becomes an optional generator over the host API. |
| Renderer | `director-render` | Pure `(scene, time) -> display list`, Skia paint on worker-owned surfaces, a content-hash layer cache that works for HTML, a media frame server with accurate seek, and an encoder path without full RGBA round trips. |
| Host session | `director-host` | Open, patch, inspect, render frame, export, revision and cancellation. The CLI, an MCP server, a future Tauri editor and serverless workers link it. |

Keep and port: the track timeline model, generational node ids, pure animation evaluation, layout-cache keys, required-font checks, the caption engine, active-speaker reframing, and the director-html parsing, cascade and paint knowledge.

Freeze: Lottie (no new features; ThorVG is the option if Lottie becomes headline), SMIL beyond what Hyper CN needs, and the Paper Shader translator.

## Phases

Each phase ends with its gate green and a commit. Phases 2 to 4 run in order; the typed computed style in phase 5 may start once phase 1 lands.

### Phase 1: Safety net and pruning

- Delete `director-pipeline`, `specs/`, `WgpuNode`, the `preview_frame`, `contact_sheet`, `verify_lottie` and `review_visuals` bins, the browser player page, HTTP fetch inside scripts, and examples that only demo removed or gimmick features.
- Fix the example walker: commit or remove missing assets, remove live network access and machine-specific paths, and stop parallel tests from racing on `set_current_dir`.
- Fail loud: unknown effects, transitions and properties return errors. Generate the authoring catalog from the registered surface.
- Commit a golden corpus of about 40 scene frames with a perceptual diff and fonts that resolve regardless of working directory.
- Make clippy green, run CI on push with a Linux job, and remove dead dependencies and the empty `mock_video` feature.
- Fold in the plan 09 close-out re-runs listed under Stale checks.

Gate: `cargo test --workspace --release` and `cargo clippy --workspace --all-targets --release -- -D warnings` pass locally and in CI, and the golden corpus fails on a deliberate one-pixel change.

### Phase 2: Scene document

- `director-scene` with the node data enum, property channel table, effect and transition table, keyframes and serde plus schemars.
- An exporter from the current `Director` so the golden corpus can round-trip through the document.

Gate: every golden scene round-trips through `director-scene` JSON without loss.

### Phase 3: Parallel renderer

- `director-render` evaluating the document per frame, painting on a worker pool, with the layer cache, media frame server and new encoder path.
- Delete the old `Director` render path once parity holds.

Gate: golden corpus parity with the old core, and a measured end-to-end 1080p HTML export speedup recorded in the table below.

### Phase 4: Host session and agent surface

- `director-host` with patch, revision and cancellation; the CLI becomes a thin client.
- An MCP server mode, generated skills, and `outline`, `diff` and QA-score commands with one JSON error envelope.
- An editor protocol: shared-memory frames, generation cancel, windowed audio and an edit and seek control plane.

Gate: an agent can open, patch, inspect and export a project through MCP alone.

### Phase 5: Authoring

- Typed `ComputedStyle` plus a property table in `director-html`; split element paint.
- Timeline JSON for motion over HTML visuals.
- GSAP to native keyframe tracks.

Gate: the Hyper CN corpus compiles through the typed style path with no new refusals, and a GSAP fixture set renders as native tracks.

### Phase 6: Proof and media quality

- A public benchmark harness against HyperFrames, FFrames and Remotion (needs the user's explicit yes before running).
- Motion blur on every paint path, 10-bit and HDR export, LUFS normalization and ducking.

Later candidates: a Tauri editor over the host protocol, a WASM player, distributed chunked rendering.

## Latest measurements

One row per check, holding the most recent passing count. Replace the row when a check is re-run. All runs used `--release`.

| Check | Count | Date | Command |
| --- | --- | --- | --- |
| workspace clippy | pass | 2026-10-25 | `cargo clippy --workspace --all-targets --release -- -D warnings` |
| director-core release tests | 435 | 2026-10-25 | `cargo test -p director-core --release` |
| visual_suite (render, scene round trip, document render) | 41 | 2026-10-25 | `cargo test -p director-core --release --test visual_suite` |
| html_export | 4 | 2026-10-22 | `cargo test -p director-cli --release --test html_export` |
| workspace release tests | 968 | 2026-10-25 | `cargo test --workspace --release` |
| director-scene | 4 | 2026-10-25 | `cargo test -p director-scene --release` |
| director-render | 10 | 2026-10-25 | `cargo test -p director-render --release` |
| scene_import lib | 2 | 2026-10-25 | `cargo test -p director-core --release --lib scene_import` |
| filter_fns | 6 | 2026-10-25 | `cargo test -p director-html --release --test filter_fns` |
| HTML scene_document | 7 | 2026-10-25 | `cargo test -p director-html --release --test scene_document` |
| Vulkan engine clippy | pass | 2026-10-22 | `cargo clippy -p director-cli --features vulkan --all-targets --release -- -D warnings` |
| text_stroke | 6 | 2026-10-20 | `cargo test -p director-html --release --test text_stroke` |
| shrink_wrap | 1 | 2026-10-20 | `cargo test -p director-html --release --test shrink_wrap` |
| padded_text | 1 | 2026-10-20 | `cargo test -p director-html --release --test padded_text` |
| rich_text | 5 | 2026-10-21 | `cargo test -p director-html --release --test rich_text` |
| font_shorthand | 4 | 2026-10-21 | `cargo test -p director-html --release --test font_shorthand` |
| gradient_tiles | 2 | 2026-10-21 | `cargo test -p director-html --release --test gradient_tiles` |
| motion_policy | 3 | 2026-10-21 | `cargo test -p director-html --release --test motion_policy` |
| transform_angles | 3 | 2026-10-22 | `cargo test -p director-html --release --test transform_angles` |
| director-core doctest | 1 | 2026-10-22 | `cargo test -p director-core --doc --release` |
| director-html lib | 110 | 2026-10-22 | `cargo test -p director-html --release` |
| paint_gaps, background_layers | 22, 5 | 2026-10-18 | release run, exact command not recorded |
| object_position_css, gradient_hints, project, shader | 9, 8, 7, 10 | 2026-10-17 | `cargo test -p director-html --release` |
| mixed_calc, gradient_calc | 12, 3 | 2026-10-18 | `cargo test -p director-html --release --test mixed_calc --test gradient_calc` |
| subgrid filter | lib 8, grid_lines 1 | 2026-10-18 | `cargo test -p director-html --release --lib --test grid_lines subgrid` |
| fixed_position, css_media, css_nesting | 5, 6, 12 | 2026-10-18 | `cargo test -p director-html --release --test fixed_position --test css_media --test css_nesting` |
| svg_presentation, svg_markers | 12, 12 | 2026-10-18 | `cargo test -p director-html --release --test svg_presentation --test svg_markers` |
| depth_sort, depth_nested | 8, 1 | 2026-10-18 | `cargo test -p director-html --release --test depth_sort --test depth_nested` |
| capture extractor | 6 | 2026-10-17 | `node --test crates/director-html/tools/tools.test.mjs` |
| static_reuse, pipeline_equivalence | 7, 8 | 2026-10-17 | `cargo test -p director-core --release --test static_reuse --test pipeline_equivalence` |
| agent commands | 15 | 2026-10-17 | `cargo test -p director-cli --release --bin director-engine agent_` |
| inspect_samples, repair_batch | 1, 5 | 2026-10-17 | release `director-cli` tests, exact command not recorded |
| recipe scripts | 9 | 2026-10-17 | release `examples` filter `example_recipes_` |

The 2026-10-19 workspace release run passed every target after two fixture updates: a rejected preview reload now uses `perspective:0`, and SVG text with no font family is no longer treated as a compile error. The `Director::new` doctest passes `fps` as `30.0`.

The 2026-10-22 skia-safe 0.153.3 upgrade (Skia m153) left every golden in place. Under the release engine, the four HTML examples, a 3 s `native-motion` export and 8-frame contact sheets of all 59 example scripts match 0.90.0 pixel for pixel.

Linux CI run 39719292637 failed only the 12 text goldens: FreeType anti-aliases glyphs differently from DirectWrite. Linux text goldens are an open decision.

## Next action

1. Continue phase 3: move the node traversal, the static layer cache and per-kind paint from core `render_recursive` into `director-render` behind the same parity tests, switch export to `director-render`, and measure a 1080p HTML export through `render_frames` against the current export.
2. Push when asked. Linux CI still fails the 12 Windows-rendered text goldens until Linux goldens land.

## Verification

```powershell
cargo test --workspace --release
cargo clippy --workspace --all-targets --release -- -D warnings
node --test crates/director-html/tools/tools.test.mjs
```

Media tests need FFmpeg and ffprobe. Do not run the Hyper CN registry smoke, Chrome, or long benchmarks without the user's explicit yes.
