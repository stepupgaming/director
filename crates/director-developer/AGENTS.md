# Purpose

- Own `director-dev`, the CLI that introspects Director's current APIs, schemas, pipeline capabilities, specs, prompts, and dependency graph.

## Ownership

- `src/reflectors/` owns system-truth extraction.
- `src/spec.rs`, `synthesizer.rs`, `prompts.rs`, and `watch.rs` own spec validation, context generation, prompt generation, and watch behavior.
- `prompts/` owns reusable prompt templates.
- `examples/` owns sample feature specs.
- `README.md` owns developer-facing command documentation.

## Local Contracts

- Reflected output must come from current code, not copied documentation.
- Command documentation, skill instructions, and `.director-dev/` generated files must be updated when commands or output shape changes.
- Feature specs are RON and should remain validated by the CLI.

## Work Guidance

- Run introspection commands before implementing cross-layer Director features.
- Keep reflectors focused by source of truth: scripting, schema, pipeline, or graph.

## Verification

- `cargo test -p director-developer`
- `cargo run -p director-developer -- info`
- `cargo run -p director-developer -- dump --output .director-dev` when reflected truth changes.

## Child DOX Index

- No child AGENTS.md files currently required.
