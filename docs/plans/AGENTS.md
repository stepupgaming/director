# Purpose

- Own **active** engineering plans for Director, especially the core-hardening program.

## Ownership

- `00-core-hardening-index.md` — master sequence, **verified truth snapshot**, non-goals, definition of done. Truth pass is complete; do not re-open product scope from `docs/archive/` without updating 00.
- `01`–`06` — time, identity, pipeline, animation eval, scene model, determinism/media. **Done.**
- `07-stdlib-and-recipes.md` — L1 motion stdlib + L2 component contract + L3 recipes on the current stack (no SDK crate).
- `08-native-projects-and-runtime.md` — portable HTML/capture projects, validated CLI edits, common input loading and native backend execution. Frontend/editor design and Remotion compatibility are outside its scope. **Done.**
- `09-native-fidelity.md` — native paint fidelity. Slice 1 and capture forwarding are implemented. The carried-forward smoke is 213/239. Next is shader translation for 26 canvas/WebGL blocks. Static-subtree reuse is issue #2. Agent production workflows are issue #3.

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
