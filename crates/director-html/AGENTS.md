# Purpose

- Compile HTML/CSS and HyperFrames compositions into Director scenes without browser frame rendering.

## Ownership

- `src/` owns parsing, cascade, style translation, deterministic animation, native element adapters, and captured-scene loading.
- `src/property.rs` owns the property table: every compiled longhand, whether it inherits, and its translation group. `css::inherited` and the unsupported-property refusal read it. `src/computed.rs` owns `ComputedStyle`: the text map plus a typed Lightning CSS `Property` per longhand, reached through `computed(name)`. It derefs to the text map for text-path readers.
- `src/style/mod.rs` owns `translate`, group dispatch and the text-path properties: color, font-size, font-family, font-variant-numeric, gaps, grid templates and placements, background-image and the background-position shorthand, mask-image, transform and perspective origins, line clamps, decoration lines and thickness, underline offset, text stroke, paint-order, object-fit and object-position, clip-path, animation and transition. Typed group modules: `style/layout.rs`, `style/background.rs`, `style/border.rs`, `style/boxes.rs` (opacity, visibility, overflow, z-index), `style/effects.rs` (box shadows, filters, backdrop blur, blending, mask mode), `style/transform.rs` and `style/text.rs`. A group module returns `None` for names it leaves to the text path.
- `src/element/mod.rs` owns `HtmlElement`, its layout hooks, and the paint order: transform, clip path, effect layer, outer shadows, background, inset shadows, content and children, borders, mask. `src/element/background.rs` owns box shadows, background colors and layers, and `background-clip: text`. `src/element/borders.rs` owns border sides, styles, and corner joins. `src/element/text.rs` owns text decoration lines.
- `src/timeline.rs` owns `director-timeline/1`: the serde document, its validation and collection from the page. `animation::from_track` turns one track into a native `Animation`.
- `src/gsap/mod.rs` owns GSAP translation: it runs the page scripts at compile time in Boa against `src/gsap/shim.js`, a read-only DOM and recording GSAP subset, and turns each element's recorded tweens into `director-timeline/1` tracks. `src/gsap/ease.rs` owns the GSAP ease names and curves. `animation::Parts` owns the transform component channels.
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
- A capture's `Element::sampled_graph` returns the locked graph and root of the active sample, and `director-render` paints it with a fresh static cache. `CapturedElement::render` paints nothing itself.
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
- The cascade carries each winning declaration's parsed `Property` into `ComputedStyle` with no reparse. Only a value containing `var()` is substituted as text and parsed again. `inherit` copies the parent's text and typed value. Custom properties keep text only.
- Lightning CSS 1.0.0-alpha.72 leaves `transform-style: preserve-3d`, `text-shadow: none` and `box-shadow: none` unparsed. The typed modules match those three texts explicitly.
- `@supports` is true for `display` (`block`, `flex`, `grid`, `inline`, `inline-block`, `none`), `background-clip` (`text`, `border-box`, `padding-box`, `content-box`), `text-overflow` (`ellipsis`, `clip`), `object-fit` (`fill`, `cover`, `contain`, `none`, `scale-down`), `position` (`static`, `relative`, `absolute`, `fixed`, `sticky`), and `overflow` / `overflow-x` / `overflow-y` (`visible`, `hidden`, `clip`). Any other declaration fails.

### Layout

