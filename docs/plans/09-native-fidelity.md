# Plan 09 — Native fidelity

> **Status:** In progress. Slice 1, capture forwarding, Paper Shader translation, captured muted video and separate audio, gradient color hints, preserve-3d depth sort, and the listed external SVG presentation properties are implemented.
> **Product direction:** Native paint and motion that cover the Hyper CN cases the last smoke audit refused for CSS, then the cases that still need a translator.

## Scope

Slice 1, implemented in `director-html`:

- Repeating linear, radial, and conic gradients.
- Per-side dashed and dotted borders.
- Perspective, 3D transforms, and backface visibility, without a shared depth sort.
- SVG `filter` and `mix-blend-mode` from an external stylesheet, plus inline filter primitives usvg already parses.

Recorded on 2026-10-15, after slice 1, at 2 fps with the release `director-engine`:

- Original and portrait both rendered 207 of 239. Those report directories under `target/` were later removed.
- The same 32 names fail in both profiles, and every failure is at capture extraction.
- Twenty-six are canvas or WebGL: the `shader-*` blocks and eight `trans-*` dissolves or wipes.
- Three SVG CSS filters or blends: `animated-bar-chart`, `animated-line-chart`, `data-flow-pipes`.
- Three perspective blocks: `perspective-marquee`, `rolodex-flip`, `x-followers-overview`.
- The previous dashed-border and repeating-conic refusals now render. `claude-code` and `live-code-compilation` are in the rendered set.

Capture forwarding, after that smoke:

- `tools/browser.mjs` stores a non-none computed `perspective` and its origin, stores `backface-visibility: hidden`, and writes SVG-descendant `filter` and `mix-blend-mode` into the captured markup.
- Re-smoke of `perspective-marquee`, `rolodex-flip`, `x-followers-overview`, `animated-bar-chart`, `animated-line-chart`, and `data-flow-pipes` rendered 6/6 original and 6/6 portrait. Those report directories under `target/` were later removed.
- The other 207 blocks were not re-run. The union at that point was 213/239. The remaining refusals were the 26 canvas and WebGL blocks.

Paper Shader translation, recorded on 2026-10-15, at 2 fps with the release `director-engine`:

- Original and portrait both rendered 26/26 of those blocks. Reports are `F:\shader-smoke-original` and `F:\shader-smoke-portrait`.
- The earlier 213 blocks were not re-run. The union is 239/239.
- Each report still is one frame at duration/2. Smoke counts compilation and render. Pixel parity is a separate measurement.
- A canvas with no captured fragment stays refused. The translator does not insert a screenshot.
- The capture stores the Paper Shader fragment and the uniforms uploaded for that sample. GLSL ES 300 becomes SKSL when the capture compiles, so a translator fix does not require a new capture.
- Object-like `#define` expands. A function-like `#define` is an error. `uniform bool` becomes `uniform int`. Precision qualifiers are stripped.
- `fwidth`, `dFdx`, and `dFdy` become a constant about `1.5 / max(resolution)`. `textureGrad` drops the explicit derivatives. `textureSize` uses the captured image's pixel size, and a function parameter of sampler type is replaced by the captured image name.
- `round` and `float2x2` transpose use helpers. Integer `%` is toward-zero division, and a zero divisor returns 0. Integer `clamp` uses `paperClampInt`. A rising `for` loop whose bound is that clamped int runs to the clamp's literal max and breaks once the index passes the live bound.
- `samplerCube`, `sampler3D`, integer samplers, `texelFetch`, image load/store, and barriers stay refused.
- `u_time` is the seek time in seconds times the authored mount speed.

Captured muted video and separate audio, measured by `cargo test -p director-html --release`:

- A packed ffv1 clip, 0.5s red then 0.5s green, with `data-media-start` 0.2 is red at t=0 and green at t=0.6. The reverse seek matches t=0. The two samples use different `object-fit` values.
- A packed wav is one mixer track across two samples. `data-media-start` 0.25 drains the same samples as one `load_audio_bytes` call. `loop` and volume 0.5 stick. `data-hidden` stores volume 0.
- Unmuted video, video loop, audio playback rate 1.5, iframe, a duplicate audio id, and a src that changes across samples fail.
- DOM paint stays on the held sample. The video decoder seeks to the composition time. This pass is not a Hyper CN registry re-smoke and not browser pixel parity.

Muted `video loop` now wraps that seek into the probed duration. The capture extractor records `looped`. The media tests have not been re-run since that change. Audio playback-rate other than 1 still fails.

Sampled captures stay sample holds. A continuous editable GSAP or JS timeline is outside this plan.

