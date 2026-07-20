# Purpose

- Own **active** engineering plans for Director, especially the core-hardening program.

## Ownership

- `00-core-hardening-index.md` — master sequence, **verified truth snapshot**, non-goals, definition of done. Truth pass is complete; do not re-open product scope from `docs/archive/` without updating 00.
- `01`–`06` — time, identity, pipeline, animation eval, scene model, determinism/media. **Done.**
- `07-stdlib-and-recipes.md` — L1 motion stdlib + L2 component contract + L3 recipes on the current stack (no SDK crate).

## Local Contracts

- Plans are the implementation contract until marked done or superseded.
- Status values: `Spec` | `In progress` | `Done` | `Superseded`.
- When a plan completes, update the index checkboxes/definition of done and `architecture/core-contracts.md` gaps column.
- Deferred product plans live in `../archive/`, not here.

## Work Guidance

- Implement in index order unless a plan explicitly allows parallel work.
- Prefer package-scoped tests while iterating; workspace checks before closeout.
- Composition CLI + preview already landed; recipe/stdlib work is plan 07 (not archive C / director-sdk).

## Verification

- Per-plan acceptance tests plus root workspace commands in `00-core-hardening-index.md`.

## Child DOX Index

- No child AGENTS.md files required.