- `display`, `box-sizing`, `position`, sizes, insets, margins, paddings, `flex-direction`, `flex-wrap`, `flex-grow`, `flex-shrink`, `flex-basis`, the `align-*` and `justify-*` properties, `aspect-ratio`, and `grid-auto-flow` translate from typed values in `style/layout.rs`. Grid templates, grid placements, and gaps stay on the text path.
- `display` accepts `block`, `flex`, `inline-flex`, `grid`, `inline-grid`, and `none`; `inline` asks for a rich text parent. Alignment accepts `normal`, `stretch`, `start`, `end`, `flex-start`, `flex-end`, `center`, and first `baseline`; content alignment adds the `space-*` values. Overflow keywords, `left`, `right`, `self-*`, and `last baseline` fail. Sizes other than `auto`, `none` for maxima, and a length fail.
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
- `background-origin: content-box` insets by the border plus padding; percentage padding uses the element width. `background-clip` `content-box` clips to the content edge, `padding-box` to the padding edge, and `border-box` paints the border box. A per-layer origin or clip list whose layers all agree acts as that one value; a mixed list fails. Fixed or local attachment fails.
- A uniform dashed or dotted border (same line, width, and color on every side) strokes one inset rounded rect. Mixed side styles clip to the border ring, fill solid sides with center-meeting wedges, and stroke dashed or dotted sides along their midlines. Dashes are `3w` on and `3w` off. Dots are a zero-length dash with a `2w` gap and a round cap. Gaps show the element background.
- Border styles also accept `double`, `groove`, `ridge`, `inset`, and `outset`. `hidden` zeroes the used width. `wavy` fails.
- `mask-mode: luminance` writes Rec.709 luma into alpha and blends with `DstIn`. A non-gradient mask image fails.
- `clip-path` accepts `polygon()`, `circle()`, `ellipse()`, and `inset()` without `round`. `url()` fails.
- `filter` accepts `invert`, `sepia`, and `opacity` after the existing color matrix, clamped to `0..=1`.
- An empty filter call takes the CSS default amount: `blur()` and `hue-rotate()` are no-ops, and `brightness()`, `contrast()`, `saturate()` and `grayscale()` take 1. `tests/filter_fns.rs` covers a chain of default amounts.
- `object-fit` accepts `fill`, `cover`, `contain`, `none`, and `scale-down`; the core `ObjectFit` default stays `Cover`. `object-position` positions image and video content, including a captured video's shared decoder. The default is 50% 50%. Keyword order can swap axes. One keyword plus a length sets the other axis.

### Text

- `text-decoration` paints `underline`, `overline`, and `line-through`. Styles are solid, double, dotted, dashed, and wavy. `text-decoration-color` paints that color; `currentcolor` stays the text color. Thickness is `auto`, a length, or a percent of 1em. `from-font` uses the font underline thickness and falls back to auto when that metric is missing or not positive. `text-underline-offset` shifts only the underline; `auto` and `0` leave it, and a percent is a fraction of 1em.
- `text-transform` accepts `none`, `uppercase`, `lowercase`, and `capitalize`. `capitalize` uppercases the first character of each whitespace-separated word. `full-width` and `full-size-kana` fail.
- `text-shadow` paints a list of shadows in CSS order, the first on top; `none` paints none. A non-zero spread fails.
- `overflow-wrap` and `word-wrap` accept `normal`, `break-word`, and `anywhere`; `break-spaces` fails. `word-break` accepts `normal`, `break-word`, and `break-all`; `keep-all` fails. `anywhere`, `break-word`, and `word-break: break-word` break a token that does not fit. `break-all` also splits a token that overflows the space left on the line. `normal` keeps that token on one line, and left alignment lays the line out past the box. Soft min-content stays the unwrapped line, so `anywhere` does not shrink to one character.
- Text in a box sized to its own text stays on one line. Taffy rounds box edges to whole pixels, so such a box can come out up to a pixel narrower than its line, and Skia also breaks a line laid out at exactly its own width. `layout_paragraph` gives a line `WRAP_SLACK` (1 px) past its box before the next word wraps, lays out a line within a pixel of its box a pixel wider than itself, and the element keeps the paragraph's alignment edge, so centred text stays centred. `append_wrapped` adds up unkerned token widths, which run several pixels past the shaped line at display sizes, so `wrap_width` skips that pre-wrap when the shaped lines already fit. `tests/shrink_wrap.rs` covers each alignment at four sub-pixel offsets, with kerned text up to 72 px.
- `white-space: pre-line` keeps hard breaks and collapses spaces. `pre-wrap` preserves spaces and wraps.
- `<br>` is a hard break under every `white-space` mode. A collapsible space beside it is dropped, and a trailing `<br>` adds no line. An inline child (`span`, `b`, `strong`, `em`, `i`, `small`) of a text block may set inherited and `font-*` properties. It ignores `box-sizing` and zero margins and paddings, which a `*` reset gives every span. Any other property on it fails with `styled inline boxes require extraction`. `tests/rich_text.rs` covers both.
- `font-variant-numeric: tabular-nums` sets `tnum`. `text-indent` inserts a leading placeholder; `hanging` and `each-line` fail. `-webkit-text-stroke` paints one stroke; width `0` skips it. A stroke colour that differs from an opaque fill paints from its own paragraph, ordered by `paint-order`. `paint-order` takes `normal` or `fill`, `stroke`, and `markers` once each in any order; unlisted layers follow in the order fill, stroke, markers. `normal` paints the stroke over the fill, and `stroke fill` paints it under. A repeated or unknown layer fails. Neither property inherits, so set both on the element that holds the text.
- The `font` shorthand compiles. `font-stretch` and `font-variant-caps` accept only `normal`; any other value fails. `tests/font_shorthand.rs` covers both.

