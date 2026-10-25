# Core Hardening — Master Index

> **Status:** Plans 01–06 implemented (2026-07-19) — core hardening program complete
> **Priority:** Engine solidity over product surface
> **Post-core product:** compositions, motion/recipes and CLI landed. Native projects and runtime are [plan 08](08-native-projects-and-runtime.md) (**Done**). Native paint and media fidelity is [plan 09](09-native-fidelity.md) (**Superseded**). The native platform overhaul is [plan 10](10-native-platform-overhaul.md) (**In progress**), which holds current status and the next action. Transcription and the FFrames scorecard are [plan 11](11-transcription-and-competitive-gaps.md) (**Spec**).

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

This snapshot records the state at the 2026-07-19 truth pass. Later plans changed some rows (generational `NodeId` in plan 02, full≡incremental equivalence in plan 03, required fonts in plan 06); `core-contracts.md` holds the current contract.

### Deliberate non-goals (until cores land)

Do not schedule under this program:

- Named compositions / props / subcommand CLI (landed; see archive plan)
- Live preview server (landed: `director-engine preview`)
- Agent inspect→repair product loop
- Recipes / SDK L1–L3 / template strategy
- New shader batches, NLE UI, GPU rewrite, STT inside the render core (plan 11 places ASR in the optional `director-asr` crate)

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
| 9 | [09-native-fidelity.md](09-native-fidelity.md) | Native CSS, SVG, 3D, shader, and captured-media fidelity for the Hyper CN corpus and authored HTML | **Superseded** |
| 10 | [10-native-platform-overhaul.md](10-native-platform-overhaul.md) | Scene document, parallel renderer, host session, HTML-first authoring, proof of speed and quality | **In progress** |
| 11 | [11-transcription-and-competitive-gaps.md](11-transcription-and-competitive-gaps.md) | Optional `director-asr` crate (Whisper, Parakeet v3 and Bark 3.0 TDT packs), transcript-driven captions and edits, FFrames scorecard, distribution and head-to-head protocol | **Spec** |

**Dependency order:** `01` first (everything samples time). `02` before trusting incremental edit. `03` can start once dirty APIs are understood; finish after `01`–`02`. `04` parallel after `01` time helpers exist. `05` after identity stabilizes. `06` continuous background; fonts/CI can land anytime.

## Workspace verification (every phase)

```bash
cargo check --workspace --all-targets
cargo test -p director-core
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
# when touching pixels/fonts:
cargo test -p director-core --release --test visual_suite
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

Plans 01–08 are done and plan 09 is superseded. Resume from the Next action section of [plan 10](10-native-platform-overhaul.md).
