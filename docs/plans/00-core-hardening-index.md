# Core Hardening — Master Index

> **Status:** Plans 01–03 implemented (2026-07-19); next is plan 04  
> **Priority:** Engine solidity over product surface  
> **Out of scope until cores land:** compositions/props CLI, preview server, recipes, agent repair loops, new shader batches, SDK/template layers

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

- Named compositions / props / subcommand CLI (archived plan)
- Live preview server
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
| 4 | [04-animation-eval.md](04-animation-eval.md) | Pure, tested evaluation; loud unknown props | Spec — **next** |
| 5 | [05-scene-model.md](05-scene-model.md) | One serializable scene contract | Spec |
| 6 | [06-determinism-and-media.md](06-determinism-and-media.md) | Same script → same pixels/audio clock | Spec |

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

Implement **[04-animation-eval.md](04-animation-eval.md)** — pure eval table tests; loud unknown props.
