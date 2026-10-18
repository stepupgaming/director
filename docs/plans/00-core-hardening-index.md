# Core Hardening — Master Index

> **Status:** Plans 01–06 implemented (2026-07-19) — core hardening program complete
> **Priority:** Engine solidity over product surface  
> **Post-core product:** compositions, motion/recipes and CLI landed. Native projects and runtime are [plan 08](08-native-projects-and-runtime.md) (**Done**). Native paint and media fidelity follow [plan 09](09-native-fidelity.md). Static-subtree paint reuse is implemented (static_reuse 7, pipeline_equivalence 8, release, 2026-10-17). Agent production workflows are issue #3. Broader recipes and SDK layers remain later work.

## Goal

Make Director a **trustworthy frame engine**: one time model, stable identity, proven update→layout→render path, locked animation eval, one scene contract, deterministic media.

“Solid” means the gates in each plan pass — not that more features exist.

## Phase 0 — Truth pass (this document)

**Done when:** claims below are verified against the tree, archive is the only home for stale audit/product docs, and `core-contracts.md` matches this snapshot.

| Check | Result |
|-------|--------|
| Root `AUDIT.md` removed / archived | Yes → `docs/archive/AUDIT-2026-07.md` with archived banner |
| Compositions / SDK / template product docs archived | Yes → `docs/archive/` |
| Active plans 01–06 exist under `docs/plans/` | Yes |
| `docs/architecture/core-contracts.md` exists | Yes |
| Root `AGENTS.md` prioritizes core hardening | Yes |
| Code claims below re-grepped this session | Yes |

### Verified code snapshot (do not trust archived audit over this)

| Spine | Verified in tree | Evidence |
|-------|------------------|----------|
| **FPS type** | **`f64` end-to-end** (plan 01) | `Director`, encoder, manifest, schema, project |
| **Frame helpers** | **Present** (plan 01) | `timeline::{frame_to_time,time_to_frame,duration_to_frame_count}` + Rhai `movie_fps` / helpers |
| **Dual timeline** | **Removed** (plan 01) | Tracks only; `flat_timeline()` is a derived DTO view |
| **Default authoring path** | **Tracks** (plan 01) | `add_scene` / loop / freeze push `TrackClip` |
| **TrackClip** | Speed, source in/out, fades, **`freeze_time`** | `timeline.rs` |
| **`add_freeze`** | **Real** (plan 01) | Sets `freeze_time` on a hold clip |
| **`add_loop`** | Track copies; **preserves `audio_tracks`** | `lifecycle.rs` |
| **Node identity** | `NodeId = usize` + `free_indices` recycle | `types.rs`, `scene.rs` |
| **Nested comps** | Full inner `Director` in `CompositionNode` | `node/composition.rs` |
| **Dirty / static** | Implemented | `compile` / `invalidate` / `expand_dirty_set` / `static_cache` / `is_static`; full≡incremental **not** property-tested |
| **Animation features** | Bezier, Hold, Expression, reverse, yoyo, loop_count **exist** | `animation.rs`; Rhai uses `parse_easing_loud` in main animate paths |
| **Seedable RNG** | Landed | `set_seed`, default `0xC0FFEE` in `lifecycle.rs` |
| **Fonts** | Roboto-Regular **required**; Bold/Italic/Inter/Poppins **optional warn** | `BUNDLED_FONTS` in `director.rs` |
| **Export color tags** | H.264 gets `color_range` + bt709 tags | `video_wrapper.rs` |
| **Project I/O** | `save_project_json` / `load_project_json`; saves tracks only; **migrates** old `legacy_timeline` on load | `project.rs` |
| **Archived audit progress table** | **Unreliable** | Claimed f64 fps + real freeze; tree contradicts both |

### Deliberate non-goals (until cores land)

Do not schedule under this program:

- Named compositions / props / subcommand CLI (landed; see archive plan)
- Live preview server (landed: `director-engine preview`)
- Agent inspect→repair product loop
- Recipes / SDK L1–L3 / template strategy
- New shader batches, NLE UI, GPU rewrite, in-engine STT

## Plan sequence

