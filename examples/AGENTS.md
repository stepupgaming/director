# Purpose

- Own Rhai example scripts, rendered reference media, and validation helpers.

## Ownership

- `basics/` owns minimal reference scripts for core Rhai API usage.
- `features/` owns scripts demonstrating higher-level rendering, media, effects, transitions, and hardware encoding examples.
- `recipes/` owns L3 full-arc composition scripts (props in, finished show out).
- `html/` owns native HTML/CSS/HyperFrames compiler reference compositions.
- `tests/` owns test scripts, visual comparison helpers, and reference media.
- Root example `.rhai` files own broader manual or preview scenarios.

## Local Contracts

- Examples are executable reference material and should stay aligned with the Rhai API.
- Keep generated media only when it serves as a durable reference or test artifact.
- Python helper scripts must not require package installation through `pip`; use `uv` if dependencies need installation.

## Work Guidance

- Update examples when user-facing scripting behavior changes.
- Prefer small, readable scripts for basics and richer compositions for feature demonstrations.
- `features/compositions.rhai` demonstrates `register_composition` + default_props; validation accepts either a Movie return or a non-empty composition registry (builders exercised with defaults).
- `features/stdlib_motion.rhai` covers L1 motion verbs (`fade_in`, `pop_in`, `fly_in_*`, `pulse`, `typewriter`, `ken_burns`, `pulse_on_beat`).
- `recipes/faceless_short.rhai` — L3 vertical kinetic explainer.
- `recipes/kinetic_lower_third.rhai` — L3 16:9 interview package (typewriter + smart lower third).
- `recipes/image_beats.rhai` — L3 vertical media-prop recipe (image paths + Ken Burns + captions).
- `recipes/stat_count.rhai` — L3 9:16 stepped count (`stat_count`).
- `recipes/quote_card.rhai` — L3 9:16 quote and attribution (`quote_card`).
- `recipes/list_reveal.rhai` — L3 16:9 three-line reveal (`list_reveal`).
- `recipes/end_card.rhai` — L3 9:16 end card (`end_card`).
- `recipes/chapter_card.rhai` — L3 16:9 chapter card (`chapter_card`).
- `recipes/countdown.rhai` — L3 9:16 stepped countdown (`countdown`). The 2026-10-17 release examples filter passed 9 recipe scripts, including these three.
- `features/smart_lower_third.rhai` — L2 content-reactive lower third demo.
- Keep test helper behavior deterministic enough for CI or local validation.

## Verification

- `cargo test -p director-core examples`
- Render touched scripts with `cargo run --release -- examples/<path>.rhai output.mp4` when visual behavior matters.
- Run relevant Python helper scripts through the repository's available Python tooling; install packages only with `uv` when needed.

## Child DOX Index

- `basics/` - Introductory Rhai scripts.
- `html/AGENTS.md` - Native HTML reference compositions and render commands.
- `features/` - Feature showcase Rhai scripts and media references.
- `tests/` - Script tests, visual diff helpers, and reference media.
