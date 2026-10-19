# Plan 09: Native fidelity

> **Status:** Superseded by [plan 10](10-native-platform-overhaul.md). Every slice this plan named is implemented. Its close-out re-runs and the measurement table moved to plan 10.
> **Product direction:** Native paint and motion that cover the Hyper CN cases the last smoke audit refused, then the cases that still need a translator.

Feature contracts live in [`crates/director-html/AGENTS.md`](../../crates/director-html/AGENTS.md). User-facing support lives in [`crates/director-html/README.md`](../../crates/director-html/README.md).

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

The Known refusals list was the open fidelity backlog when the plan was superseded. Plan 10 decides which refusals return after the typed style path lands.
