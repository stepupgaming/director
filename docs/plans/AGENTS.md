# Purpose

- Own engineering plans for Director: the completed core-hardening program and the phases that followed it.

## Ownership

- `00-core-hardening-index.md` - master sequence, **verified truth snapshot**, non-goals, definition of done. Truth pass is complete; do not re-open product scope from `docs/archive/` without updating 00.
- `01`–`06` - time, identity, pipeline, animation eval, scene model, determinism/media. **Done.**
- `07-stdlib-and-recipes.md` - L1 motion stdlib + L2 component contract + L3 recipes on the current stack (no SDK crate). **Done at the v1 stop-line.**
- `08-native-projects-and-runtime.md` - portable HTML/capture projects, validated CLI edits, common input loading and native backend execution. **Done.**
- `09-native-fidelity.md` - native paint and media fidelity. **In progress.** It is the current-state and handoff document: what is implemented, Hyper CN smoke coverage, the latest measurement table, known refusals, and the next action. Feature contracts live in `crates/director-html/AGENTS.md`.

## Local Contracts

- Plans are the implementation contract until marked done or superseded.
- Status values: `Spec` | `In progress` | `Done` | `Superseded`.
- When a plan completes, update the index checkboxes/definition of done and `architecture/core-contracts.md` gaps column.
- Deferred product plans live in `../archive/`, not here.
- Only the active plan carries test counts. Done plans keep their acceptance record and link forward instead of restating later status.

## Work Guidance

- Implement in index order unless a plan explicitly allows parallel work.
- Prefer package-scoped tests while iterating; workspace checks before closeout.
- Agent production workflows after plan 09 are tracked in GitHub issue #3.

## Verification

- Per-plan acceptance tests plus root workspace commands in `00-core-hardening-index.md`.

## Child DOX Index

- No child AGENTS.md files required.
