# Purpose

- Compile HTML/CSS and HyperFrames compositions into Director scenes without browser frame rendering.

## Ownership

- `src/` owns parsing, cascade, style translation, deterministic animation, native element adapters, and captured-scene loading.
- `src/project.rs` owns `director-native-project/1`, editable source and validated element overrides; `src/assets.rs` owns asset recording, deduplicated SHA-256 blobs and temporary media-file leases.
- `tools/` owns optional compile-time browser extraction, Chromium and CPU/GPU parity, real HyperFrames CLI and CPU/GPU export benchmarks, and Hyper CN registry auditing.
- `tests/` owns executable compatibility and pixel regressions.
- `benches/` owns Criterion compile/render workloads.
- `README.md` owns the user-facing feature summary, incompatibilities, commands, and reproducible measured results. This file owns the exact behavior contracts. Current phase status and test counts live in `docs/plans/10-native-platform-overhaul.md`.

## Local Contracts

### Architecture

- Depend on `director-core`; do not introduce HTML behavior into Rhai or the core dependency-graph compiler.
- Use upstream HTML, CSS, selector, layout, typography and SVG libraries. Unsupported syntax or rendering behavior must produce diagnostics; compilation fails when errors remain.
- Native adapters must remain semantically Send under the core scene-node contract. Share captured graphs behind `Arc<Mutex<SceneGraph>>`, never `Rc` or an unprotected shared graph.
- Generated captures, videos, reference checkouts and benchmark reports live under ignored `target/` or user-selected outputs.

### Native projects

- Native projects preserve HTML/capture authoring data and compile it on load. Do not serialize HTML adapters through core project I/O, which owns a different node-kind contract.
- Pack every consumed local stylesheet, font, image, audio and video plus required bundled fonts. Frozen project compilation cannot read original files or substitute host files for missing blobs; hash mismatches fail.
- Apply edits by stable authored element ID and validate the complete candidate before acceptance or atomic save. Captures retain sampled timing and fixed geometry; reject edits requiring uncaptured layout/animation.
- Preserve unique authored capture IDs. Anonymous/duplicate IDs receive noncolliding structural paths, stable while sampled topology is stable; expose those paths in inspection and store them in packed captures.
- Keep extracted media files alive through the Director asset-loader lifetime, even when the project or compiler wrapper is dropped.
- Share immutable source and encoded blobs across edit candidates; validation must not clone entire media payloads merely to change an edit map.

### Capture and extraction

- Browser extraction is explicit and compile-time only. Capture layout, style, SVG and media references, never screenshots as native scene content. Captures declare their sample rate and cannot claim continuous animation equivalence.
- The capture format is `director-html-capture/1` with `deny_unknown_fields`. New fields must be optional with defaults.
- Registry audits must distinguish renderable inputs from diagnosed incompatibilities.
- The extractor forwards a non-none computed `perspective` with its origin, `backface-visibility: hidden`, SVG-descendant `filter` and `mix-blend-mode`, `transform-style: preserve-3d` (flat is omitted, so a captured flat parent stays flat), `text-decoration-line` when every token is `underline`, `overline`, or `line-through`, `mask-mode: luminance`, `position` when it is `fixed` or `sticky` (geometry stays the sampled rect), `font-variant-numeric`, `text-indent`, the `-webkit-text-stroke` properties, `overflow-wrap`, and `word-break`. These forwards have node tests and have not been re-smoked in Chrome.
- The extractor refuses other decoration values, mask modes other than alpha, match-source, and luminance, `background-blend-mode`, image-url backgrounds, generated content, and iframe.
- Captured SVG markup rejects `foreignObject`, SMIL, scripts, and nested images.
- Captured style edits accept only the previous paint list. Perspective, transform, and border-style edits on a capture fail. Authored HTML recompiles them.

### Paper Shader translation

