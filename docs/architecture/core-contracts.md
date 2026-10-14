# Core Contracts

Normative engine spines. Implementation plans: [`docs/plans/`](../plans/00-core-hardening-index.md).

When code and this doc disagree, **fix the doc in the same change** (or fix the code if the doc is the intent).

Truth snapshot for “current gap” columns: verified 2026-07-19 in plan **00** (re-grep before starting each phase).

---

## 1. Time

| Item | Contract | Status (2026-07-19) |
|------|----------|---------------------|
| Timeline structure | Single model: `tracks` / `TrackClip` only | **Done** — stored `Director.timeline` removed; `flat_timeline()` derives DTOs |
| Clip local time | `local_time(global)` from speed, source window, optional `freeze_time` | **Done** — `TrackClip::local_time`; `add_freeze` sets `freeze_time` |
| FPS | `f64` end-to-end (Director, export, manifest, schema, project) | **Done** |
| Frame ↔ time | `floor(time * fps + 1e-9)` | **Done** — `timeline::{frame_to_time,time_to_frame,duration_to_frame_count}` + Rhai `movie_fps` / helpers |
| Loop | `add_loop` duplicates clip placement and **copies audio track ids** | **Done** on track clips |

See plan **01** (implemented).

---

## 2. Identity

| Item | Contract | Status (2026-07-19) |
|------|----------|---------------------|
| `NodeId` | Generational `{ index, generation }`; free-list reuse bumps generation | **Done** |
| Stale handles | `get_node` / Rhai `ensure_alive` fail; no silent alias | **Done** |
| Nested composition | Inner `NodeId`s valid only on the nested director; outer cannot fetch them | **Done** (contract + tests) |
| Entity ids | UUID stable (already) | OK |

### Nested composition boundary

| Concern | Rule |
|---------|------|
| Identity | Inner arena ids are **not** valid on the outer `Director` |
| Time | Outer feeds `local_time - start_offset` into inner `update` |
| Assets/fonts | Shared via cloned `AssetManager` / `DirectorContext` when the composition is created |
| Dirty | Invalidating the outer composition node re-renders the whole inner surface |
| Serialize | Inner director is a nested project document |

See plan **02** (implemented).

---

## 3. Graph mutation

| Op | Contract | Status |
|----|----------|--------|
| Reparent | Same `NodeId`; hierarchy edges update | **Done** |
| `apply_effect` | **Rewrite (honest):** returns new effect-wrapper `NodeId`; original stays alive as child; scene roots / parent edges rewrite to wrapper. Rhai returns the new handle; old handle still addresses the inner node. If target is already `EffectNode`, `push_or_wrap_effect` **attaches** in place (same id). | **Done** |
| `set_mask` | Mask keeps its `NodeId`; detached from previous parent's children; exclusive via `mask_node` + parent pointer; destroy owner destroys mask | **Done** |
| Destroy | Recursive (children + exclusive mask); generation bump; ids stale | **Done** |

See plan **02** (implemented).

---

## 4. Frame pipeline

```text
update(time, dirty?) → layout → post_layout → render → (mix_audio / encode)
```

| Item | Contract | Status (2026-07-19) |
|------|----------|---------------------|
| Full vs incremental | Same pixels for same scene+time after public mutations | **Done** — `tests/pipeline_equivalence.rs` |
| Static cache | Auto-invalidated via `note_mutation` / `invalidate`; dirty static nodes never serve cache hits | **Done** |
| Dirty expansion | Ancestors, descendants, mask subtrees, reverse mask users; drop stale ids | **Done** |
| Uncompiled graph | Incremental falls back to full (`!is_compiled()` ⇒ dirty `None`) | **Done** |
| Mutation hook | Public pixel/layout mutations call `Director::note_mutation` | **Done** (color, mask, set_static, invalidate) |

Entry points: `render_at_time` (export / incremental-aware), `render_frame` (preview; full update).

See plan **03** (implemented).

---

## 5. Animation eval

| Item | Contract | Status |
|------|----------|--------|
| `Animated::value_at(t)` | Pure in `raw_keyframes` + t (hold before first / after last; segment easing incl. Hold) | **Locked** (plan 04) |
| `reverse` / `yoyo` / `loop_count` | Authoring ops that rewrite stored keyframes; runtime eval stays pure | **Locked** |
| Unknown prop/easing | Rhai `animate` / `spring` / path APIs return runtime `Err`; no silent linear/no-op | **Locked** |
| Text fit | `fitted_font_size` separate from animated `default_font_size` channel | **Locked** |
| Expression easing | Bad/empty idents fail via `evaluate_expression_result` | **Locked** |
| Path loops | `wrap_looped_time(effective, duration, loop_count)` SSOT | **Locked** |

See plan **04** (implemented). Acceptance: `crates/director-core/tests/animation_eval.rs`.

---

## 6. Scene document

| Item | Contract | Status |
|------|----------|--------|
| Canonical I/O | Versioned `Project` JSON (`PROJECT_VERSION`) ↔ `Director` | **Primary** path (`save_project_json` / `load_project_json`) |
| `fps` | `f64` in document (NTSC-safe) | **Locked** |
| Tracks | Canonical; `legacy_timeline` migrates on load | **Locked** (plan 01) |
| Node ids | Generational `{ index, generation }` in JSON | **Locked** (plan 02) |
| Required kinds | Box, Text, Image, Video, Caption, SmartCaption, Effect, Composition, Lottie, Spectrum — rebuild or **hard error** | **Locked** (plan 05) |
| Optional kinds | Vector, Shape, ActiveSpeakerCamera, Wgpu — may placeholder with warning | Declared |
| Catalog | `api_catalog` SSOT for Rhai effect names; `project_effect_type_tags` / `EffectType::type_tag` for runtime enum | **Parity tested** |
| MovieRequest / pipeline | Secondary inbound; do not add features only there without project parity | Documented |

See plan **05** (implemented). Acceptance: `tests/project_roundtrip.rs` Plan 05 cases.

---

## 7. Determinism & media

| Item | Contract | Status |
|------|----------|--------|
| Fonts | Roboto R/B/I, Inter R/B, Poppins Bold required; export fails via `ensure_required_fonts` | **Locked** (plan 06) |
| RNG | `rand_float` / `set_seed` use process `StdRng` default `0xC0FFEE`; no `thread_rng` in lib | **Locked** |
| Video seek | **Floor** onto the probed source frame grid (`video_seek_time` / `video_source_frame_index`); 30 FPS fallback when probing fails; freeze pins local time | **Locked** |
| Video decode | Reuse a persistent FFmpeg pipe for sequential reads; cache up to 8 frames; restart on backward or >60-frame seeks | **Implemented** |
| A/V duration | `duration_to_frame_count` × `audio_samples_per_video_frame(sr, fps, 2)`; the mixer takes per-channel sample frames and emits interleaved stereo | **Locked**; MP4 duration acceptance in CLI `tests/html_export.rs` |
| GPU export | Supplied Vulkan context requires a GPU target; synchronized direct RGBA readback reuses the export buffer | **Implemented**; live GPU readback test and HTML CPU/GPU parity harness |
| CI | Fonts downloaded on PR/push; visual suite `mock_video` | Present |

See plan **06** (implemented). Acceptance: `tests/determinism_media.rs`.

---

## Explicit non-contracts (for now)

These are **not** core solidity work:

- Live preview server: landed (`director-engine preview`); compositions/props CLI: landed  
- Agent repair loops / recipes / SDK L1–L3  
- New shader packs, NLE UI, GPU rewrite, in-engine STT  

Deferred material: [`docs/archive/`](../archive/README.md).
