# DOX framework

- DOX is highly performant AGENTS.md hierarchy installed here
- Agent must follow DOX instructions across any edits
- This repository is Director, a Rust workspace for programmatic video rendering with Rhai scripting, a native HTML/CSS compiler, Skia rendering, Taffy layout, Lottie support, and a CLI with agent inspection tools, an MCP server and an editor protocol.

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
- Test counts live only in the measurement table of the active plan (`docs/plans/10-native-platform-overhaul.md`). Keep one row per check with its latest count, date, and exact command, and replace the row on re-run. Contract docs state behavior without counts.

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
- When asked for advice, provide options rather than a single path. When a chosen option finishes, report the measured result and the next steps.
- Never use `pip` for Python package installation; use `uv`.
- Writing style for docs, commits, and replies: affirmative statements, no em dashes, and no contrastive "X, not Y" constructions.
- Director's product goal is to replace HyperFrames, FFrames and Remotion with a superior native video platform. Assess authoring, motion and media quality, preview/editing, agent tooling, deployment and measured rendering efficiency together; compiler throughput alone does not establish product superiority.
- Director is library-first. Delete in-repo frontend code; a future editor (a Tauri app in the style of the HyperFrames and Remotion studios) links the host session API. Remotion source compatibility stays out of scope.
- Major overhauls are welcome. Earlier architecture came from weaker models; judge it on merit and replace it when a better design wins.
- HTML/CSS is the primary authoring front door. Rhai is an optional generator.
- Build Director with `cargo --release`. Do not populate `target/debug`. Rustc temp may go on `F:\cargo-tmp` when `C:` is tight.
- Do not use git worktrees. Keep every change in the shared `C:\Projects\director` tree.
- Use parallel subagents on file-disjoint slices when it helps. Default subagents to Grok 4.7 (`grok-4.7-xhigh`). Give them explicit step-by-step prompts with exact files, allowed edits and the report format, and review their diffs before landing. Subagents do not compile, run tests, commit, or edit `vendor/`, `Cargo.toml`, `Cargo.lock`, AGENTS.md, README, or docs. The parent runs one release test pass after their edits land, does the DOX pass, and commits.
- Commit each finished slice with one imperative sentence. Never commit `.freebuff/`. Push only when asked.
- Ask before GPU- or browser-heavy work: the Hyper CN registry smoke, Chrome launches, and long benchmarks.
- On a Windows `LNK1181` link failure for skia-bindings 0.153.3, restore `skia.lib` and `skunicode_icu.lib` beside `skunicode_core.lib` from the prebuilt `ganesh-jpegd-jpege-pdf-textlayout-vulkan` archive. Do not start a Skia source build.
- Every build, test run and `-p` subset compiles Skia with Ganesh and Vulkan (the workspace `skia-safe` features), and `vulkan` is a default feature of `director-render`, `director-host` and `director-cli`. `--gpu` picks the GPU at run time. Keep features on for every command, because a feature that only some commands turn on gives every crate above Skia another copy. Dev-dependencies and `-p` subsets still resolve features their own way, so `cargo build -p director-cli`, `cargo test --workspace` and each `cargo test -p <crate>` keep their own set of copies (about 0.6 GB each). `cargo test --workspace` and `cargo build -p director-cli` relink the `director-cli` binaries for each other, which takes about 6 s.
- Each Skia-linking crate builds its integration tests as one `integration` binary. `tests/main.rs` declares every `tests/<name>.rs` as `mod <name>;`, and its `every_test_file_is_a_module` test fails when a file is missing. Run one file with `cargo test -p <crate> --release --test integration <name>::`. The files share one process, so a test must not change env vars or the current directory. A test that reads its own log lines under `tracing::subscriber::with_default` calls `tracing::callsite::rebuild_interest_cache()` first, because another test's thread may have cached the callsite as unwanted.
- Cargo never deletes old builds. `python scripts/prune-target.py` reads the `.fingerprint` graph and keeps the newest build of each target, anything built in the last day, and every unit those were compiled from. It reports the rest, and `--delete` removes it from `deps`, `examples`, `build` and `.fingerprint`. Builds of workspace targets or features that no longer exist always go. `--days 0` keeps only the newest builds, so a command whose targets a later command rebuilt compiles its copies again. It refuses while cargo or rustc runs. Run it when `C:` runs low or `target/release` passes about 10 GB.

## Work Guidance

- Current phase status, latest test counts, and the next action live in `docs/plans/10-native-platform-overhaul.md`. Read it first when resuming work.
- Use Cargo workspace conventions from `Cargo.toml`; prefer package-scoped checks while iterating and workspace checks before broad closeout.
- Keep Rust crate ownership clear: public API and shared runtime behavior belong in `crates/director-core`, the serializable scene document in `crates/director-scene`, document frame rendering in `crates/director-render`, the host session (open, patch, inspect, render, export) in `crates/director-host`, command-line concerns in `crates/director-cli`, schema reflection in `crates/director-schema`, and Lottie-specific logic in the `lottie-*` crates.
- `crates/director-html` owns HTML/CSS/HyperFrames compilation, GSAP translation to native tracks, portable native authoring projects and explicit compile-time browser extraction. Native rendering continues through `director-core`; Hyper CN is part of its compatibility corpus.
- Keep core contracts in `docs/architecture/core-contracts.md` green when changing engine spines.
- Rhai examples are executable reference material. Update examples and tests when scripting-facing behavior changes.
- Generated or downloaded assets should not become durable source unless the relevant AGENTS.md explicitly says they are owned there.
- For Python scripts, keep dependencies executable through existing tooling and install packages only with `uv` when installation is necessary.
- Product direction: high-quality native video production for agents and developers, covering short-form and long-form (Shorts, Reels, TikTok, YouTube). Keep preview, editing, inspection and export consistent across supported authoring inputs. Web research as an input belongs at orchestration layers outside the render core.
- `vendor/` contains third-party source selected by local Cargo patches. Do not modify vendor sources without updating its `AGENTS.md`; exclude vendored packages from workspace membership.
- `vendor/skia-safe/` is skia-safe 0.153.3 with three edits keeping the Vulkan `get_proc` thread-local resolver alive for lazy readback/flush resolution. On a dependency bump, reapply or verify removal of the edits documented in `vendor/AGENTS.md`.

## Verification

- Rust workspace: `cargo check --workspace --all-targets --release`
- Rust tests: `cargo test --workspace --release` (CI runs this on push to `main` and on pull requests)
- Rust lint: `cargo clippy --workspace --all-targets --release -- -D warnings`
- Visual regression: `cargo test -p director-core --release --test integration visual_suite::`

## Child DOX Index

- `.agent/AGENTS.md` - Local agent skills and reference material for Director-specific workflows.
- `.github/AGENTS.md` - GitHub workflows and issue templates.
- `crates/AGENTS.md` - Rust workspace crates and package-level ownership.
- `docs/AGENTS.md` - Architecture, core contracts, active plans, contributing, archive.
- `examples/AGENTS.md` - Rhai example scripts, rendered media references, and script validation helpers.
- `vendor/AGENTS.md` - Vendored third-party crates with durable patches.