- Do not treat Hyper CN WebGL/canvas content as supported until a native translation exists. A canvas with a captured Paper Shader fragment compiles to SKSL and draws in the element content rect. A canvas with no fragment stays refused, as do other WebGL and Three.js runtimes. The translator never inserts a screenshot.
- The capture stores the fragment and the uniforms uploaded for that sample. GLSL ES 300 becomes SKSL at compile time, so a translator fix does not require a new capture.
- Object-like `#define` expands; a function-like `#define` is an error. `uniform bool` becomes `uniform int`. Precision qualifiers are stripped.
- `fwidth`, `dFdx`, and `dFdy` become a constant of about `1.5 / max(resolution)`. `textureGrad` drops the explicit derivatives. `textureSize` uses the captured image's pixel size, and a sampler-typed function parameter is replaced by the captured image name.
- `round` and `float2x2` transpose use helpers. Integer `%` truncates toward zero and a zero divisor returns 0. Integer `clamp` uses `paperClampInt`. A rising `for` loop bounded by that clamped int runs to the clamp's literal max and breaks once the index passes the live bound.
- `samplerCube`, `sampler3D`, integer samplers, `texelFetch`, image load/store, and barriers stay refused.
- `u_time` is the seek time in seconds times the authored mount speed.

### Captured media

- Captured muted video shares one decoder per element id. Sample rect and `object-fit` may change. The active sample seeks that decoder to the composition time. Muted `video loop` seeks with `media_start + elapsed * playback_rate`, then wraps into `[0, duration)` using the ffprobe duration. The extractor records `looped`.
- Captured audio is one mixer track per id, including an element the visibility window hides. `data-hidden` sets track volume 0. Audio `loop` loops the track. `data-media-start` drains samples the same way as static HTML. Bytes come from the capture asset map.
- Unmuted video, an audio playback rate other than 1, iframe, a duplicate media id in one frame, and a media id whose src or timing changes across samples fail. DOM paint stays on the held sample.

### Cascade and at-rules

- Nested style rules flatten into the same selector list as top-level rules. `&` is replaced with the parent selector. A parent list becomes `:is(...)`.
- Nested `@media` and `@supports` evaluate at compile time against the composition size. `(orientation: landscape)` is true when width is at least height. `(aspect-ratio)`, `(min-aspect-ratio)`, and `(max-aspect-ratio)` compare width/height to the ratio. Top-level `@media` fails with `unsupported CSS at-rule`. Unknown media features fail.
- `@supports` is true for `display` (`block`, `flex`, `grid`, `inline`, `inline-block`, `none`), `background-clip` (`text`, `border-box`, `padding-box`, `content-box`), `text-overflow` (`ellipsis`, `clip`), `object-fit` (`fill`, `cover`, `contain`, `none`, `scale-down`), `position` (`static`, `relative`, `absolute`, `fixed`, `sticky`), and `overflow` / `overflow-x` / `overflow-y` (`visible`, `hidden`, `clip`). Any other declaration fails.

### Layout

- Homogeneous `calc()`, `min()`, `max()`, and `clamp()` lengths become px. Homogeneous percents stay percents (`50%` is fraction `0.5`). `convert_length` passes no percent basis. `length_on` resolves a mixed calc when the caller supplies a finite basis greater than 0.
- Mixed-calc bases: width, min-width, max-width, left, right, margin, padding, text-indent, and a length flex-basis use the containing-block width. Height, min-height, max-height, top, and bottom use the containing-block height. Padding and margin use width on every side. column-gap uses the element's own definite content width; row-gap uses its own definite content height.
- The root containing block is the composition size. A nested box uses its parent's definite content box. An auto block width stretches to that box. An auto height, an auto shrink-to-fit width, and a grid item with width auto stay indefinite. A missing, zero, negative, or non-finite basis fails with `mixed calc() requires a length basis`.
- A gradient color stop written as `calc(<percent> + <px>)` or `calc(<px> + <percent>)` uses the gradient line length; a radial stop uses the ending radius. `calc(50% + 10px + 1em)` fails on a gradient stop. A zero-length gradient line keeps the mixed-calc error. `round`, `rem`, `mod`, `abs`, `sign`, and `hypot` fail.
- `grid-template-columns` and `grid-template-rows` accept an integer `repeat` up to 4096, `repeat(auto-fit, ...)`, `repeat(auto-fill, ...)`, `[line]` names, and `subgrid`. An auto repeat nested inside another repeat fails.
- `subgrid` copies the parent axis when that axis has a known track count, including its line names. A grandchild line name places on that copied line. `subgrid [name]` adds those names when one group is present for every parent line. A missing parent axis, an auto-fit or auto-fill parent, a name list of a different length, and `subgrid` inside `repeat()` fail.
- `grid-column` and `grid-row` accept `auto`, a line number, `span N`, or a line name stored on the parent template. A length glued to `[name]`, the form lightningcss emits, is that track plus the name. A missing name or a bare custom ident fails with `named grid lines`.
- `position: fixed` is out of flow and uses the composition as its containing block. A transformed ancestor does not recapture fixed. `position: sticky` stays in flow, stretches like a static block, and its insets stay auto. There is no scroll offset.
- `line-clamp` and `-webkit-line-clamp` accept `none` or a positive integer and ellipsize at that line count inside the content width. `text-overflow: ellipsis` requires `white-space: nowrap` and overflow-x hidden or clip. Visible nowrap spills.

