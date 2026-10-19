# Purpose

- Own durable documentation for architecture, contribution workflow, and engineering plans.

## Ownership

- `architecture/` - engine overview, **normative core contracts**, and reproducible measurements plus the baseline-tracking workflow in `render-performance.md`.
- `plans/` - engineering plans: completed core hardening, motion/recipes, native projects/runtime, and the active native fidelity phase.
- `contributing/` - development and documentation workflow.
- `archive/` - superseded or deferred docs (not current contracts).

## Local Contracts

- Documentation must describe **current** behavior or clearly marked **planned** behavior.
- Remove or archive stale claims when code diverges; do not leave progress tables that oversell the tree.
- Active engine work is guided by `plans/` + `architecture/core-contracts.md`, not by archived audit/product drafts.
- Cross-reference crates, examples, and tests when docs describe executable behavior.
- The active plan holds current status, the latest measurement table, and the next action. Parent indexes link to it and do not copy its status or counts.

## Work Guidance

- Prefer updating `core-contracts.md` in the same change as spine behavior.
- New multi-phase work gets a plan under `plans/` and a line in `plans/00-core-hardening-index.md` (or a successor index).
- Do not resurrect archived SDK/recipe/composition plans as current priority without an explicit product decision after core gates.

## Verification

- Docs-only: check links and paths.
- With code: run the verification listed in the touched plan / root `AGENTS.md`.

## Child DOX Index

- `architecture/` - Overview, core contracts, and render performance measurements. Raw benchmark outputs stay under ignored `target/`.
- `plans/` - Plan index (00), completed plans 01–08, and the active plan 09 (native fidelity), which is the current-state and handoff document.
- `contributing/` - Development and documentation workflow.
- `archive/` - Historical audit and deferred product docs.