### Transforms and 3D

- Both transform paths wrap rotate and skew angles into `[0, 360)` before asking lightningcss for a matrix. lightningcss builds no matrix for a negative angle or a full turn and above, and a list without a matrix paints untransformed. `tests/transform_angles.rs` covers negative rotate, rotate past a turn, and negative skew.
- A transform list that contains a projective function resolves lengths to pixels and uses the lightningcss matrix. A 2D result stays on the affine canvas matrix. A 3D result is the column-vector matrix in row-major order with translation in the fourth column. Non-projective lists keep the 2D path.
- `perspective` projects descendants only. It wraps descendant drawing as `translate(origin) * perspective(distance) * translate(-origin)`; the element's own text, image, and SVG draw before that wrap. The default origin is `50% 50%`. Distance and `translateZ` must be absolute px. An absolute `transform-origin` z pivots the local matrix. A percent z and `perspective-origin` z fail.
- `transform-style: preserve-3d` depth-sorts by ascending Z of each child's center after its own local matrix, then z-index, then source order. A direct child that also preserves 3d contributes its descendants, and depth sums along that chain. A non-sharing child stays one plane. An affine child has Z 0. The depth key excludes the parent perspective. A plane whose Z changes across its quad, such as crossing `rotateX` cards, is occluded per pixel; constant Z stays on the center sort. `flat` keeps z-index order. Other values fail.
- `backface-visibility: hidden` skips an element whose own matrix `m33` is negative. The cleared frame is opaque black, so a hidden backface reads as `[0, 0, 0, 255]`.
- CSS animation identity and interpolation cover `rotateX`, `rotateY`, `rotateZ`, `translateZ`, `translate3d`, and `perspective`. The identity perspective distance is `1e6` px. Animated lengths on those functions must already be px.

### SVG

- An external stylesheet may set `fill`, `fill-opacity`, `fill-rule`, `color`, `stroke`, `stroke-width`, `stroke-linecap`, `stroke-linejoin`, `stroke-dasharray`, `stroke-dashoffset`, `stroke-opacity`, `stroke-miterlimit`, `opacity`, `transform`, `clip-path`, `clip-rule`, `filter`, `mix-blend-mode`, `marker-start`, `marker-mid`, `marker-end`, `mask`, `font-family`, `font-size`, `font-weight`, and `font-style` on SVG descendants. Each declaration is appended to the element's `style` attribute. `transform` values drop `px` because usvg rejects them. `clip-path` and the marker longhands are copied as printed. The `mask` shorthand is stored as `mask-image` and written back as `mask`; HTML `mask-image` stays on the HTML mask path. External `animation` and its longhands fail.
- An internal `<style>` inside the SVG stays in the markup and is not added to the document CSS.
- SVG `<text>` is shaped with the bundled Roboto, Inter, and Poppins faces plus document `@font-face` faces. The default family is Roboto and the default size is 16. The font database loads on the first SVG. Rhai `VectorNode::new` still parses with an empty font database.
- resvg clamps each isolated group layer (clip-path, mask, opacity, filter) to `-2 * size .. 3 * size` of its target in the parent layer's coordinates, so paint reaching far left or above the viewBox through three or more nested layers vanished. `VectorNode` pads the raster left and top past every layer's reach, up to twice the output size per side, and crops back. `node/vector.rs` tests cover left reach, top reach, and the unpadded case.
- An authored `foreignObject` lays out its HTML children in that viewport and paints them after the SVG raster. `x`, `y`, `width`, and `height` accept a number, `px`, or a percent of the SVG viewport, and user units map through `viewBox`. The box clips overflow. A non-positive width or height paints nothing. Loose text fails with `foreignObject text requires an HTML element`. A transform on the `foreignObject` or an SVG ancestor fails with `transformed foreignObject requires extraction`. A nested `foreignObject`, or one inside a nested `svg`, `symbol`, `defs`, `marker`, `pattern`, `clipPath`, or `mask`, requires extraction.
- Authored SMIL: `animate`, `animateTransform`, `animateMotion`, and `set` are sampled at the element's local time, patched into the markup, stripped, and reparsed. One animation per attribute. `animate` and `animateTransform` interpolate numbers or colors linearly. `animateMotion` follows a paced polyline and appends a translate. `set` holds `to`. `fill` is `freeze` or `remove`. The layout box stays the time-0 SVG size. Spline timing, `keySplines`, `keyPoints`, `mpath`, begin or end lists, `additive` other than replace, `accumulate` other than none, `min`, `max`, motion `rotate` auto, path curves, and unknown SMIL attributes require extraction. The animated SVG marks the element as time-varying for the static cache.
- Scripts and nested images fail with `SVG scripts and nested images require native translation`.

