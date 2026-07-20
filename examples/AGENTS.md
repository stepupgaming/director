# Purpose

- Own Rhai example scripts, rendered reference media, and validation helpers.

## Ownership

- `basics/` owns minimal reference scripts for core Rhai API usage.
- `features/` owns scripts demonstrating higher-level rendering, media, effects, transitions, and hardware encoding examples.
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
- Keep test helper behavior deterministic enough for CI or local validation.

## Verification

- `cargo test -p director-core examples`
- Render touched scripts with `cargo run --release -- examples/<path>.rhai output.mp4` when visual behavior matters.
- Run relevant Python helper scripts through the repository's available Python tooling; install packages only with `uv` when needed.

## Child DOX Index

- `basics/` - Introductory Rhai scripts.
- `features/` - Feature showcase Rhai scripts and media references.
- `tests/` - Script tests, visual diff helpers, and reference media.
