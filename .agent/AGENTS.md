# Purpose

- Own local agent-facing skills, reusable Director instructions, API references, and example prompts.

## Ownership

- `.agent/skills/director-rhai/` owns Rhai script-authoring instructions, examples, and API reference material.
- `.agent/skills/director-developer/` owns instructions for using the `director-developer` introspection CLI.
- Root AGENTS.md owns broad agent behavior; this file owns only local skill assets.

## Local Contracts

- Keep skill instructions aligned with actual APIs in `crates/director-core` and current generated truth in `.director-dev/`.
- Do not place runtime application code here.
- References intended for agents must be concise, operational, and kept current when the source APIs change.

## Work Guidance

- When changing Rhai API behavior, update `.agent/skills/director-rhai/resources/API_REFERENCE.md` if the existing reference becomes stale.
- When changing introspection commands or outputs, update `.agent/skills/director-developer/SKILL.md`.

## Verification

- Validate skill examples by running the relevant Director CLI or package tests when the referenced API changes.

## Child DOX Index

- `skills/director-rhai/` - Rhai authoring skill, examples, and API reference.
- `skills/director-developer/` - Developer introspection skill instructions.
