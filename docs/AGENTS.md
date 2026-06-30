# Purpose

- Own durable documentation for architecture, contribution workflow, and technical specifications.

## Ownership

- `architecture/` owns engine overview and roadmap-style architecture documentation.
- `contributing/` owns development and documentation workflow.
- `specs/` owns technical architecture and API strategy documents.

## Local Contracts

- Documentation must describe current or intentionally planned behavior clearly enough to guide implementation.
- Remove stale claims when source code or workflow changes.
- Cross-reference examples, specs, and crates when docs describe executable behavior.

## Work Guidance

- Keep broad architecture in `architecture/`, process in `contributing/`, and detailed technical specs in `specs/`.
- Update docs when public APIs, workflows, or architecture responsibilities change.

## Verification

- For docs-only edits, review links and referenced paths.
- Run code verification when docs are changed alongside implementation.

## Child DOX Index

- `architecture/` - Overview documents.
- `contributing/` - Development and documentation workflow.
- `specs/` - Technical API and architecture specification documents.
