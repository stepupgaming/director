# Plan 07 — L1 Stdlib + L2 Components + L3 Recipes

> **Status:** Done (v1 stop-line, 2026-07-19)
> **Stack:** L0 core (done) → L1 motion verbs → L2 templates/compositions → L3 recipes
> **Not in scope:** `director-sdk` crate, binding migration, capability matrix (Option C deferred)

## Honest scope

| Layer | Done means | Status |
|-------|------------|--------|
| **L0** | Engine spines 01–06 green | **Done** |
| **L1** | Archive-class macros: fade/pop/fly + **typewriter**, **pulse_on_beat**, **ken_burns**; tests; examples; catalogued | **Done (v1)** |
| **L2** | Layout templates **and** at least one **content-reactive** component (`add_smart_lower_third`); composition+props as the reusable unit | **Done (v1)** — not a full component library |
| **L3** | Multiple recipe arcs under `examples/recipes/`, each `register_composition`, CLI/preview green; media props path | **Done (v1)** |

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

Catalogued in `RHAI_FUNCTION_CATALOG` (`api_catalog.rs`) for agent/schema surface.

## L2 contract

- Layout sugar: existing `add_title_card`, `add_lower_third`, quote/progress/bento/...
- **Smart:** `add_smart_lower_third` — name/title stack, accent bar, width from content length (clamped).
- Reusable unit with props: `register_composition`.

## L3 recipes (v1)

| Recipe | Composition | Arc |
|--------|-------------|-----|
| `examples/recipes/faceless_short.rhai` | `faceless_short` | 9:16 kinetic explainer beats |
| `examples/recipes/kinetic_lower_third.rhai` | `kinetic_lower_third` | 16:9 interview plate + typewriter + smart L3 |
| `examples/recipes/image_beats.rhai` | `image_beats` | 9:16 media props + Ken Burns + captions |

### Explicit non-goals (deferred — not v1)

- Faceless Historian with **TTS / Supertonic VO**
- Podcast Clipper with **energy-based auto-cut + speaker reframes**
- Full smart-component library beyond `add_smart_lower_third` + templates
- `director-sdk` crate / Option C binding migration
- Asset-store / `.dirproj` component distribution

## v1 stop-line

Plan 07 **stops here**. Further authoring work is additive (more recipes, more L1 verbs as needed, more L2 components) and does **not** reopen this plan unless a contract breaks.

What v1 guarantees:

1. L1 motion pack on `NodeHandle` with unit tests + `stdlib_motion` example.
2. L2 smart lower third + composition registry + props merge.
3. ≥3 L3 recipes, including one with **real image path props** (`image_beats`).
4. Agent-facing catalog entries for L1/L2 helpers.
5. Examples harness validates recipe scripts (Movie or composition builders).

What v1 does **not** claim:

- Production TTS/VO pipelines
- Podcast auto-edit
- Exhaustive component catalog
- Separate SDK crate

## Verification

```bash
cargo test -p director-core --lib stdlib
cargo test -p director-core --test examples stdlib_motion
cargo test -p director-core --test examples smart_lower_third
cargo test -p director-core --test examples faceless_short
cargo test -p director-core --test examples kinetic_lower_third
cargo test -p director-core --test examples image_beats
```

CLI smoke (optional local):

```bash
cargo run -p director-cli -- compositions examples/recipes/image_beats.rhai --json
cargo run -p director-cli -- still examples/recipes/image_beats.rhai --composition image_beats -t 1 -o /tmp/image_beats.png
```

## Done when (plan exit)

- [x] L1: fade/pop/fly/pulse + typewriter + pulse_on_beat + ken_burns + unit tests
- [x] L2: `add_smart_lower_third` + example
- [x] L3: >=2 composition recipes green under examples harness + CLI list
- [x] L1 catalogued in agent-facing docs / api surface list (`RHAI_FUNCTION_CATALOG` + DOX)
- [x] At least one recipe path with **real media props** (image paths): `image_beats`
- [x] Documented v1 stop-line (this section) — second vertical family deferred with Historian/Clipper

**Plan 07 = Done at the v1 stop-line.**
