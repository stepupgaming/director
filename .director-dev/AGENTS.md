# Purpose

- Own generated system-truth dumps produced by `director-developer`.

## Ownership

- `rhai_api.json` records reflected Rhai API metadata.
- `schemas.json` records reflected schema metadata.
- `pipeline_capabilities.json` records reflected node, effect, transition, and pipeline capabilities.

## Local Contracts

- Treat files in this directory as generated artifacts.
- Regenerate from the CLI instead of editing JSON by hand unless the task is specifically to repair broken generated output.
- Keep this directory synchronized with schema, scripting API, and pipeline capability changes.

## Work Guidance

- Preferred refresh command: `cargo run -p director-developer -- dump --output .director-dev`.
- Review diffs after regeneration; generated truth changes can reveal unintended API changes.

## Verification

- Run `cargo run -p director-developer -- info` after changing the generator.
- Run `cargo run -p director-developer -- dump --output .director-dev` when source truth changes.

## Child DOX Index

- No child AGENTS.md files currently required.
