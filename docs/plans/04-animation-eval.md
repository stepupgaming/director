# Plan 04 — Animation Evaluation Lock

> **Status:** Spec  
> **Priority:** P1  
> **Depends on:** 01 for time helpers; independent of product features  
> **Blocks:** trustworthy motion; feeds 05 serialization of keyframes

## Problem

Animation surface is broader than the old audit claimed (`CubicBezier`, `Hold`, `Expression`, `reverse`, `yoyo`, `loop_count` exist in `animation.rs`), but the **evaluation contract is not locked**:

1. Multi-segment keyframes, zero-duration segments, t before first / after last keyframe — under-specified in tests.
2. Unknown property / easing paths still soft-fail in places (`warn` + linear / no-op) instead of a single loud policy.
3. `Expression` easing bakes/evaluates with a small function set — edge errors unclear.
4. `TextFit::Shrink` vs animated `font_size` was fixed once; needs permanent regression tests.
5. Spring vs keyframe interaction and `current_value` mutation during fit/layout can still fight.
6. Path animation loop wrapping lives in `Director::update` separately from `Animated<T>` — second eval story.

## Goals

- **Pure eval:** `value_at(t)` determined only by keyframe data + documented loop/reverse state + t.
- Table-driven tests for every `EasingType` and segment boundary.
- One policy: invalid animate targets/easings **error** on authoring APIs used by Rhai.
- Fit/layout never writes animation channel state.

## Non-goals

- New easing types beyond fixing expression safety.
- Timeline UI curves editor.
- Full AE expression language.

## Target model

### `Animated<T>` contract

```text
value_at(t):
  empty keyframes → initial / default
  t <= first.time → first.value (or extrapolate — document HOLD)
  t >= last.time → last.value
  else interpolate segment with segment.easing
```

Loop/reverse/yoyo:

- Prefer transforming the query time **before** segment search (unwrap loops into effective t), not mutating keyframes every frame.
- If current impl mutates keyframe lists on `reverse()`/`yoyo()`, treat those as **authoring** ops; runtime eval should still be a pure function of stored data + t.

### Authoring API policy

| Input | Behavior |
|-------|----------|
| Unknown property name | `Err` / Rhai runtime error listing valid names |
| Unknown easing string | `Err` with valid list (`parse_easing_result` already exists — use everywhere) |
| Zero duration segment | Allowed: step to target at t_start (document) |
| Negative duration | `Err` |
| Empty expression | `Err` or identity — document |

### Property registry

Single list of animatable property names (transform + per-element), shared by:

- Rhai `animate` / `spring`
- Validation
- Optional schema/docs generation later (plan 05)

### Text fit

- Fit-solved size stored in a **separate** field from `Animated` font_size channel.
- Tests: animate font_size with Shrink fit enabled ⇒ no channel clobber.

## Implementation steps

1. Document eval semantics in `animation.rs` module docs + `core-contracts.md`.
2. Add `animated_eval` unit tests module (table driven).
3. Grep all easing parse sites; route Rhai through loud `Result` path only.
4. Build property name set per element kind; fail unknown.
5. Audit `TextFit` write paths; add regression test.
6. Path animation: document wrapping; add tests for loop_count boundary.
7. Expression evaluator: reject unknown identifiers; test `t=0`, `t=1`, div-by-zero guard.

## Tests (acceptance)

| Test | Asserts |
|------|---------|
| `eval_linear_midpoint` | classic lerp |
| `eval_hold_stays_start_until_end` | Hold easing |
| `eval_bezier_monotonic_x` | cubic bezier y in range for valid controls |
| `eval_before_after_extents` | clamp policy |
| `reverse_yoyo_loop_tables` | known sequences |
| `unknown_prop_errors` | Rhai or Rust API |
| `unknown_easing_errors` | no silent linear |
| `shrink_fit_does_not_clobber_font_size_keyframes` | regression |
| `expression_rejects_bad_ident` | safe failure |

## Files likely touched

- `crates/director-core/src/animation.rs`
- `crates/director-core/src/scripting/api/animation.rs`
- `crates/director-core/src/scripting/utils.rs`
- `crates/director-core/src/node/text.rs`, `mod.rs`
- `crates/director-core/src/director.rs` (path anim section)
- Tests under `crates/director-core/src/animation.rs` or `tests/animation_eval.rs`

## Risks

- Tightening errors breaks scripts that relied on typos no-oping — acceptable under bleeding-edge preference; call out in changelog/examples.
- Expression feature performance if re-parsed every sample — cache parsed form if needed.

## Done when

- [ ] Eval semantics documented and table-tested  
- [ ] Rhai animate paths cannot silent-no-op unknown prop/easing  
- [ ] Fit vs font_size regression locked  
- [ ] Path loop boundaries tested  
