# Purpose

- Compile HTML/CSS and HyperFrames compositions into Director scenes without browser frame rendering.

## Ownership

- `src/` owns parsing, cascade, style translation, deterministic animation, native element adapters, and captured-scene loading.
- `src/project.rs` owns `director-native-project/1`, editable source and validated element overrides; `src/assets.rs` owns asset recording, deduplicated SHA-256 blobs and temporary media-file leases.
- `tools/` owns optional compile-time browser extraction, Chromium and CPU/GPU parity, real HyperFrames CLI and CPU/GPU export benchmarks, and Hyper CN registry auditing.
- `tests/` owns executable compatibility and pixel regressions.
- `benches/` owns Criterion compile/render workloads.
- `README.md` owns supported features, incompatibilities, commands, and reproducible measured results.

## Local Contracts

- Nested style rules flatten into the same selector list as top-level rules. `&` is replaced with the parent selector. A parent list becomes `:is(...)`. Nested `@media` and `@supports` still fail.
- Depend on `director-core`; do not introduce HTML behavior into Rhai or the core dependency-graph compiler.
- Use upstream HTML, CSS, selector, layout, typography and SVG libraries. Unsupported syntax or rendering behavior must produce diagnostics; compilation fails when errors remain.
- Browser extraction is explicit and compile-time only. Capture layout, style, SVG and media references, never screenshots as native scene content. Captures declare their sample rate and cannot claim continuous animation equivalence.
- Do not treat Hyper CN WebGL/canvas content as supported until a native translation exists. Registry audits must distinguish renderable inputs from diagnosed incompatibilities.
- The smoke extractor forwards a non-none computed `perspective` with its origin, `backface-visibility: hidden`, and SVG-descendant `filter` / `mix-blend-mode`. It copies `text-decoration-line` when every token is `underline`, `overline`, or `line-through`, and copies `mask-mode: luminance`. Other decoration values, other mask modes, `background-blend-mode`, and image-url backgrounds stay refused. `node --test crates/director-html/tools/tools.test.mjs` passed 6 tests on 2026-10-17. That is not a Chrome re-smoke. A canvas with a captured Paper Shader fragment compiles to SKSL and draws in the element content rect. A canvas with no fragment stays refused, as do other WebGL and Three.js runtimes. `fwidth` is a constant screen-width estimate, and `textureGrad` drops explicit derivatives. The 2026-10-15 shader smoke rendered 26/26 original and 26/26 portrait. Carried forward with the 213 blocks that were not re-run, coverage is 239/239. Smoke counts compilation and render. Pixel parity is separate.
- Captured muted video shares one decoder per element id. Sample rect and `object-fit` may change. The active sample seeks that decoder to the composition time. Captured audio is one mixer track per id, including an element the visibility window hides. `data-hidden` sets track volume 0. Audio `loop` loops the track. `data-media-start` drains samples the same way as static HTML. Bytes come from the capture asset map. Unmuted video, an audio playback rate other than 1, iframe, a duplicate media id in one frame, and a media id whose src or timing changes across samples fail. Muted `video loop` seeks with `media_start + elapsed * playback_rate`, then wraps into `[0, duration)`. Duration comes from ffprobe. The capture extractor records `looped` and no longer refuses the attribute. DOM paint stays on the held sample.
- Generated captures, videos, reference checkouts and benchmark reports live under ignored `target/` or user-selected outputs.
- Native adapters must remain semantically Send under the core scene-node contract. Share captured graphs behind `Arc<Mutex<SceneGraph>>`, never `Rc` or an unprotected shared graph.
- Native projects preserve HTML/capture authoring data and compile it on load. Do not serialize HTML adapters through core project I/O, which owns a different node-kind contract.
- Pack every consumed local stylesheet, font, image, audio and video plus required bundled fonts. Frozen project compilation cannot read original files or substitute host files for missing blobs; hash mismatches fail.
- Apply edits by stable authored element ID and validate the complete candidate before acceptance or atomic save. Captures retain sampled timing and fixed geometry; reject edits requiring uncaptured layout/animation.
- Preserve unique authored capture IDs. Anonymous/duplicate IDs receive noncolliding structural paths, stable while sampled topology is stable; expose those paths in inspection and store them in packed captures.
- Keep extracted media files alive through the Director asset-loader lifetime, even when the project or compiler wrapper is dropped.
- Share immutable source and encoded blobs across edit candidates; validation must not clone entire media payloads merely to change an edit map.
- Repeating linear, radial, and conic gradients expand stops across the box and clamp. Color hints expand in sRGB before that tiling. A hint must sit strictly between two stops. A 50% hint stays linear. A hint on the left stop becomes the next color, and a hint on the right stop becomes the previous color. Any other hint inserts nine sRGB stops. Vendor-prefixed gradients fail. An orphan hint fails. A hint that parses and then fails expansion paints nothing.
- Uniform dashed and dotted borders stay one inset stroke. Mixed side styles clip to the border ring and stroke each dashed or dotted side on its midline.
- Parent `perspective` projects descendants only. `transform-style: preserve-3d` depth-sorts direct children by the Z of each child's center after its own local matrix, then z-index, then source order. Positive `translateZ` paints later. `flat` keeps z-index order. Other values fail. That Z does not include the parent perspective. `backface-visibility: hidden` skips an element whose own matrix faces away. An absolute `transform-origin` z pivots the local matrix and is included in preserve-3d depth. A percent z still fails. `perspective-origin` z still fails. Perspective distance and `translateZ` are absolute px. The capture extractor copies `transform-style` when the computed value is `preserve-3d`.
- An external stylesheet may set SVG `fill`, `color`, `stroke`, `stroke-width`, `stroke-linecap`, `stroke-linejoin`, `stroke-dasharray`, `stroke-dashoffset`, `opacity`, `transform`, `clip-path`, `filter`, and `mix-blend-mode`. `transform` values drop `px` before usvg. `clip-path` is copied as `url(#id)`. External `animation` and its longhands still fail. SVG text, foreignObject, SMIL, scripts, and nested images stay rejected.
- Captured style edits do not accept perspective, transforms, or border-style. Authored HTML recompiles those properties.
- A single `url()` or `data:` background paints after gradients and before content. Size accepts `auto`, `cover`, `contain`, and lengths. Repeat accepts `repeat`, `no-repeat`, `repeat-x`, `repeat-y`, `space`, and `round`. The four shader modes map image space onto the canvas. A file URL resolves through the compile `AssetStore`, so `CompileOptions.base_dir` and a frozen project blob apply. A data URI still decodes inline. A gradient shader is sized to `background-origin` and clamped across the border box. The default origin is padding-box, so a transparent border shows the end color. `border-box` sizes the gradient to the border box. Masks stay on the border box. Mixed url and gradient backgrounds fail. `text-decoration` paints `underline`, `overline`, and `line-through` in `currentColor`. Styles are solid, double, dotted, dashed, and wavy. Thickness is `auto`, a length, or a percent of 1em. `from-font` still fails. `text-decoration-color` paints that color. `currentcolor` stays the text color. A dashed underline leaves a gap. `object-position` positions image and video content, including a captured video's shared decoder. The default is 50% 50%. Keyword order can swap axes. One keyword plus a length sets the other axis. `mask-mode: luminance` writes Rec.709 luma into alpha and blends with `DstIn`. `background-origin: content-box` insets the image by the border plus the padding. Percentage padding uses the element width. `space` and `round` paint explicit tiles, including a repeat axis mixed with either one. A product above 4096 tiles is capped. `repeat`, `no-repeat`, `repeat-x`, and `repeat-y` stay on the image shader. Fixed or local attachment and a non-gradient mask image still fail. `background-clip: content-box` clips the background to the content edge. `padding-box` clips to the padding edge. `border-box` still paints the border box. `background-origin` is unchanged. Measured 2026-10-17: paint_gaps 16, object_position_css 6, lib 96, depth_sort 7, css_nesting 6, authoring_slices 2. compatibility was not re-run. CSS named color `green` is `#008000`.