Gradient color hints, preserve-3d depth sort, and the listed external SVG presentation properties, measured on 2026-10-17 by `cargo test -p director-html --release --test gradient_hints --test depth_sort --test svg_presentation --test compatibility`:

- `gradient_hints`: 7 passed. A 25% hint pulls the linear midpoint toward blue. A 50% hint matches the unhinted ramp. Radial and conic hints change the sample. A repeating linear hint still paints a later period. Vendor gradients and an orphan hint fail.
- `depth_sort`: 7 passed. Under `preserve-3d`, `translateZ(40px)` paints over `translateZ(-40px)`. Equal Z uses z-index. A hidden backface stays black. An absolute `transform-origin` z of `30px` on `rotateY(180deg)` paints that card over a `translateZ(40px)` sibling. `0px` leaves the sibling in front. A flat card with origin z `40px` still fills its box. A percent z fails. `perspective-origin` z fails.
- `svg_presentation`: 8 passed. External fill, fill opacity, `currentColor`, stroke, a `translate` with `px` stripped, and `clip-path: url(#cut)` paint. External animation and SVG text still fail.
- `compatibility`: 23 passed, including external SVG fill.

This pass is not a Hyper CN registry re-smoke and not browser pixel parity. The carried-forward smoke stays 239/239.

Static-subtree paint reuse is GitHub issue #2. Agent production workflows are GitHub issue #3. Frontend and editor design, Remotion source compatibility, and a separate `director-sdk` crate stay outside this plan.

## Implementation contracts

- A gradient shader is sized to `background-origin` and drawn with clamp across the border box. The default origin is padding-box. `border-box` sizes the ramp to the border box. Radial and conic centers include the origin rect's top-left. Masks stay on the border box. Measured 2026-10-17: `paint_gaps` 16 and `gradient_hints` 7.
- Repeating gradients compile to the same linear, radial, or conic shader as the non-repeating form. Stops expand across the unit interval by the first-to-last period, then clamp. A period under `1e-4`, or fewer than two surviving stops, paints the last color. Expansion stops at 240 cycles and 720 stops. Equal positions with different colors are nudged by `1e-4` so the hard edge survives. Color hints expand in sRGB before that tiling. A hint must sit strictly between two color stops. A hint at the left stop becomes the next color. A hint at the right stop becomes the previous color. A midpoint of 0.5 is dropped. Any other hint inserts nine sRGB stops on the Gecko midpoint curve. The repeat period stays first-to-last of the resolved stops. Vendor-prefixed gradients fail, including a prefixed linear gradient. An orphan hint fails. A hint that parses and then fails expansion paints nothing.
- A uniform dashed or dotted border, same line, width, and color on every side, strokes one inset rounded rect. Any other mix clips to the border ring, fills solid sides with the existing center-meeting wedges, and strokes dashed or dotted sides along their midlines. Dashes are `3w` on and `3w` off. Dots are a zero-length dash with a `2w` gap and a round cap. Gaps show the element background.
- A transform list that contains a projective function resolves lengths to pixels, wraps angles into `[0, 360)`, and uses the lightningcss matrix. A 2D result stays on the affine canvas matrix. A 3D result is the column-vector matrix in row-major order, with translation in the fourth column. Non-projective lists keep the existing 2D path.
- `perspective` is not a self-transform. It wraps descendant drawing as `translate(origin) * perspective(distance) * translate(-origin)`. The element's own text, image, and SVG are drawn before that wrap. The default origin is `50% 50%`. Distance must be positive absolute px. `translateZ` is absolute px. An absolute `transform-origin` z wraps that matrix. A percent z still fails. `perspective-origin` z still fails.
- `transform-style: preserve-3d` depth-sorts direct children by ascending Z of each child's center after its own local matrix, then by z-index, then by source order. Positive `translateZ` paints later. An affine child has Z 0. `flat` keeps the z-index sort. Any other value fails. That Z does not include the parent perspective matrix. `backface-visibility: hidden` skips the element when its own matrix `m33` is negative. The cleared frame is opaque black, so a hidden backface reads as `[0, 0, 0, 255]`. An absolute `transform-origin` z wraps that matrix. A percent z still fails. `perspective-origin` z still fails. The capture extractor copies `transform-style` when the computed value is `preserve-3d`. Flat is omitted, so a captured flat parent stays flat. That forwarding has a node test and has not been re-smoked in Chrome.
- CSS animation identity and interpolation cover `rotateX`, `rotateY`, `rotateZ`, `translateZ`, `translate3d`, and `perspective`. The identity perspective distance is `1e6` px because `perspective(0)` cannot build a matrix. Animated lengths on those functions must already be px.
- SVG serialization copies descendant markup. When the document stylesheet sets `fill`, `color`, `stroke`, `stroke-width`, `stroke-linecap`, `stroke-linejoin`, `stroke-dasharray`, `stroke-dashoffset`, `opacity`, `transform`, `clip-path`, `filter`, or `mix-blend-mode`, that declaration is appended to the element's `style` attribute. `transform` values have `px` removed before usvg sees them. `clip-path` is copied as the cascaded `url(#id)`. External `animation` and its longhands still fail with the animation diagnostic. `text`, `foreignObject`, SMIL, scripts, and nested images stay rejected. An internal `<style>` inside the SVG is left in the markup and is not added to the document CSS.
- Captured style edits still accept only the previous paint list. Perspective, transforms, and border-style edits on a capture fail. Authored HTML recompiles them.

