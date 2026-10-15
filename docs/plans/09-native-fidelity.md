# Plan 09 — Native fidelity

> **Status:** In progress. Slice 1 is implemented.
> **Product direction:** Native paint and motion that cover the Hyper CN cases the last smoke audit refused for CSS, then the cases that still need a translator.

## Scope

Slice 1, implemented in `director-html`:

- Repeating linear, radial, and conic gradients.
- Per-side dashed and dotted borders.
- Perspective, 3D transforms, and backface visibility, without a shared depth sort.
- SVG `filter` and `mix-blend-mode` from an external stylesheet, plus inline filter primitives usvg already parses.

Later slices of this plan:

- Re-run the Hyper CN registry smoke and record the new render and refusal counts.
- Native translators for canvas, WebGL, and shader blocks.
- Continuous editable GSAP and JS timelines.
- Captured video and audio.
- Gradient color hints, shared 3D occlusion, and the remaining external SVG presentation properties.

Static-subtree paint reuse is GitHub issue #2. Agent production workflows are GitHub issue #3. Frontend and editor design, Remotion source compatibility, and a separate `director-sdk` crate stay outside this plan.

## Implementation contracts

- Repeating gradients compile to the same linear, radial, or conic shader as the non-repeating form. Stops expand across the unit interval by the first-to-last period, then clamp. A period under `1e-4`, or fewer than two surviving stops, paints the last color. Expansion stops at 240 cycles and 720 stops. Equal positions with different colors are nudged by `1e-4` so the hard edge survives. Color hints and vendor-prefixed gradients still fail.
- A uniform dashed or dotted border, same line, width, and color on every side, strokes one inset rounded rect. Any other mix clips to the border ring, fills solid sides with the existing center-meeting wedges, and strokes dashed or dotted sides along their midlines. Dashes are `3w` on and `3w` off. Dots are a zero-length dash with a `2w` gap and a round cap. Gaps show the element background.
- A transform list that contains a projective function resolves lengths to pixels, wraps angles into `[0, 360)`, and uses the lightningcss matrix. A 2D result stays on the affine canvas matrix. A 3D result is the column-vector matrix in row-major order, with translation in the fourth column. Non-projective lists keep the existing 2D path.
- `perspective` is not a self-transform. It wraps descendant drawing as `translate(origin) * perspective(distance) * translate(-origin)`. The element's own text, image, and SVG are drawn before that wrap. The default origin is `50% 50%`. Distance must be positive absolute px. `translateZ` is absolute px. A three-value `transform-origin` still fails.
- `transform-style: flat` and `preserve-3d` parse and do not depth-sort. `backface-visibility: hidden` skips the element when its own matrix `m33` is negative. The cleared frame is opaque black, so a hidden backface reads as `[0, 0, 0, 255]`.
- CSS animation identity and interpolation cover `rotateX`, `rotateY`, `rotateZ`, `translateZ`, `translate3d`, and `perspective`. The identity perspective distance is `1e6` px because `perspective(0)` cannot build a matrix. Animated lengths on those functions must already be px.
- SVG serialization copies descendant markup and, when the document stylesheet sets `filter` or `mix-blend-mode`, appends that declaration to the element's `style` attribute so usvg sees it. Other external SVG presentation properties still fail. `text`, `foreignObject`, SMIL, scripts, and nested images stay rejected. An internal `<style>` inside the SVG is left in the markup and is not added to the document CSS.
- Captured style edits still accept only the previous paint list. Perspective, transforms, and border-style edits on a capture fail. Authored HTML recompiles them.

## Slice 1 acceptance

Pixel checks in `crates/director-html/tests/compatibility.rs`:

1. Repeating conic wedges, a repeating linear period, and a repeating radial period land on the expected color before the box ends.
2. A 64px border box with a dashed top, solid right, dotted bottom, and solid left keeps each side's color. The top gap shows the white background. The dotted bottom uses CSS `green` (`#008000`).
3. `perspective: 80px` plus `rotateY(60deg)` differs from `perspective: none`, and the card center stays red. `rotateY(180deg)` with `backface-visibility: hidden` clears the center. An animated `rotateY` from `0deg` to `70deg` under a parent perspective is red at the left edge at t=0 and black at t=1.
4. An external `svg .box { filter: blur(4px) }` spills red outside the shape and leaves the far corner black. `mix-blend-mode: multiply` of red over blue goes dark. An inline `feGaussianBlur` with an expanded filter region spills red outside the rect.

The last registry smoke remains 205/239 original and 205/239 portrait at 2 fps. Those counts stay until the audit is run again. Smoke is still not visual parity.

## Verification

```powershell
cargo test -p director-html
cargo clippy -p director-html --all-targets --no-deps -- -D warnings
```

Package media tests need FFmpeg and ffprobe. Do not treat a green `director-html` run as a workspace-green run. Existing `director-core` warnings stay outside this plan.

## Next action

Re-run the Hyper CN registry smoke, record which of the previous 34 refusals now render, and keep the WebGL and shader refusals as the following slice.
