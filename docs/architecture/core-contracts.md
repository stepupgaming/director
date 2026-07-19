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

| Item | Contract | Current gap |
|------|----------|-------------|
| `NodeId` | Never aliases under destroy/reuse while a handle lives | `usize` + free-list recycle |
| Stale handles | Operations error; no silent no-op on wrong node | Weak |
| Nested composition | Inner ids invalid in outer director | Under-documented |
| Entity ids | UUID stable (already) | OK |

See plan **02**.

---

## 3. Graph mutation

| Op | Contract |
|----|----------|
| Reparent | Same `NodeId`; hierarchy edges update |
| `apply_effect` | Either non-rewriting attach **or** rewrite that returns new id and invalidates old — no silent handle theft |
| `set_mask` | Mask identity preserved per documented ownership |
| Destroy | Recursive; ids stale |

See plan **02**.

---

## 4. Frame pipeline

```text
update(time, dirty?) → layout → post_layout → render → (mix_audio / encode)
```

| Item | Contract | Current gap |
|------|----------|-------------|
| Full vs incremental | Same pixels for same scene+time after public mutations | Not property-tested |
| Static cache | Auto-invalidated on mutation; never required manual clear for correctness | Manual clear |
| Dirty expansion | Ancestors, descendants, masks, dependents | Partially implemented |
| Uncompiled graph | Incremental falls back to full | Verify |

See plan **03**.

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