## Slice 1 acceptance

Pixel checks in `crates/director-html/tests/compatibility.rs`:

1. Repeating conic wedges, a repeating linear period, and a repeating radial period land on the expected color before the box ends.
2. A 64px border box with a dashed top, solid right, dotted bottom, and solid left keeps each side's color. The top gap shows the white background. The dotted bottom uses CSS `green` (`#008000`).
3. `perspective: 80px` plus `rotateY(60deg)` differs from `perspective: none`, and the card center stays red. `rotateY(180deg)` with `backface-visibility: hidden` clears the center. An animated `rotateY` from `0deg` to `70deg` under a parent perspective is red at the left edge at t=0 and black at t=1.
4. An external `svg .box { filter: blur(4px) }` spills red outside the shape and leaves the far corner black. `mix-blend-mode: multiply` of red over blue goes dark. An inline `feGaussianBlur` with an expanded filter region spills red outside the rect.

The carried-forward smoke is 239/239 on each profile: 207 from the full run, the six re-smoked CSS blocks, and the 26 Paper Shader blocks. The 213 blocks from before the shader pass were not re-run. Pixel parity is a separate measurement.

## Verification

```powershell
cargo test -p director-html
cargo clippy -p director-html --all-targets --no-deps -- -D warnings
```

Package media tests need FFmpeg and ffprobe. Do not treat a green `director-html` run as a workspace-green run. Existing `director-core` warnings stay outside this plan.

## Next action

The slices named in this plan are implemented. Image backgrounds, text-decoration lines, and luminance masks passed `paint_gaps` 7 on 2026-10-17, including `background-origin: content-box` and a dashed underline. CSS nesting passed `css_nesting` 6. `authoring_slices` 2 passed. Homogeneous calc/min/max/clamp lengths resolve. A mix of px and percent still fails. grid-template auto-fit, auto-fill, and named lines resolve. Subgrid and named grid-column placement still fail. The 2026-10-17 release `cargo test -p director-html --release --lib --test paint_gaps` passed lib 96, paint_gaps 18, and depth_sort 7. gradient_hints stayed 7. CSS object-position passed object_position_css 6. Text decoration accepts solid, double, dotted, dashed, and wavy. `text-decoration-color` paints that color. `currentcolor` stays the text color. `from-font` uses the font underline thickness and falls back to auto when that metric is missing or not positive. One `url()` may share a background with gradients. CSS order paints the first layer on top. A second url still fails. `text-transform` accepts `none`, `uppercase`, `lowercase`, and `capitalize`. `full-width` and `full-size-kana` still fail. `background-clip: content-box` clips the background to the content edge. `padding-box` clips to the padding edge. `border-box` still paints the border box. space and round repeat paint explicit tiles. repeat, no-repeat, repeat-x, and repeat-y stay on the image shader. The earlier `--test compatibility` run passed 23. An absolute transform-origin z pivots the local matrix and changes preserve-3d order. A percent z still fails. perspective-origin z still fails. A file background URL resolves through CompileOptions.base_dir. Capture still refuses image-url backgrounds. The capture extractor copies `preserve-3d`, solid text-decoration lines, and `mask-mode: luminance` (`tools.test.mjs` 6, not a Chrome re-smoke). Issue #2, static-subtree reuse, passed `static_reuse` 7 and `pipeline_equivalence` 8 on 2026-10-17. Issue #3's preview ranges are written and the preview tests were not re-run. `inspect --sample` and `batch --json` passed `inspect_samples` 1 and `repair_batch` 5. The release examples filter `example_recipes_` passed 9, including `end_card`, `chapter_card`, and `countdown`. Do not start a registry re-smoke without an explicit yes.
