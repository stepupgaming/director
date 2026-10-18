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
- Director's product goal is to replace HyperFrames, FFrames and Remotion with a superior native video platform. Assess authoring, motion and media quality, preview/editing, agent tooling, deployment and measured rendering efficiency together; compiler throughput alone does not establish product superiority.
- Current product work prioritizes native projects, runtime, CLI and agent tooling. Frontend/editor design and Remotion compatibility are outside this phase; competing with Remotion does not require accepting its source format.
- Build Director with `cargo --release`. Do not populate `target/debug`.
- Core-hardening plans 01–06, the implemented scope of plan 07, and plan 08 are Done. Plan 09 owns native paint and media fidelity. Slice 1, capture forwarding, Paper Shader translation, and captured muted video and separate audio are implemented. The carried-forward Hyper CN smoke is 239/239. Sampled captures stay sample holds. Gradient color hints, preserve-3d depth sort, and the listed external SVG presentation properties are implemented. The 2026-10-17 release `cargo test -p director-html --release` passed lib 98, paint_gaps 22, gradient_hints 8, depth_sort 7, depth_nested 1, svg_presentation 11, css_nesting 12, css_media 6, object_position_css 9, and grid_lines 1. compatibility was not re-run and stays 23. No new registry smoke. authoring_slices 2 was not re-run. Nested `@media` and `@supports` evaluate against the composition size, including orientation and aspect-ratio. Top-level `@media` still fails. A gradient stop `calc(<percent> + <px>)` uses the gradient line length. Other mixed px and percent still fail. Named grid lines place a column, including a length glued to `[name]`. `subgrid` still fails. Text decoration accepts solid, double, dotted, dashed, and wavy. `text-decoration-color` paints that color. `currentcolor` stays the text color. `from-font` uses the font underline thickness and falls back to auto when that metric is missing or not positive. One `url()` may share a background with gradients. CSS order paints the first layer on top. A second url still fails. `text-transform` accepts `none`, `uppercase`, `lowercase`, and `capitalize`. `full-width` and `full-size-kana` still fail. `text-shadow` paints one shadow. `none` paints none. A second shadow fails. A non-zero spread fails. `overflow-wrap` and `word-wrap` accept `normal`, `break-word`, and `anywhere`. `word-break` accepts `normal`, `break-word`, and `break-all`. `keep-all` still fails. `break-all` also splits a token that overflows the space left on the line. A token that does not fit wraps for the other break values and stays one line for `normal`. `background-origin: content-box` insets by the border plus padding. `background-clip: content-box` clips the background to the content edge. `padding-box` clips to the padding edge. `border-box` still paints the border box. space and round repeat paint explicit tiles. `director-engine sheet`, `audio`, `check`, `snapshot`, and `onion` passed the 2026-10-17 `agent_` filter, 15 tests. `render --scale` in `(0, 1]` rasterizes fewer pixels and keeps the layout size. `--draft-encode` sets software x264 to ultrafast and CRF 30 and does not change the raster size. Hardware qp and the alpha VP9 path stay put. The default stays slow and CRF 18. repeat, no-repeat, repeat-x, and repeat-y stay on the image shader. An absolute transform-origin z pivots the local matrix and changes preserve-3d order. A percent z still fails. perspective-origin z still fails. A file background URL resolves through CompileOptions.base_dir. Capture still refuses image-url backgrounds. The capture extractor copies transform-style when the computed value is preserve-3d. That forwarding is a node test, not a Chrome re-smoke. Static-subtree paint reuse is implemented. The 2026-10-17 release tests passed: static_reuse 7, pipeline_equivalence 8. Agent production workflows are issue #3. Broader recipes and a separate director-sdk crate remain later work.

## Work Guidance

