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

| Item | Contract | Current gap |
|------|----------|-------------|
| `value_at(t)` | Pure in keyframes + t + documented loop state | Under-tested edges |
| Unknown prop/easing | Authoring API errors | Some soft paths remain |
| Text fit | Must not write animation channels | Needs permanent regression |

See plan **04**. Features (bezier/hold/yoyo) exist; **correctness lock** is the work.

---

## 6. Scene document

| Item | Contract | Current gap |
|------|----------|-------------|
| Canonical I/O | Versioned project document ↔ Director | Parallel MovieRequest drift history |
| Required kinds | Hard error on load failure | Some fallbacks possible |
| Catalog | Effects/transitions SSOT in `api_catalog` | Keep CI parity |

See plan **05**.

---

## 7. Determinism & media

| Item | Contract | Current gap |
|------|----------|-------------|
| Fonts | Required set present or export/CI fails | Bold/etc optional warn |
| RNG | All lib noise uses seeded RNG | Enforce no `thread_rng` |
| Video seek | One policy (floor/nearest) + freeze pin | Under-specified |
| A/V duration | Same duration policy as frame count | Under-tested |

See plan **06**.

---

## Explicit non-contracts (for now)

These are **not** core solidity work:

- Named compositions / props CLI / live preview  
- Agent repair loops / recipes / SDK L1–L3  
- New shader packs, NLE UI, GPU rewrite, in-engine STT  

Deferred material: [`docs/archive/`](../archive/README.md).
