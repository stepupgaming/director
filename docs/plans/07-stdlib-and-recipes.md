# Plan 07 — L1 Stdlib + L2 Components + L3 Recipes

> **Status:** Done (2026-07-19)  
> **Stack:** L0 core (done) → L1 motion verbs → L2 templates/compositions → L3 recipes  
> **Not in scope:** `director-sdk` crate, binding migration, capability matrix (Option C deferred)

## Goal

Ship the full authoring stack **on the current architecture** (A+B hybrid):

| Layer | Meaning | Home |
|-------|---------|------|
| **L0** | Engine + raw Rhai | `director-core` (plans 01–06) |
| **L1** | Motion / behavior macros | `scripting/api/stdlib.rs` |
| **L2** | Structural components | `templates.rs` + `register_composition` + props |
| **L3** | Full video arcs | `examples/recipes/*.rhai` |

## Design rules

1. No new crate. L1 registers on the same engine as L0.
2. L1 is thin wrappers over keyframe `animate` (one eval path).
3. L2 API = existing `add_*` templates **or** `register_composition` with shallow props.
4. Every L3 recipe **registers at least one composition** so CLI `compositions` / `preview` / `render --composition` work.
5. Recipes take props (title, beats, colors, assets); no hidden globals.
6. Quality bar: intentional short-form look; optional style_policy later.

## L1 verbs (v1)

| Verb | Behavior |
|------|----------|
| `fade_in(dur[, delay])` | opacity 0→1 ease_out |
| `fade_out(dur[, delay])` | opacity 1→0 ease_in |
| `pop_in(dur[, delay])` | scale + opacity bounce |
| `fly_in_up/down/left/right(dur[, delay[, px]])` | translate + fade |
| `fly_out_up/down/left/right(dur[, delay[, px]])` | translate + fade |
| `pulse(dur[, delay[, peak]])` | scale 1→peak→1 |

## L2 contract

- **Layout sugar:** `add_title_card`, `add_lower_third`, `add_quote_card`, `add_progress_bar`, `add_bento_grid`, `add_split_screen`, `add_reaction_overlay`, `add_watermark`, `add_subscribe_button`, `add_countdown_timer`, `add_social_handle_overlay`.
- **Reusable unit with props:** `register_composition(name, meta, \|props\| { ... })`.
- Props merge is **shallow** (composition registry).

## L3 first recipe

`examples/recipes/faceless_short.rhai` — vertical 1080×1920 kinetic explainer:

- Props: title, beats (array of strings), accent, bg
- Scenes: cold open title → beat cards → end card
- Uses L1 verbs + L2 title/quote-style layouts
- Composition name: `faceless_short`

## Verification

```bash
cargo test -p director-core --lib stdlib
cargo test -p director-core --test examples example_recipes_faceless_short
cargo run -p director-cli --bin director-engine -- compositions examples/recipes/faceless_short.rhai --json
cargo run -p director-cli --bin director-engine -- still examples/recipes/faceless_short.rhai --composition faceless_short -t 1 -o output/faceless.png
```

## Done when

- [x] L1 stdlib registered with unit tests
- [x] L2 contract documented (this plan + AGENTS)
- [x] L3 `faceless_short` recipe green under examples harness
- [ ] (follow-up) more L1 verbs / second recipe as needed