- Homogeneous `calc()`, `min()`, `max()`, and `clamp()` lengths become px. Homogeneous percents stay percents (`50%` is fraction `0.5`). A mix of px and percent fails with `mixed calc() requires a length basis`. `convert_length` passes no percent basis, so the composition size is not treated as the containing block. `round`, `rem`, `mod`, `abs`, `sign`, and `hypot` fail.
- `grid-template-columns` and `grid-template-rows` accept an integer `repeat` up to 4096, `repeat(auto-fit, ...)`, `repeat(auto-fill, ...)`, and `[line]` names. `subgrid` fails. An auto repeat nested inside another repeat fails. `grid-column` and `grid-row` still accept only `auto`, a line number, or `span N`. Measured 2026-10-17 by `cargo test -p director-html --release --lib --test paint_gaps`: lib 96, paint_gaps 16, depth_sort 7. The earlier compatibility run stayed 23. An absolute transform-origin z is applied. A percent z still fails. Decoration styles come from `decoration_value` and paint through `decoration_marks`.

## Work Guidance

- Preserve arbitrary-time seek determinism. Reuse immutable assets and resolve static styles before rendering.
- Keep measured parity and performance claims tied to reproducible commands and artifacts.
- CPU/GPU comparisons must synchronize GPU work through readback, verify the actual backend/encoder and probe every MP4's frame count and audio/video/container duration. GPU rendering and NVENC encoding are separate controls.
- Exercise real Hyper CN original and portrait registry files. Compilation/render smoke results are separate from measured pixel parity.

## Verification

- `cargo test -p director-html`
- `tests/project.rs` verifies portability after source deletion, checksums, asset deduplication, explicit fonts/images, literal text, edit rejection, sampled inspection and real media lifetime.
- Media tests require FFmpeg and ffprobe in PATH and fail when tools or fixtures are unavailable.
- `cargo bench -p director-html --bench html`
- `cargo test -p director-cli`
- `node crates/director-html/tools/parity.mjs --help`
- `node --test crates/director-html/tools/tools.test.mjs`
- `node crates/director-html/tools/gpu-benchmark.mjs --input examples/html/native-motion.html --director target/release/director-engine --output target/new-gpu-comparison` requires a Vulkan build and the selected encoders.

## Child DOX Index

- None.