### Scene document

- Every native adapter has a document body. `src/export.rs` writes `HtmlElement` as `NodeBody::Html` from `base_style` and `base_paint`, so an export after rendering records the unanimated box. Its opacity channel is the base paint opacity. Text content uses `nodes::text_export`, because the stroke paragraph rebuilds from the paint.
- Gradients, CSS animations, Paper shaders and SMIL animations convert in their own modules through `to_doc`. `PaintStyle::filters` keeps the typed filter list beside the compiled `filter` chain, and both change together.
- Background images and inline SVG markup embed as assets. Paper shader images embed as raw RGBA8 (`application/x-rgba8`).
- Native and captured videos have kind `html_video` and export `HtmlVideoBody` with their loop flag and duration. A non-looped native video exports the core video body. A capture exports each distinct sampled graph once through `export_graph`, with a per-sample index list.
- Fonts registered from HTML stay outside the document, matching core text.
- Audio and video with a file `src` export as a path asset holding that `src`, which the compile `AssetStore` resolves against `base_dir` or frozen blobs. Captured media record their packed asset reference the same way. Audio `data-media-start` becomes the track `source_start`; video keeps it on the HTML body. A data URI `src` embeds the decoded audio samples or the video file bytes.
- `tests/scene_document.rs` round-trips every `examples/html` file, a paint fixture, SMIL, a looped video with a path asset, file and data URI audio, a two-graph capture and a Paper shader through JSON.
- `src/import.rs` is the inverse for the HTML kinds: `register_import(&mut registry)` adds the `html`, `html_video`, `html_capture` and `svg_smil` decoders. The `html` decoder rebuilds `PaintStyle` (the compiled `filter` chain from `filters`, the transform from its CSS text), decodes the content body through the registry, rebuilds text content with `compiler::prepare_paragraph`, and restores animations and the Paper shader through each module's `from_doc`. Video content of a box goes through `capture::decode_box_video`, so boxes with equal video and timing share one decoder through `ImportContext::shared`, matching the sharing between capture samples.
- `capture::decode_capture` imports each sampled graph through `scene_import::import_graph`, restores every node's sampled rect from `CaptureGraph::rects` and calls `post_layout` with it, and keeps equal samples on one shared graph. `smil::decode` rebuilds `AnimatedSvg` from its markup asset and animations, resolving SVG text against the bundled faces (`compiler::bundled_svg_fonts`) plus the document's font faces.
- `AssetStore` implements `AssetLoader::media_path`, so imported path-asset video decodes from the project file.

### Timeline JSON

- `<script type="application/director-timeline+json">` carries a `director-timeline/1` document: `{format, tracks: [{target, property, keyframes: [{time, value, ease?}]}]}`. Unknown fields fail. Several timeline scripts merge into one timeline.
- `target` is an HTML `id`, or a structural path below the composition root: `/` for the root, `/i/j` with zero-based element child indices. `time` is seconds on the composition clock. `value` is CSS text, or a number: unitless for `opacity`, px for `width`, `height`, `left`, `top` and `border-top-left-radius`. Color and transform tracks take CSS text.
- `ease` takes a CSS timing function and shapes the segment arriving at its keyframe; it defaults to `linear`. A track holds its first value before its first keyframe and its last value after its last one. A single keyframe holds for the whole timeline.
- Each track compiles to a native `Animation` (delay = first time, fill `both`) after the element's `@keyframes`, so it exports and edits as a `CssAnimation`.
- Refusals: another format, a duplicate target and property, an empty track, negative or non-finite times, times that do not increase, an unsupported property, a number on a color or transform track, a target with no animatable element, and a property that `@keyframes` also animates on the same element.
- `tests/timeline.rs` covers holds, easing, numeric values, color interpolation, scene export, serde round trips and refusals.

### GSAP