### Backgrounds, borders, masks, and filters

- Repeating gradients compile to the same linear, radial, or conic shader as the non-repeating form. Stops expand across the unit interval by the first-to-last period, then clamp. A period under `1e-4`, or fewer than two surviving stops, paints the last color. Expansion stops at 240 cycles and 720 stops. Equal positions with different colors are nudged by `1e-4` so the hard edge survives.
- Color hints expand in sRGB before tiling. A hint must sit strictly between two color stops. A hint on the left stop becomes the next color; a hint on the right stop becomes the previous color. A 50% hint is dropped. Any other hint inserts nine sRGB stops on the Gecko midpoint curve. Vendor-prefixed gradients and orphan hints fail. A hint that parses and then fails expansion paints nothing.
- A gradient shader is sized to `background-origin` and clamped across the border box. The default origin is padding-box, so a transparent border shows the end color. Radial and conic centers include the origin rect's top-left. Masks stay on the border box. A length `background-size` tiles a gradient when an axis repeats. `tiled_gradient` records the gradient once at the tile size and repeats that picture in 2D, so a radial dot stays a dot; a `no-repeat` axis decals. `to bottom right` uses the CSS corner angle of the painted rect; a square is 135deg.
- Any number of `url()` or `data:` layers may share `background-image` with gradients. CSS order paints the first layer on top. A file URL resolves through the compile `AssetStore`, so `CompileOptions.base_dir` and frozen project blobs apply. A data URI decodes inline. A missing file fails. `none` layers are dropped before list indexing.
- `background-size`, `background-position`, and `background-repeat` are comma lists aligned with the layers. A shorter list cycles by source index, and the x and y lists cycle independently. Size accepts `auto`, `cover`, `contain`, and lengths. `background-position` takes one or two values per layer; one value sets x and centers y unless it is `top` or `bottom`. Three or four values fail.
- `repeat`, `no-repeat`, `repeat-x`, and `repeat-y` use the image shader. `space` and `round` paint explicit tiles, including mixed with a repeat axis, capped at 4096 tiles.
- `background-origin: content-box` insets by the border plus padding; percentage padding uses the element width. `background-clip` `content-box` clips to the content edge, `padding-box` to the padding edge, and `border-box` paints the border box. Fixed or local attachment fails.
- A uniform dashed or dotted border (same line, width, and color on every side) strokes one inset rounded rect. Mixed side styles clip to the border ring, fill solid sides with center-meeting wedges, and stroke dashed or dotted sides along their midlines. Dashes are `3w` on and `3w` off. Dots are a zero-length dash with a `2w` gap and a round cap. Gaps show the element background.
- Border styles also accept `double`, `groove`, `ridge`, `inset`, and `outset`. `hidden` zeroes the used width. `wavy` fails.
- `mask-mode: luminance` writes Rec.709 luma into alpha and blends with `DstIn`. A non-gradient mask image fails.
- `clip-path` accepts `polygon()`, `circle()`, `ellipse()`, and `inset()` without `round`. `url()` fails.
- `filter` accepts `invert`, `sepia`, and `opacity` after the existing color matrix, clamped to `0..=1`.
- `object-fit` accepts `fill`, `cover`, `contain`, `none`, and `scale-down`; the core `ObjectFit` default stays `Cover`. `object-position` positions image and video content, including a captured video's shared decoder. The default is 50% 50%. Keyword order can swap axes. One keyword plus a length sets the other axis.

### Text

