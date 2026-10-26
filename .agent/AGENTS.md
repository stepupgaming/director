# Purpose

- Own local agent-facing skills, reusable Director instructions, API references, and example prompts.

## Ownership

- `.agent/skills/director-rhai/` owns Rhai script-authoring instructions, examples, and API reference material.
- `.agent/skills/director-agent-loop/` owns the check, sheet, audio, still, and render loop, including snapshot, onion, and draft scale.
- `.agent/skills/director-mcp/` owns the MCP skill (`SKILL.md` and `tools.json`) that `director-engine skills .agent/skills` generates from the MCP tool schemas in `crates/director-cli/src/skills.rs`.
- Root AGENTS.md owns broad agent behavior; this file owns only local skill assets.

## Local Contracts

- Keep skill instructions aligned with actual APIs in `crates/director-core`.
- Do not place runtime application code here.
- `director-mcp` is generated output. Change the tool argument types, descriptions or `src/skills.rs`, then regenerate it; hand edits fail the director-cli freshness test.
- References intended for agents must be concise, operational, and kept current when the source APIs change.

## Work Guidance

- When changing Rhai API behavior, update `.agent/skills/director-rhai/resources/API_REFERENCE.md` if the existing reference becomes stale.

## Verification

- Validate skill examples by running the relevant Director CLI or package tests when the referenced API changes.
- `cargo test -p director-cli --release skills` fails when `director-mcp` differs from the generated skill.

## Child DOX Index

- `skills/director-rhai/` - Rhai authoring skill, examples, and API reference.
- `skills/director-agent-loop/` - CLI loop for check, sheet, audio, still, snapshot, onion, and render.
- `skills/director-mcp/` - Generated MCP tool skill: setup, loop, tool arguments and error codes.
