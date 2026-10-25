# Plan 03 — Frame Pipeline Correctness (Full ≡ Incremental)

> **Status:** Done (2026-07-19)  
> **Priority:** P0  
> **Depends on:** 01 (time), 02 (ids) for full confidence; scaffolding can start earlier  
> **Blocks:** trusting static cache and compile-phase optimizations in production

## Problem

The frame path is:

```text
update(time, dirty?) → layout → post_layout → director-render Painter → (encode)
```

Incremental machinery exists (`compile`, `invalidate`, `dirty_nodes`, `expand_dirty_set`, `is_static` / `static_cache`), but:

1. **Full vs incremental pixel equivalence is not a property test** — only generation counters on a few unit cases.
2. Static cache requires **manual** `clear_static_cache` after mutation — easy to ship stale pixels.
3. Hierarchy changes require re-`compile()`; easy to forget.
4. Layout `dirty_style` must flip when animated layout-affecting props change — gaps cause one-frame geometry bugs.
5. Motion blur / effect UV edge cases can diverge between paths if sampling differs.
6. `CompositionNode` surface cache is a second caching layer with its own invalidation rules.

## Goals

- **Invariant:** for any scene and time,  
  `render_full(t) == render_incremental(t)` after any sequence of invalidations that the public API allows (bit-exact or documented tolerance).
- Static/comp caches **cannot** serve stale content after a mutating API call.
- Dirty expansion includes all nodes required for correct transform/layout/mask context.
- Documented single entry points: `director_render::FrameState::render`, `render_frame`, export loop.

## Non-goals

- Parallel GPU renderer.
- Changing Skia backend.
- New visual effects.

## Target model

### Render modes

| Mode | Dirty set | Cache |
|------|-----------|--------|
| Full | `None` (all active nodes update + draw) | Reuse `static_cache` when `StaticCacheKey` matches. Time-varying subtrees store `Some(local_time)` and miss on a later frame |
| Incremental | `Some(expand(dirty))` | Static hit only if the node is not dirty |

After each frame, consuming dirty set is OK if the next mutation re-invalidates. Prefer: **mutations always mark dirty**; render clears after successful frame.

### Automatic invalidation

On any public mutation that can change pixels or layout:

1. Mark affected `NodeId`s dirty (and dependents via graph when compiled).
2. Drop static cache entries for affected subtrees (ancestors that composited them).
3. Drop composition surface cache when inner or outer time/inputs change.
4. Set `dirty_style` when Taffy-relevant fields change.

If `compile()` has never run, incremental mode **falls back to full** (safe default).

### Dirty expansion (normative)

`expand_dirty_set` must include:

- The dirty node
- All **ancestors** to root (transform/opacity/layout context)
- All **descendants** (content)
- **Mask** nodes referenced by any included node (and their subtrees)
- Nodes that **use** a dirty node as mask
- Dependents from `DependencyGraph` (entities, audio bindings)

Property: skipping any of the above can change pixels ⇒ expansion incomplete.

### Layout sync

- `dirty_style == true` ⇒ push style to Taffy before compute.
- Animated width/height/margin/etc. must set `dirty_style` in `update` when value changes.
- Test: animate width; layout_rect changes every frame under both full and incremental.

## Implementation steps

1. Inventory cache write/read sites (`static_cache`, comp `surface_cache`).
2. Centralize `Director::note_mutation(ids | inputs)` used by scripting APIs.
3. Auto-clear static entries along ancestor chain on mutation.
4. Safe default: uncompiled graph ⇒ full render.
5. Property test harness:
   - Build random-ish graphs (boxes, text, opacity, mask, nested depth limit).
   - Random invalidate subsets + random times.
   - Compare RGBA buffers (exact or max abs diff 0–1).
6. Golden tests: motion blur weights; one off-origin SkSL effect (close UV audit).
7. Layout animation regression.
8. Document pipeline in `core-contracts.md`.

## Tests (acceptance)

| Test | Asserts |
|------|---------|
| `prop_full_eq_incremental` | N random trials, pixel match |
| `static_cache_auto_invalidates_on_color_change` | mutation flips pixels without manual clear |
| `hierarchy_change_without_compile_falls_back_or_updates` | no stale tree |
| `mask_dirty_expands_to_masked_content` | mask edit updates composite |
| `animated_layout_updates_rect` | layout_rect tracks animation |
| `motion_blur_normalized_fixture` | no over-bright multi-sample |
| `effect_uv_off_origin_fixture` | Liquid/Acid or probe harness |

## Files likely touched

- `crates/director-render/src/frame.rs`, `crates/director-render/src/paint/` (the frame path moved here from core `systems/renderer.rs`)
- `crates/director-core/src/systems/layout.rs`
- `crates/director-core/src/director.rs` (`update`, `expand_dirty_set`, `invalidate`)
- `crates/director-core/src/compiler.rs`
- `crates/director-core/src/node/composition.rs`
- `crates/director-core/src/scripting/**` (mutation hooks)
- New: `tests/pipeline_equivalence.rs` or similar

## Risks

- Property tests flaky if non-determinism remains (fonts/RNG) — pin seed/fonts (plan 06).
- Performance: over-invalidation is correct; tune later.
- Nested mutex directors complicate random graph gen — cap nesting.

## Done when

- [x] Property equivalence test — `tests/pipeline_equivalence.rs` (`prop_full_eq_incremental`)  
- [x] No correctness dependency on manual `clear_static_cache` — `note_mutation` / dirty bypass  
- [x] Expansion rules documented and unit-tested (ancestors, descendants, reverse mask)  
- [x] UV-origin probe harness remains (`effect_uv_probe`); motion-blur path unchanged  
- [x] `core-contracts.md` pipeline section matches code  

### Implementation notes

- `Director::note_mutation` / `note_node_mutation` / `evict_caches_for` / `is_compiled`.
- `director_render::FrameState::render`: uncompiled or empty dirty ⇒ full frame.
- Static cache: skip hit when node is dirty; evict on mutation.
- `invalidate(Node(_))` always dirties that node (property mutations).
- `expand_dirty_set`: reverse mask users + retain live ids only.
