# Plan 09: Native fidelity

> **Status:** In progress. Every slice this plan named is implemented. Closing it needs the re-runs listed under Next action.
> **Product direction:** Native paint and motion that cover the Hyper CN cases the last smoke audit refused, then the cases that still need a translator.

This file is the current-state and handoff document for the active phase. Feature contracts live in [`crates/director-html/AGENTS.md`](../../crates/director-html/AGENTS.md). User-facing support lives in [`crates/director-html/README.md`](../../crates/director-html/README.md).

## Implemented

- Slice 1: repeating linear, radial, and conic gradients; per-side dashed and dotted borders; perspective, 3D transforms, and backface visibility; SVG `filter` and `mix-blend-mode` from an external stylesheet.
- Capture forwarding of perspective, backface, SVG filters and blends, preserve-3d, decoration lines, luminance masks, fixed and sticky position, and the text properties listed in the crate contract.
- Paper Shader translation from captured GLSL ES 300 fragments to SKSL.
- Captured muted video, including `loop`, and separate audio.
- Gradient color hints, preserve-3d depth sort with per-pixel occlusion for crossing planes, and absolute `transform-origin` z.
- External SVG presentation properties, markers, masks, SVG text, authored `foreignObject`, and authored SMIL.
- CSS nesting with nested `@media` and `@supports`, containing-block mixed `calc()`, named grid lines and `subgrid`, fixed and sticky position.
- Image backgrounds with per-layer lists, origin, clip, `space` and `round`; text decoration styles and color; `text-transform`, one `text-shadow`, `overflow-wrap` and `word-break`; `object-position`; luminance masks.
- Static-subtree paint reuse on full render passes (issue #2).
- Agent tooling: `sheet`, `audio`, `check --safe-zone`, `snapshot`, `onion`, `render --scale`, `--draft-encode`, `inspect --sample`, `project repair`, `batch --json`, and preview audio byte ranges.

Sampled captures stay sample holds. A continuous editable GSAP or JS timeline is outside this plan and outside the compiler.

## Hyper CN smoke coverage

- Carried-forward coverage is 239/239 on the original and portrait profiles. It combines the 2026-10-15 full run (207), the six CSS blocks re-smoked after capture forwarding, and the 26 Paper Shader blocks (reports at `F:\shader-smoke-original` and `F:\shader-smoke-portrait`).
- The registry has not been re-smoked since. Smoke counts compilation and one rendered still per block. Pixel parity is a separate measurement and exists only for `examples/html/native-motion.html` and `blur-in`.
- Do not start a registry re-smoke or launch Chrome without an explicit yes from the user.

## Latest measurements

One row per check, holding the most recent passing count. Replace the row when a check is re-run. All runs used `--release`.

| Check | Count | Date | Command |
| --- | --- | --- | --- |
| director-html lib | 101 | 2026-10-17 | `cargo test -p director-html --release` |
| paint_gaps, background_layers | 22, 5 | 2026-10-18 | release run, exact command not recorded |
| object_position_css, gradient_hints, project, shader | 9, 8, 7, 10 | 2026-10-17 | `cargo test -p director-html --release` |
| mixed_calc, gradient_calc | 12, 3 | 2026-10-18 | `cargo test -p director-html --release --test mixed_calc --test gradient_calc` |
| subgrid filter | lib 8, grid_lines 1 | 2026-10-18 | `cargo test -p director-html --release --lib --test grid_lines subgrid` |
| fixed_position, css_media, css_nesting | 5, 6, 12 | 2026-10-18 | `cargo test -p director-html --release --test fixed_position --test css_media --test css_nesting` |
| svg_presentation, svg_markers | 12, 12 | 2026-10-18 | `cargo test -p director-html --release --test svg_presentation --test svg_markers` |
| depth_sort, depth_nested | 8, 1 | 2026-10-18 | `cargo test -p director-html --release --test depth_sort --test depth_nested` |
| capture extractor | 6 | 2026-10-17 | `node --test crates/director-html/tools/tools.test.mjs` |
| static_reuse, pipeline_equivalence | 7, 8 | 2026-10-17 | `cargo test -p director-core --release --test static_reuse --test pipeline_equivalence` |
| agent commands | 15 | 2026-10-17 | `cargo test -p director-cli --release --bin director-engine agent_` |
| inspect_samples, repair_batch | 1, 5 | 2026-10-17 | release `director-cli` tests, exact command not recorded |
| recipe scripts | 9 | 2026-10-17 | release `examples` filter `example_recipes_` |

Stale checks, last green before later changes landed:

- `compatibility` (23) and `authoring_slices` (2) have not been re-run since 2026-10-17.
- The director-html media tests have not been re-run since muted `video loop` support.
- `preview::tests` have not been re-run since `/api/audio` byte ranges.
- The full `cargo test -p director-html --release` has not been re-run since 2026-10-17, so tests added on 2026-10-18 were run only by their filtered commands.

Workspace state: `cargo test --workspace` fails on four unmodified Rhai example scripts, and workspace clippy reports existing `director-core` warnings. Package lint and workspace all-target checks pass.

## Known refusals

These still produce a diagnostic and are the candidates for the next slices:

- External stylesheet `animation` and its longhands on SVG descendants.
- Top-level `@media`.
- Captured SVG `foreignObject`, SMIL, scripts, and nested images.
- Image-url backgrounds in a capture.
- Percent `transform-origin` z and `perspective-origin` z. Separately, the preserve-3d depth key silently ignores parent perspective.
- A second `text-shadow` and a non-zero shadow spread.
- `word-break: keep-all`, `overflow-wrap: break-spaces`, `text-transform: full-width | full-size-kana`.
- `clip-path: url()` and `inset(... round ...)`; `wavy` borders; fixed or local background attachment; a non-gradient HTML mask image.
- `steps()` with `jump-none` or `jump-both`.
- Audio playback rate other than 1, unmuted video, and iframe.
- Canvas without a captured Paper Shader fragment, and other WebGL or Three.js runtimes.

## Next action

1. Close the plan: run the full `cargo test -p director-html --release` with FFmpeg available, plus `cargo clippy -p director-html --all-targets --no-deps -- -D warnings`, update the measurement table, mark this plan Done, and close issue #2.
2. Re-run `preview::tests` for issue #3 and record the result.
3. Pick the next refusal. The last session suggested external stylesheet `animation` on SVG.
4. With the user's explicit yes, re-smoke the Hyper CN registry and measure pixel parity beyond `blur-in`.

Issue #3 tracks agent production workflows. Frontend and editor design, Remotion source compatibility, and a separate `director-sdk` crate stay outside this plan.

## Verification

```powershell
cargo test -p director-html --release
cargo clippy -p director-html --all-targets --no-deps -- -D warnings
node --test crates/director-html/tools/tools.test.mjs
```

Media tests need FFmpeg and ffprobe. A green `director-html` run is not a workspace-green run.
