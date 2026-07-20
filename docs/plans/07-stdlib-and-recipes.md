# Plan 07 — L1 Stdlib + L2 Components + L3 Recipes

> **Status:** In progress (2026-07-19) — **not finished**
> **Stack:** L0 core (done) → L1 motion verbs → L2 templates/compositions → L3 recipes
> **Not in scope:** `director-sdk` crate, binding migration, capability matrix (Option C deferred)

## Honest scope

Calling L0–L3 "done" after a thin motion pack + one recipe was wrong. This plan tracks a **real** authoring stack:

| Layer | Done means | Status now |
|-------|------------|------------|
| **L0** | Engine spines 01–06 green | **Done** |
| **L1** | Archive-class macros: fade/pop/fly + **typewriter**, **pulse_on_beat**, **ken_burns**; tests; examples | **Mostly done** (v1 verbs landed; expand as recipes need) |
| **L2** | Layout templates **and** at least one **content-reactive** component (`add_smart_lower_third`); composition+props as the reusable unit | **Partial** — templates + smart L3 + compositions exist; not a full component library |
| **L3** | Multiple recipe arcs under `examples/recipes/`, each `register_composition`, CLI/preview green | **Partial** — `faceless_short` + `kinetic_lower_third`; more arcs TBD |

## Design rules

1. No new crate. L1 registers on the same engine as L0.
2. L1 is thin wrappers over keyframe `animate` / text animators (one eval path).
3. L2 API = `add_*` templates **or** `register_composition` with shallow props.
4. Every L3 recipe **registers at least one composition**.
5. Recipes take props; no hidden globals.
6. Quality bar: intentional short-form look.

## L1 verbs

| Verb | Behavior |
|------|----------|
| `fade_in` / `fade_out` | opacity |
| `pop_in` | scale + opacity bounce |
| `fly_in_*` / `fly_out_*` | translate + fade |
| `pulse` | single scale pulse |
| `pulse_on_beat(bpm, duration[, peak])` | beat-locked pulses |
| `typewriter(dur[, delay])` | TextNode glyph opacity stagger |
| `ken_burns(dur[, zoom[, dx, dy]])` | slow zoom + drift |

## L2 contract

- Layout sugar: existing `add_title_card`, `add_lower_third`, quote/progress/bento/...
- **Smart:** `add_smart_lower_third` — name/title stack, accent bar, width from content length (clamped).
- Reusable unit with props: `register_composition`.

## L3 recipes (current)

| Recipe | Composition | Arc |
|--------|-------------|-----|
| `examples/recipes/faceless_short.rhai` | `faceless_short` | 9:16 kinetic explainer beats |
| `examples/recipes/kinetic_lower_third.rhai` | `kinetic_lower_third` | 16:9 interview plate + typewriter + smart L3 |

### Still not L3 (archive fantasy — explicit non-goals until deps exist)

- Faceless Historian with **TTS / Supertonic VO**
- Podcast Clipper with **energy-based auto-cut + speaker reframes**
- Asset-store / `.dirproj` component distribution

## Verification

```bash
cargo test -p director-core --lib stdlib
cargo test -p director-core --test examples stdlib_motion
cargo test -p director-core --test examples smart_lower_third
cargo test -p director-core --test examples faceless_short
cargo test -p director-core --test examples kinetic_lower_third
```

## Done when (plan exit)

- [x] L1: fade/pop/fly/pulse + typewriter + pulse_on_beat + ken_burns + unit tests
- [x] L2: `add_smart_lower_third` + example
- [x] L3: >=2 composition recipes green under examples harness + CLI list
- [ ] L1 catalogued in agent-facing docs / api surface list (if catalog tracks scripting helpers)
- [ ] At least one recipe path with **real media props** (image paths), not only typography
- [ ] Second vertical recipe family OR documented stop-line for v1 stack

Until those last boxes flip, **do not mark this plan Done.**