- Inline scripts run once at compile time in Boa (`boa_engine` 0.22) over a pre-order snapshot of the element tree. `src` scripts from a GSAP build (`/gsap@`, `/gsap/` or a `gsap*` file name) are skipped; any other `src`, and a non-JavaScript `type`, fail. `DOMContentLoaded` and `load` listeners run after the scripts.
- The shim exposes `querySelector(All)`, `getElementById`, `getElementsByClassName`, `getElementsByTagName`, `matches`, `closest`, `children`, attributes, `dataset`, `textContent`, `classList.contains` and `window.__timelines`. Style writes, DOM mutation, layout reads, `Math.random`, `Date.now`, `performance.now`, `requestAnimationFrame`, timers, `matchMedia`, `fetch` and observers fail.
- The exported timeline is the one registered in `window.__timelines` under the composition id, or the only one registered. An unregistered timeline with tweens that is not nested fails. `gsap.to`, `gsap.from` and `gsap.fromTo` outside a timeline fail; `gsap.set` outside a timeline sets the starting state.
- Timelines take `to`, `from`, `fromTo`, `set`, `add` of a nested timeline, `addLabel` and GSAP position parameters (absolute, labels, `<`, `>`, `+=`, `-=`, percentages). Timeline `defaults` and `gsap.defaults` take `duration` and `ease`; GSAP's own defaults are 0.5 s and `power1.out`. Tweens take `duration`, `delay`, `ease`, `repeat` (including -1, which repeats until the composition ends), `yoyo`, `repeatDelay`, `immediateRender`, function values and `stagger` as a number, a function or `{each|amount, from}`.
- Properties: `opacity`, `autoAlpha`, `x`, `y`, `z`, `xPercent`, `yPercent`, `rotation`/`rotate`/`rotationZ`, `rotationX`, `rotationY`, `skewX`, `skewY`, `scale`, `scaleX`, `scaleY`, `width`, `height`, `left`, `top`, `borderRadius`, `color` and `backgroundColor`, with `+=` and `-=` values. `x` and `y` given in `%` drive the percent channels.
- Transform properties become component tracks (`translate-x`, `translate-y`, `translate-z`, `translate-x-percent`, `translate-y-percent`, `rotate-z`, `rotate-x`, `rotate-y`, `skew-x`, `skew-y`, `scale-x`, `scale-y`), composed in GSAP order over the element's CSS transform. The CSS transform must decompose into translations, rotations, one skew and scales; a component track over any other transform, or beside a `transform` track, fails.
- Tracks follow GSAP seek semantics: immediate `from`, `fromTo` and `set` set the starting state in script order, a `to` starts from the value its channel holds at its start time, and each tween shows until the next tween on its channel starts. Repeats, yoyo and repeat delays unroll into keyframes. Exact eases keep their GSAP name (`CssTiming::Gsap`); `steps(n)` becomes jump keys and a tween cut short by a later one is sampled once per frame.
- Refusals: callbacks, `keyframes`, `startAt`, `yoyoEase`, `transformOrigin`, `clearProps`, plugins, `timeScale`, `call`, `addPause`, timeline `repeat` and `yoyo`, `repeat` with `stagger`, unsupported eases and properties, and an earlier tween on a channel that outlives a later one. GSAP on an inline text span or outside the composition fails. Color tracks interpolate premultiplied while GSAP blends straight RGBA, so they differ while alpha changes.
- `director-engine html timeline` prints the emitted tracks. `tests/gsap.rs` and `tests/fixtures/gsap` (entrance, loop, stagger) are the phase 5 GSAP gate set: keyframes, pixels, scene export and refusals.

### CSS animation

- Authored `@keyframes` become native animation tracks whose duration, easing, and values stay editable on HTML recompile. Scroll-driven keyframes fail. GSAP timelines become `director-timeline/1` tracks (see GSAP); other executable JavaScript fails.
- `HtmlElement::local_transform` gives the core motion policy the animated transform about its origin, so keyframed movement counts toward motion scoring. A 3D transform contributes its planar part. `tests/motion_policy.rs` covers a static box, a moving box, and one over the speed limit.
- `animation-direction` accepts `normal`, `reverse`, `alternate`, and `alternate-reverse`. `animation-fill-mode` accepts `none`, `forwards`, `backwards`, and `both`. `steps()` with `jump-none` or `jump-both` fails.

## Work Guidance

- Preserve arbitrary-time seek determinism. Reuse immutable assets and resolve static styles before rendering.
- Keep measured parity and performance claims tied to reproducible commands and artifacts.
- CPU/GPU comparisons must synchronize GPU work through readback, verify the actual backend/encoder and probe every MP4's frame count and audio/video/container duration. GPU rendering and NVENC encoding are separate controls.
- Exercise real Hyper CN original and portrait registry files. Compilation/render smoke results are separate from measured pixel parity.
- A new CSS longhand needs a `src/property.rs` entry. Port a property group to typed values by matching Lightning CSS `Property` variants in its own `src/style/` module. Painting stages live in `src/element/`. Parallel edits must own separate files or separate match arms.
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