- `text-decoration` paints `underline`, `overline`, and `line-through`. Styles are solid, double, dotted, dashed, and wavy. `text-decoration-color` paints that color; `currentcolor` stays the text color. Thickness is `auto`, a length, or a percent of 1em. `from-font` uses the font underline thickness and falls back to auto when that metric is missing or not positive. `text-underline-offset` shifts only the underline; `auto` and `0` leave it, and a percent is a fraction of 1em.
- `text-transform` accepts `none`, `uppercase`, `lowercase`, and `capitalize`. `capitalize` uppercases the first character of each whitespace-separated word. `full-width` and `full-size-kana` fail.
- `text-shadow` paints one shadow; `none` paints none. A second shadow or a non-zero spread fails.
- `overflow-wrap` and `word-wrap` accept `normal`, `break-word`, and `anywhere`; `break-spaces` fails. `word-break` accepts `normal`, `break-word`, and `break-all`; `keep-all` fails. `anywhere`, `break-word`, and `word-break: break-word` break a token that does not fit. `break-all` also splits a token that overflows the space left on the line. `normal` keeps that token on one line, and left alignment lays the line out past the box. Soft min-content stays the unwrapped line, so `anywhere` does not shrink to one character.
- Text in a box sized to its own text stays on one line. Taffy rounds box edges to whole pixels, so such a box can come out up to a pixel narrower than its line, and Skia also breaks a line laid out at exactly its own width. `layout_paragraph` gives a line `WRAP_SLACK` (1 px) past its box before the next word wraps, lays out a line within a pixel of its box a pixel wider than itself, and the element keeps the paragraph's alignment edge, so centred text stays centred. `append_wrapped` adds up unkerned token widths, which run several pixels past the shaped line at display sizes, so `wrap_width` skips that pre-wrap when the shaped lines already fit. `tests/shrink_wrap.rs` covers each alignment at four sub-pixel offsets, with kerned text up to 72 px.
- `white-space: pre-line` keeps hard breaks and collapses spaces. `pre-wrap` preserves spaces and wraps.
- `<br>` is a hard break under every `white-space` mode. A collapsible space beside it is dropped, and a trailing `<br>` adds no line. An inline child (`span`, `b`, `strong`, `em`, `i`, `small`) of a text block may set inherited and `font-*` properties. It ignores `box-sizing` and zero margins and paddings, which a `*` reset gives every span. Any other property on it fails with `styled inline boxes require extraction`. `tests/rich_text.rs` covers both.
- `font-variant-numeric: tabular-nums` sets `tnum`. `text-indent` inserts a leading placeholder; `hanging` and `each-line` fail. `-webkit-text-stroke` paints one stroke; width `0` skips it. A stroke colour that differs from an opaque fill paints from its own paragraph, ordered by `paint-order`. `paint-order` takes `normal` or `fill`, `stroke`, and `markers` once each in any order; unlisted layers follow in the order fill, stroke, markers. `normal` paints the stroke over the fill, and `stroke fill` paints it under. A repeated or unknown layer fails. Neither property inherits, so set both on the element that holds the text.
- The `font` shorthand compiles. `font-stretch` and `font-variant-caps` accept only `normal`; any other value fails. `tests/font_shorthand.rs` covers both.

### Transforms and 3D

- A transform list that contains a projective function resolves lengths to pixels, wraps angles into `[0, 360)`, and uses the lightningcss matrix. A 2D result stays on the affine canvas matrix. A 3D result is the column-vector matrix in row-major order with translation in the fourth column. Non-projective lists keep the 2D path.
- `perspective` projects descendants only. It wraps descendant drawing as `translate(origin) * perspective(distance) * translate(-origin)`; the element's own text, image, and SVG draw before that wrap. The default origin is `50% 50%`. Distance and `translateZ` must be absolute px. An absolute `transform-origin` z pivots the local matrix. A percent z and `perspective-origin` z fail.
- `transform-style: preserve-3d` depth-sorts by ascending Z of each child's center after its own local matrix, then z-index, then source order. A direct child that also preserves 3d contributes its descendants, and depth sums along that chain. A non-sharing child stays one plane. An affine child has Z 0. The depth key excludes the parent perspective. A plane whose Z changes across its quad, such as crossing `rotateX` cards, is occluded per pixel; constant Z stays on the center sort. `flat` keeps z-index order. Other values fail.
- `backface-visibility: hidden` skips an element whose own matrix `m33` is negative. The cleared frame is opaque black, so a hidden backface reads as `[0, 0, 0, 255]`.
- CSS animation identity and interpolation cover `rotateX`, `rotateY`, `rotateZ`, `translateZ`, `translate3d`, and `perspective`. The identity perspective distance is `1e6` px. Animated lengths on those functions must already be px.

### SVG

