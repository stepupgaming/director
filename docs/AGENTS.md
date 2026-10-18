# Purpose

- Own durable documentation for architecture, contribution workflow, technical specs, and active engineering plans.

## Ownership

- `architecture/` — engine overview, **normative core contracts**, and reproducible measurements plus the baseline-tracking workflow in `render-performance.md`.
- `plans/` — engineering plans: completed core hardening, motion/recipes, native projects/runtime, and the native fidelity phase.
- `contributing/` — development and documentation workflow.
- `specs/` — RON / harness-oriented feature specs (see also repo-root `specs/`).
- `archive/` — superseded or deferred docs (not current contracts).

## Local Contracts

- Documentation must describe **current** behavior or clearly marked **planned** behavior.
- Remove or archive stale claims when code diverges; do not leave progress tables that oversell the tree.
- Active engine work is guided by `plans/` + `architecture/core-contracts.md`, not by archived audit/product drafts.
- Cross-reference crates, examples, and tests when docs describe executable behavior.

## Work Guidance

- Prefer updating `core-contracts.md` in the same change as spine behavior.
- New multi-phase work gets a plan under `plans/` and a line in `plans/00-core-hardening-index.md` (or a successor index).
- Do not resurrect archived SDK/recipe/composition plans as current priority without an explicit product decision after core gates.

## Verification

- Docs-only: check links and paths.
- With code: run the verification listed in the touched plan / root `AGENTS.md`.

## Child DOX Index

- `architecture/` — Overview, core contracts, and render performance measurements. Raw benchmark outputs stay under ignored `target/`.
- `plans/` — Engineering plan index (00), completed core hardening (01–06), motion/recipes (07), native projects/runtime (08), and native fidelity (09: slice 1, capture forwarding, Paper Shader translation, captured muted video and audio, gradient color hints, nested preserve-3d, external SVG presentation, per-layer background lists, containing-block mixed calc, SVG markers and masks, SVG text, authored SVG foreignObject, authored SVG SMIL, definite-parent subgrid, single-frame fixed and sticky, and the 2026-10-17 release run of lib 101, paint_gaps 22, background_layers 5, mixed_calc 4, and svg_markers 3; a 2026-10-18 run passed mixed_calc 12 and gradient_calc 3; a 2026-10-18 run of `cargo test -p director-html --release --lib --test grid_lines subgrid` passed lib 8, 101 filtered out, and grid_lines 1, 1 filtered out; a 2026-10-18 run of `cargo test -p director-html --release --test fixed_position --test css_media --test css_nesting` passed fixed_position 5, css_media 6, and css_nesting 12; a 2026-10-18 run of `cargo test -p director-html --release --test svg_presentation --test svg_markers` passed svg_presentation 12 and svg_markers 3; a later 2026-10-18 run of the same command passed svg_presentation 12 and svg_markers 6; a later 2026-10-18 run of the same command passed svg_presentation 12 and svg_markers 12).
- `contributing/` — Development and documentation workflow.
- `specs/` — Reserved for docs-local design notes (may be empty; harness RON specs live in repo-root `specs/`).
- `archive/` — Historical audit and deferred product docs.
