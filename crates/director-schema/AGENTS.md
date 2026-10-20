# Purpose

- Own schema definitions and schema reflection support for Director configuration and data structures.

## Ownership

- `src/lib.rs` owns exported schema types.
- `README.md` owns crate-level schema usage notes.

## Local Contracts

- Types intended for developer introspection must derive or support JSON schema generation.
- `EffectConfig` and `TransitionType` are **codegen'd** from `director-core::api_catalog` via `build.rs`; edit the catalog, not the generated output.
- `NodeKind` must stay in sync with `director-core::api_catalog` JSON-schema entries (enforced by `test_catalog_node_sync`).
- Rhai `add_loop` / `add_freeze` map to `Scene.loop_count` and `Scene.post_hold` in `MovieRequest`.
- `manifest::api_manifest_json()` and `export_movie_request_schema()` are the agent-facing schema exports.
- `project_manifest::build_project_manifest()` emits a static agent summary (timeline, nodes, assets) from `MovieRequest` without rendering.
- Schema changes may require updates in docs and specs.

## Work Guidance

- Keep schema types stable, serializable, and explicit about optional fields.
- Coordinate changes with `director-core` when schemas mirror runtime types.
- After changing `api_catalog.rs`, run `cargo test -p director-schema` to regenerate and verify enums.

## Verification

- `cargo test -p director-schema`
- `test_catalog_schema_sync` — every catalog effect/transition deserializes into schema types
- `test_catalog_node_sync` — every `json_schema` catalog node deserializes into `NodeKind`

## Child DOX Index

- No child AGENTS.md files currently required.