| # | Plan | Outcome | Status |
|---|------|---------|--------|
| 0 | This index + truth pass | Single source of “what’s true” | **Done** |
| 1 | [01-time-and-timeline.md](01-time-and-timeline.md) | One timeline; real freeze/speed/in-out; `f64` fps | **Done** |
| 2 | [02-identity-and-mutation.md](02-identity-and-mutation.md) | Stable IDs; explicit graph mutation | **Done** |
| 3 | [03-frame-pipeline.md](03-frame-pipeline.md) | Full render ≡ incremental; auto cache invalidation | **Done** |
| 4 | [04-animation-eval.md](04-animation-eval.md) | Pure, tested evaluation; loud unknown props | **Done** |
| 5 | [05-scene-model.md](05-scene-model.md) | One serializable scene contract | **Done** |
| 6 | [06-determinism-and-media.md](06-determinism-and-media.md) | Same script → same pixels/audio clock | **Done** |
| 7 | [07-stdlib-and-recipes.md](07-stdlib-and-recipes.md) | L1 verbs + L2 contract + L3 recipes | **Done (v1 stop-line)** |
| 8 | [08-native-projects-and-runtime.md](08-native-projects-and-runtime.md) | Portable native projects, common loading, headless inspection/edits and GPU backend | **Done** |
| 9 | [09-native-fidelity.md](09-native-fidelity.md) | Slice 1, capture forwarding, Paper Shader, captured media, color hints, preserve-3d depth sort, and listed SVG presentation properties are in. 2026-10-17 release tests: gradient_hints 7, depth_sort 4, svg_presentation 8, compatibility 23. Smoke stays 239/239. Image backgrounds, text-decoration lines, and luminance masks passed paint_gaps 7 on 2026-10-17. CSS nesting passed css_nesting 6. Homogeneous calc/min/max/clamp lengths resolve. Mixed calc uses the composition axis when that axis is finite and positive. A pure percent stays a percent. A missing basis still fails. grid-template auto-fit, auto-fill, and named lines resolve. A later 2026-10-17 release run passed lib 101, paint_gaps 22, gradient_hints 8, depth_sort 7, depth_nested 1, svg_presentation 11, svg_markers 3, background_layers 4, mixed_calc 4, css_nesting 12, and grid_lines 1. `overflow-wrap` and `word-wrap` accept `normal`, `break-word`, and `anywhere`. `word-break` accepts `normal`, `break-word`, and `break-all`. `keep-all` still fails. compatibility stayed 23. CSS object-position passed object_position_css 6. `background-clip: content-box` clips the background to the content edge. `padding-box` clips to the padding edge. `border-box` still paints the border box. space and round repeat paint explicit tiles. repeat, no-repeat, repeat-x, and repeat-y stay on the image shader. A file background URL resolves through CompileOptions.base_dir. The capture extractor copies preserve-3d | **In progress** |

**Dependency order:** `01` first (everything samples time). `02` before trusting incremental edit. `03` can start once dirty APIs are understood; finish after `01`–`02`. `04` parallel after `01` time helpers exist. `05` after identity stabilizes. `06` continuous background; fonts/CI can land anytime.

## Workspace verification (every phase)

```bash
cargo check --workspace --all-targets
cargo test -p director-core
cargo test --workspace
cargo clippy --workspace --all-targets -- --D warnings
# when touching pixels/fonts:
cargo test -p director-core --test visual_suite --no-default-features --features mock_video
```

## Doc ownership

- Plans live in `docs/plans/` (this program).
- Architecture snapshot: `docs/architecture/overview.md` + `docs/architecture/core-contracts.md`.
- Historical audit/product drafts: `docs/archive/`.
- DOX: update nearest `AGENTS.md` when a phase changes ownership or contracts.

## Definition of done (program)

1. No legacy `timeline` path in update/render/export.
2. `fps` is `f64` (or rational) end-to-end; 23.976/29.97 tested.
3. `add_freeze` pins `local_time`; `add_loop` preserves audio.
4. Node IDs do not silently recycle under live handles.
5. Property tests: full frame == incremental frame for random invalidations.
6. Animation eval table tests green; unknown properties error.
7. Round-trip (or explicit unsupported-kind policy) for every public node kind.
8. CI enforces pinned fonts + core tests on PR/push.
9. `AUDIT` / product SDK docs remain archived; this index stays current.

## Next action

Core hardening plans 01–06, plan 07's implemented scope and [plan 08](08-native-projects-and-runtime.md) are **Done**. [Plan 09](09-native-fidelity.md) slice 1, capture forwarding, Paper Shader translation, and captured muted video and separate audio are implemented. The carried-forward Hyper CN smoke is 239/239 original and 239/239 portrait: the full run rendered 207, the six re-smoked CSS blocks render, and the 26 shader blocks rendered on 2026-10-15. The 213 blocks from before the shader pass were not re-run. Sampled captures stay sample holds. Gradient color hints, preserve-3d depth sort, and the listed external SVG presentation properties passed the 2026-10-17 release tests (gradient_hints 7, depth_sort 4, svg_presentation 8, compatibility 23). No new registry smoke. Image backgrounds, text-decoration lines, and luminance masks passed paint_gaps 7 on 2026-10-17. CSS nesting passed css_nesting 6. Homogeneous calc/min/max/clamp lengths resolve. Mixed calc uses the composition axis when that axis is finite and positive. A pure percent stays a percent. A missing basis still fails. grid-template auto-fit, auto-fill, and named lines resolve. A later 2026-10-17 release run passed lib 101, paint_gaps 22, gradient_hints 8, depth_sort 7, depth_nested 1, svg_presentation 11, svg_markers 3, background_layers 4, mixed_calc 4, css_nesting 12, and grid_lines 1. `overflow-wrap` and `word-wrap` accept `normal`, `break-word`, and `anywhere`. `word-break` accepts `normal`, `break-word`, and `break-all`. `keep-all` still fails. compatibility stayed 23. CSS object-position passed object_position_css 6. `background-clip: content-box` clips the background to the content edge. `padding-box` clips to the padding edge. `border-box` still paints the border box. space and round repeat paint explicit tiles. repeat, no-repeat, repeat-x, and repeat-y stay on the image shader. A file background URL resolves through CompileOptions.base_dir. Capture still refuses image-url backgrounds. The capture extractor copies preserve-3d. Static-subtree paint reuse passed static_reuse 7 and pipeline_equivalence 8 on 2026-10-17. Issue #3 tracks agent production workflows. Frontend/editor design, Remotion source compatibility and a separate SDK remain outside the current focus.
