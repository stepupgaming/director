# Purpose

- Own **active** engineering plans for Director, especially the core-hardening program.

## Ownership

- `00-core-hardening-index.md` — master sequence, **verified truth snapshot**, non-goals, definition of done. Truth pass is complete; do not re-open product scope from `docs/archive/` without updating 00.
- `01`–`06` — time, identity, pipeline, animation eval, scene model, determinism/media. **Done.**
- `07-stdlib-and-recipes.md` — L1 motion stdlib + L2 component contract + L3 recipes on the current stack (no SDK crate).
- `08-native-projects-and-runtime.md` — portable HTML/capture projects, validated CLI edits, common input loading and native backend execution. Frontend/editor design and Remotion compatibility are outside its scope. **Done.**
- `09-native-fidelity.md` — native paint fidelity. Slice 1, capture forwarding, Paper Shader translation, captured muted video and audio, gradient color hints, preserve-3d depth sort, and the listed external SVG presentation properties are implemented. The 2026-10-17 release tests passed: gradient_hints 7, depth_sort 4, svg_presentation 8, compatibility 23. No new registry smoke. Image backgrounds, text-decoration lines, and luminance masks passed paint_gaps 7 on 2026-10-17. CSS nesting passed css_nesting 6. authoring_slices 2 passed. Homogeneous calc/min/max/clamp lengths resolve. Mixed px and percent still fail. grid-template auto-fit, auto-fill, and named lines resolve. A later 2026-10-17 release run passed lib 96, paint_gaps 18, and depth_sort 7. compatibility stayed 23. CSS object-position passed object_position_css 6. `background-clip: content-box` clips the background to the content edge. `padding-box` clips to the padding edge. `border-box` still paints the border box. space and round repeat paint explicit tiles. repeat, no-repeat, repeat-x, and repeat-y stay on the image shader. A file background URL resolves through CompileOptions.base_dir. Capture still refuses image-url backgrounds. The capture extractor copies preserve-3d, solid text-decoration lines, and mask-mode luminance. Static-subtree reuse passed static_reuse 7 and pipeline_equivalence 8 on 2026-10-17. Agent production workflows are issue #3.

## Local Contracts

- Plans are the implementation contract until marked done or superseded.
- Status values: `Spec` | `In progress` | `Done` | `Superseded`.
- When a plan completes, update the index checkboxes/definition of done and `architecture/core-contracts.md` gaps column.
- Deferred product plans live in `../archive/`, not here.

## Work Guidance

- Implement in index order unless a plan explicitly allows parallel work.
- Prefer package-scoped tests while iterating; workspace checks before closeout.
- Composition CLI + preview already landed; recipe/stdlib work is plan 07 (not archive C / director-sdk).

## Verification

- Per-plan acceptance tests plus root workspace commands in `00-core-hardening-index.md`.

## Child DOX Index

- No child AGENTS.md files required.