- Use Cargo workspace conventions from `Cargo.toml`; prefer package-scoped checks while iterating and workspace checks before broad closeout.
- Keep Rust crate ownership clear: public API and shared runtime behavior belong in `crates/director-core`, command-line concerns in `crates/director-cli`, schema reflection in `crates/director-schema`, and Lottie-specific logic in the `lottie-*` crates.
- `crates/director-html` owns HTML/CSS/HyperFrames compilation, portable native authoring projects and explicit compile-time browser extraction. Native rendering continues through `director-core`; Hyper CN is part of its compatibility corpus.
- Rhai examples are executable reference material. Update examples and tests when scripting-facing behavior changes.
- Generated or downloaded assets should not become durable source unless the relevant AGENTS.md explicitly says they are owned there.
- For Python scripts, keep dependencies executable through existing tooling and install packages only with `uv` when installation is necessary.
- **Core hardening (01–06) is Done.** Composition CLI, preview and plan 07's motion/component/recipe scope are implemented. Plan 08 is Done. Plan 09 owns native paint and media fidelity. Slice 1, capture forwarding, Paper Shader translation, and captured muted video and separate audio are implemented. The carried-forward smoke is 239/239. Sampled captures stay sample holds. Gradient color hints, preserve-3d depth sort, and the listed external SVG presentation properties are implemented. The 2026-10-17 release `cargo test -p director-html --release` passed lib 98, paint_gaps 22, gradient_hints 8, depth_sort 7, depth_nested 1, svg_presentation 11, css_nesting 12, css_media 6, object_position_css 9, and grid_lines 1. compatibility was not re-run and stays 23. No new registry smoke. authoring_slices 2 was not re-run. Nested `@media` and `@supports` evaluate against the composition size, including orientation and aspect-ratio. Top-level `@media` still fails. A gradient stop `calc(<percent> + <px>)` uses the gradient line length. Other mixed px and percent still fail. Named grid lines place a column, including a length glued to `[name]`. `subgrid` still fails. Text decoration accepts solid, double, dotted, dashed, and wavy. `text-decoration-color` paints that color. `currentcolor` stays the text color. `from-font` uses the font underline thickness and falls back to auto when that metric is missing or not positive. One `url()` may share a background with gradients. CSS order paints the first layer on top. A second url still fails. `text-transform` accepts `none`, `uppercase`, `lowercase`, and `capitalize`. `full-width` and `full-size-kana` still fail. `text-shadow` paints one shadow. `none` paints none. A second shadow fails. A non-zero spread fails. `overflow-wrap` and `word-wrap` accept `normal`, `break-word`, and `anywhere`. `word-break` accepts `normal`, `break-word`, and `break-all`. `keep-all` still fails. `break-all` also splits a token that overflows the space left on the line. A token that does not fit wraps for the other break values and stays one line for `normal`. `background-origin: content-box` insets by the border plus padding. `background-clip: content-box` clips the background to the content edge. `padding-box` clips to the padding edge. `border-box` still paints the border box. space and round repeat paint explicit tiles. `director-engine sheet`, `audio`, `check`, `snapshot`, and `onion` passed the 2026-10-17 `agent_` filter, 15 tests. `render --scale` in `(0, 1]` rasterizes fewer pixels and keeps the layout size. `--draft-encode` sets software x264 to ultrafast and CRF 30 and does not change the raster size. Hardware qp and the alpha VP9 path stay put. The default stays slow and CRF 18. repeat, no-repeat, repeat-x, and repeat-y stay on the image shader. An absolute transform-origin z pivots the local matrix and changes preserve-3d order. A percent z still fails. perspective-origin z still fails. A file background URL resolves through CompileOptions.base_dir. Capture still refuses image-url backgrounds. The capture extractor copies transform-style when the computed value is preserve-3d. That forwarding is a node test, not a Chrome re-smoke. Static-subtree paint reuse is implemented. The 2026-10-17 release tests passed: static_reuse 7, pipeline_equivalence 8. Issue #3 tracks agent production workflows. Broader recipes and a separate director-sdk crate remain later work. Keep core contracts green when changing engine spines. Use `cargo --release` for Director builds.
- Product direction: high-quality native video production for agents and developers, covering short-form and long-form (Shorts, Reels, TikTok, YouTube). Keep preview, editing, inspection and export consistent across supported authoring inputs. Web research as an input belongs at orchestration layers, not in the render core.
- `vendor/` contains third-party source selected by local Cargo patches. Do not modify vendor sources without updating its `AGENTS.md`; exclude vendored packages from workspace membership.
- `vendor/skia-safe/` is skia-safe 0.90.0 with two edits keeping the Vulkan `get_proc` thread-local resolver alive for lazy readback/flush resolution. On a dependency bump, reapply or verify removal of the edits documented in `vendor/AGENTS.md`.

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
