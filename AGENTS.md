# DOX framework

- DOX is highly performant AGENTS.md hierarchy installed here
- Agent must follow DOX instructions across any edits
- This repository is Director, a Rust workspace for programmatic video rendering with Rhai scripting, Skia rendering, Taffy layout, Lottie support, a CLI, developer introspection tools, and a React preview frontend.

## Core Contract

- AGENTS.md files are binding work contracts for their subtrees
- Work products, source materials, instructions, records, assets, and durable docs must stay understandable from the nearest applicable AGENTS.md plus every parent AGENTS.md above it

## Read Before Editing

1. Read the root AGENTS.md
2. Identify every file or folder you expect to touch
3. Walk from the repository root to each target path
4. Read every AGENTS.md found along each route
5. If a parent AGENTS.md lists a child AGENTS.md whose scope contains the path, read that child and continue from there
6. Use the nearest AGENTS.md as the local contract and parent docs for repo-wide rules
7. If docs conflict, the closer doc controls local work details, but no child doc may weaken DOX

Do not rely on memory. Re-read the applicable DOX chain in the current session before editing.

## Update After Editing

Every meaningful change requires a DOX pass before the task is done.

Update the closest owning AGENTS.md when a change affects:

- purpose, scope, ownership, or responsibilities
- durable structure, contracts, workflows, or operating rules
- required inputs, outputs, permissions, constraints, side effects, or artifacts
- user preferences about behavior, communication, process, organization, or quality
- AGENTS.md creation, deletion, move, rename, or index contents

Update parent docs when parent-level structure, ownership, workflow, or child index changes. Update child docs when parent changes alter local rules. Remove stale or contradictory text immediately. Small edits that do not change behavior or contracts may leave docs unchanged, but the DOX pass still must happen.

## Hierarchy

- Root AGENTS.md is the DOX rail: project-wide instructions, global preferences, durable workflow rules, and the top-level Child DOX Index
- Child AGENTS.md files own domain-specific instructions and their own Child DOX Index
- Each parent explains what its direct children cover and what stays owned by the parent
- The closer a doc is to the work, the more specific and practical it must be

## Child Doc Shape

- Create a child AGENTS.md when a folder becomes a durable boundary with its own purpose, rules, responsibilities, workflow, materials, or quality standards
- Work Guidance must reflect the current standards of the project or user instructions; if there are no specific standards or instructions yet, leave it empty
- Verification must reflect an existing check; if no verification framework exists yet, leave it empty and update it when one exists

Default section order:
- Purpose
- Ownership
- Local Contracts
- Work Guidance
- Verification
- Child DOX Index

## Style

- Keep docs concise, current, and operational
- Document stable contracts, not diary entries
- Put broad rules in parent docs and concrete details in child docs
- Prefer direct bullets with explicit names
- Do not duplicate rules across many files unless each scope needs a local version
- Delete stale notes instead of explaining history
- Trim obvious statements, repeated rules, misplaced detail, and warnings for risks that no longer exist

## Closeout

1. Re-check changed paths against the DOX chain
2. Update nearest owning docs and any affected parents or children
3. Refresh every affected Child DOX Index
4. Remove stale or contradictory text
5. Run existing verification when relevant
6. Report any docs intentionally left unchanged and why

## User Preferences

When the user requests a durable behavior change, record it here or in the relevant child AGENTS.md

- Favor bleeding-edge iteration over release-stage conservatism; do not slow work by prioritizing fallback paths unless the task explicitly asks for compatibility behavior.
- Reduce user cognitive load: make concrete changes, surface concise tradeoffs, and keep operational instructions direct.
- When asked for advice, provide options rather than a single path.
- Never use `pip` for Python package installation; use `uv`.
- Do not use the phrasing the user has forbidden in project instructions.
- Core-hardening plans 01–06 are Done. Named compositions + subcommand CLI + live preview are in tree; recipes/agent repair remain later product work.

## Work Guidance

- Use Cargo workspace conventions from `Cargo.toml`; prefer package-scoped checks while iterating and workspace checks before broad closeout.
- Keep Rust crate ownership clear: public API and shared runtime behavior belong in `crates/director-core`, command-line concerns in `crates/director-cli`, schema reflection in `crates/director-schema`, and Lottie-specific logic in the `lottie-*` crates.
- Rhai examples are executable reference material. Update examples and tests when scripting-facing behavior changes.
- Generated or downloaded assets should not become durable source unless the relevant AGENTS.md explicitly says they are owned there.
- For Python scripts, keep dependencies executable through existing tooling and install packages only with `uv` when installation is necessary.
- **Core hardening (01–06) is Done.** Product surface now includes named compositions + props + subcommand CLI + live preview. Still deferred: recipes, agent repair loops, SDK/template layers (`docs/archive/`). Keep core contracts green when changing engine spines.
- Longer-term product direction (after core gates): autonomous high-quality video production for short-form and long-form (Shorts, Reels, TikTok, YouTube). Avoid optimizing for low-quality generated output. Web research as an input belongs at orchestration layers, not in the render core.
- `vendor/` contains patched copies of third-party crates that cannot be fixed via `[patch.crates-io]` to upstream. Do not modify vendor sources without updating the corresponding `AGENTS.md`.
- `vendor/skia-safe/` is skia-safe 0.90.0 with two patches keeping the Vulkan `get_proc` thread-local resolver alive for lazy resolution (readback/flush). If the dependency is ever bumped, reapply the patches from the diff in `vendor/AGENTS.md`.

## Verification

- Rust workspace: `cargo check --workspace --all-targets`
- Rust tests: `cargo test --workspace`
- Rust lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Visual regression: `cargo test --test visual_suite --no-default-features --features mock_video` from `crates/director-core`


## Child DOX Index

- `.agent/AGENTS.md` - Local agent skills and reference material for Director-specific workflows.
- `.github/AGENTS.md` - GitHub workflows and issue templates.
- `.grits/AGENTS.md` - Local issue/focus records.
- `crates/AGENTS.md` - Rust workspace crates and package-level ownership.
- `docs/AGENTS.md` - Architecture, core contracts, active plans, contributing, archive.
- `examples/AGENTS.md` - Rhai example scripts, rendered media references, and script validation helpers.
- `specs/AGENTS.md` - RON feature specs consumed by the developer harness.
- `vendor/AGENTS.md` - Vendored third-party crates with durable patches.
