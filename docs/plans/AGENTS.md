# Purpose

- Own engineering plans for Director: the completed core-hardening program and the phases that followed it.

## Ownership

- `00-core-hardening-index.md` - master sequence, **verified truth snapshot**, non-goals, definition of done. Truth pass is complete; do not re-open product scope from `docs/archive/` without updating 00.
- `01`–`06` - time, identity, pipeline, animation eval, scene model, determinism/media. **Done.**
- `07-stdlib-and-recipes.md` - L1 motion stdlib + L2 component contract + L3 recipes on the current stack (no SDK crate). **Done at the v1 stop-line.**
- `08-native-projects-and-runtime.md` - portable HTML/capture projects, validated CLI edits, common input loading and native backend execution. **Done.**
- `09-native-fidelity.md` - native paint and media fidelity. **Superseded.** Keeps the implemented list, Hyper CN smoke coverage and the refusal backlog. Feature contracts live in `crates/director-html/AGENTS.md`.
- `10-native-platform-overhaul.md` - scene document, parallel renderer, host session, HTML-first authoring and proof. **In progress.** It is the current-state and handoff document: phases and gates, the latest measurement table, and the next action.
- `11-transcription-and-competitive-gaps.md` - optional `director-asr` crate with transcript-driven captions and edits, the corrected FFrames scorecard, distribution and the head-to-head benchmark protocol. **Spec.** Its checks record measurements in the plan 10 table while plan 10 is active.

## Local Contracts

- Plans are the implementation contract until marked done or superseded.
- Status values: `Spec` | `In progress` | `Done` | `Superseded`.
- When a plan completes, update the index checkboxes/definition of done and `architecture/core-contracts.md` gaps column.
- Deferred product plans live in `../archive/`, not here.
- Only the active plan carries test counts. Done plans keep their acceptance record and link forward instead of restating later status.

## Work Guidance

- Implement in index order unless a plan explicitly allows parallel work.
- Prefer package-scoped tests while iterating; workspace checks before closeout.
- Plan 10 phases run in the order the plan states; its typed computed style work may start once phase 1 lands.

## Verification

- Per-plan acceptance tests plus root workspace commands in `00-core-hardening-index.md`.

## Child DOX Index

- No child AGENTS.md files required.