- An external stylesheet may set `fill`, `fill-opacity`, `fill-rule`, `color`, `stroke`, `stroke-width`, `stroke-linecap`, `stroke-linejoin`, `stroke-dasharray`, `stroke-dashoffset`, `stroke-opacity`, `stroke-miterlimit`, `opacity`, `transform`, `clip-path`, `clip-rule`, `filter`, `mix-blend-mode`, `marker-start`, `marker-mid`, `marker-end`, `mask`, `font-family`, `font-size`, `font-weight`, and `font-style` on SVG descendants. Each declaration is appended to the element's `style` attribute. `transform` values drop `px` because usvg rejects them. `clip-path` and the marker longhands are copied as printed. The `mask` shorthand is stored as `mask-image` and written back as `mask`; HTML `mask-image` stays on the HTML mask path. External `animation` and its longhands fail.
- An internal `<style>` inside the SVG stays in the markup and is not added to the document CSS.
- SVG `<text>` is shaped with the bundled Roboto, Inter, and Poppins faces plus document `@font-face` faces. The default family is Roboto and the default size is 16. The font database loads on the first SVG. Rhai `VectorNode::new` still parses with an empty font database.
- An authored `foreignObject` lays out its HTML children in that viewport and paints them after the SVG raster. `x`, `y`, `width`, and `height` accept a number, `px`, or a percent of the SVG viewport, and user units map through `viewBox`. The box clips overflow. A non-positive width or height paints nothing. Loose text fails with `foreignObject text requires an HTML element`. A transform on the `foreignObject` or an SVG ancestor fails with `transformed foreignObject requires extraction`. A nested `foreignObject`, or one inside a nested `svg`, `symbol`, `defs`, `marker`, `pattern`, `clipPath`, or `mask`, requires extraction.
- Authored SMIL: `animate`, `animateTransform`, `animateMotion`, and `set` are sampled at the element's local time, patched into the markup, stripped, and reparsed. One animation per attribute. `animate` and `animateTransform` interpolate numbers or colors linearly. `animateMotion` follows a paced polyline and appends a translate. `set` holds `to`. `fill` is `freeze` or `remove`. The layout box stays the time-0 SVG size. Spline timing, `keySplines`, `keyPoints`, `mpath`, begin or end lists, `additive` other than replace, `accumulate` other than none, `min`, `max`, motion `rotate` auto, path curves, and unknown SMIL attributes require extraction. The animated SVG marks the element as time-varying for the static cache.
- Scripts and nested images fail with `SVG scripts and nested images require native translation`.

### CSS animation

- Authored `@keyframes` become native animation tracks whose duration, easing, and values stay editable on HTML recompile. Scroll-driven keyframes fail. GSAP and JS timelines never become tracks.
- `HtmlElement::local_transform` gives the core motion policy the animated transform about its origin, so keyframed movement counts toward motion scoring. A 3D transform contributes its planar part. `tests/motion_policy.rs` covers a static box, a moving box, and one over the speed limit.
- `animation-direction` accepts `normal`, `reverse`, `alternate`, and `alternate-reverse`. `animation-fill-mode` accepts `none`, `forwards`, `backwards`, and `both`. `steps()` with `jump-none` or `jump-both` fails.

## Work Guidance

- Preserve arbitrary-time seek determinism. Reuse immutable assets and resolve static styles before rendering.
- Keep measured parity and performance claims tied to reproducible commands and artifacts.
- CPU/GPU comparisons must synchronize GPU work through readback, verify the actual backend/encoder and probe every MP4's frame count and audio/video/container duration. GPU rendering and NVENC encoding are separate controls.
- Exercise real Hyper CN original and portrait registry files. Compilation/render smoke results are separate from measured pixel parity.
- Most CSS properties are one large match in `src/style.rs`, and painting is in `src/element.rs`. Parallel edits must own separate files or separate match arms.
- HTML pixel tests clear to opaque black, inject Roboto, and set `body{margin:0}`. Treat a pixel as empty when r, g, and b are below 20.

## Verification

- `cargo test -p director-html --release`
- `tests/project.rs` verifies portability after source deletion, checksums, asset deduplication, explicit fonts/images, literal text, edit rejection, sampled inspection and real media lifetime.
- Media tests require FFmpeg and ffprobe in PATH and fail when tools or fixtures are unavailable.
- `cargo bench -p director-html --bench html`
- `cargo test -p director-cli`
- `node crates/director-html/tools/parity.mjs --help`
- `node --test crates/director-html/tools/tools.test.mjs`
- `node crates/director-html/tools/gpu-benchmark.mjs --input examples/html/native-motion.html --director target/release/director-engine --output target/new-gpu-comparison` requires a Vulkan build and the selected encoders.

## Child DOX Index

- None.
