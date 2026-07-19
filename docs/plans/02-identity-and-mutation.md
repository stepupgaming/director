# Plan 02 — Identity & Graph Mutation

> **Status:** Done (2026-07-19)  
> **Priority:** P0  
> **Depends on:** 01 preferred (timeline callers settled)  
> **Blocks:** 03 (reliable dirty sets), 05 (stable serialization ids)

## Problem

1. `NodeId = usize` indexes an arena with **free-list recycling** (`scene.rs`). Destroy → reuse → live handles silently point at a new element.
2. Graph-editing ops (`apply_effect`, `set_mask`, reparent) **rewrite topology** and can change which id is “canonical” while callers keep old handles.
3. `CompositionNode` embeds a **full nested `Director`** with its own arena — ids are not comparable across boundaries; time/assets coupling is ad hoc.
4. Incremental compile/invalidate keys off `NodeId`; aliasing makes dirty sets wrong.
5. Semantic `EntityId` (UUID) is already stable — scene nodes should not be weaker without reason.

## Goals

- Handles never alias after destroy or recycle.
- Mutation APIs have **explicit, tested** semantics (rewrite vs attach).
- Nested composition boundary documented: what is shared, what is isolated.
- Invalidate/compile remain correct under create/destroy/reparent.

## Non-goals

- Full ECS rewrite.
- GUI selection model.
- Changing EntityId design.

## Target model

### Stable node identity

**Recommended approach (generational index):**

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct NodeId {
    index: u32,
    generation: u32,
}
```

- Arena slot stores `generation`.
- `add_node` reuses free index **only** with incremented generation.
- `get_node` / `get_node_mut` require generation match; mismatch ⇒ `None`.
- Rhai handles store `NodeId`; methods fail clearly on stale ids.

**Alternative (simpler, denser leaks):** never recycle until explicit `compact()`; document that compact invalidates all external ids. Prefer generational unless profiling forces otherwise.

### Handle invalidation policy

| Event | Effect on existing NodeIds |
|-------|----------------------------|
| `destroy_node` | That id and descendants become stale |
| `apply_effect` (if rewrites) | Old target id policy below |
| Reparent | Id unchanged |
| Project load | New arena; ids from file are the new truth (no merge with pre-load handles) |

### Mutation semantics (pick and lock)

**Decision for this plan: dual-mode with default “attach” long-term; short-term make rewrite honest.**

#### A. Effect attachment (preferred end state)

- `apply_effect(node, …)` adds effect config on the node (or a child effect layer) **without** replacing the caller’s handle.
- Returned value is the same `NodeId` (or explicit `EffectHandle` that doesn’t steal identity).

#### B. Rewrite (current-ish) — if kept temporarily

- Must return the **new** canonical `NodeId`.
- Old id is destroyed or marked stale (generation bump on old slot).
- Rhai binding updates: scripts must use the returned handle.
- Loud error if a stale handle is used afterward.

`set_mask`:

- Mask node id stored on target; mask is not silently destroyed.
- Reparenting mask into internal slot must not recycle the mask’s public id without documentation.
- Prefer: mask remains a normal child referenced by `mask_node: NodeId` without removing it from the tree **or** document exclusive ownership.

### Nested composition contract

Document in `core-contracts.md` and enforce with tests:

| Concern | Rule |
|---------|------|
| Identity | Inner `NodeId`s are **not** valid in the outer director |
| Time | Outer feeds `local_time - start_offset` (or clip mapping) into inner `update` |
| Assets/fonts | Shared via `DirectorContext` / cloned `AssetManager` — document which |
| Dirty | Invalidating outer comp node re-renders whole inner surface unless inner exposes dirty API later |
| Serialize | Inner director is a nested project document (already direction in `project.rs`) |

Optional later: replace full nested `Director` with `Subgraph { scene, tracks, fps, … }` to drop double god-object — **not required** to close this plan if contracts + tests exist.

## Implementation steps

1. Introduce generational `NodeId` (or non-recycle) in `types.rs` / `scene.rs`.
2. Fix all construct/match sites (compiler will guide).
3. Rhai: stale handle → runtime error string, not panic/no-op.
4. Inventory `apply_effect` / `set_mask` / reparent in `scripting/utils.rs`, `properties.rs`, nodes API.
5. Implement chosen mutation policy + tests for handle identity before/after.
6. `destroy_node`: assert free-list reuse does not satisfy old `NodeId` equality.
7. Nested comp: unit test that outer handle ≠ inner root id space; time offset test.
8. Update `project` JSON if `NodeId` representation changes (store index+gen or stable string keys).

## Tests (acceptance)

| Test | Asserts |
|------|---------|
| `node_id_no_alias_after_destroy` | destroy + add ⇒ old id get_node is None |
| `stale_handle_rhai_errors` | method on destroyed node errors |
| `apply_effect_handle_policy` | per chosen A/B: same id or returned new + old stale |
| `set_mask_preserves_mask_id` | mask NodeId still resolves |
| `reparent_keeps_id` | id stable across reparent |
| `nested_comp_id_isolation` | inner ids not fetchable on outer |
| `invalidate_after_destroy` | dirty set never contains stale ids |

## Files likely touched

- `crates/director-core/src/types.rs`
- `crates/director-core/src/scene.rs`
- `crates/director-core/src/scripting/types.rs`, `utils.rs`, `api/*`
- `crates/director-core/src/node/composition.rs`
- `crates/director-core/src/compiler.rs`, `project.rs`
- Broad compile fix across core tests

## Risks

- Large mechanical diff for `NodeId` shape change.
- Project/schema version bump.
- Visual tests hold raw usizes — centralize helpers.

## Done when

- [x] No silent id recycle under live handles — generational `NodeId` + free-list gen bump  
- [x] Mutation policy documented in `core-contracts.md` and tested  
- [x] Nested composition boundary documented and tested  
- [x] Compile/invalidate ignore stale ids (`get_node` generation match; dirty retain live)  
- [x] Acceptance tests: `crates/director-core/tests/identity_mutation.rs` + scene unit tests  

### Implementation notes

- `NodeId { index, generation }` in `types.rs`; arena `generations` parallel to `nodes`.
- Project JSON stores full `NodeId` (serde object); `PROJECT_VERSION` bumped.
- Rhai `node_id()` still returns bare index for script compat; compiler invalidation resolves index → live `id_at`.
- `apply_effect_to_node`: rewrite wrap; `push_or_wrap_effect`: attach when already `EffectNode`.
- `set_mask`: exclusive ownership; destroy owner destroys mask.
