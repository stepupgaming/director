# Purpose

- Own schema definitions and schema reflection support for Director configuration and data structures.

## Ownership

- `src/lib.rs` owns exported schema types.
- `README.md` owns crate-level schema usage notes.

## Local Contracts

- Types intended for developer introspection must derive or support JSON schema generation.
- Schema changes may require updates in `crates/director-developer`, `.director-dev/`, docs, and specs.

## Work Guidance

- Keep schema types stable, serializable, and explicit about optional fields.
- Coordinate changes with `director-core` when schemas mirror runtime types.

## Verification

- `cargo test -p director-schema`
- `cargo run -p director-developer -- dump --output .director-dev` when reflected schema changes.

## Child DOX Index

- No child AGENTS.md files currently required.
