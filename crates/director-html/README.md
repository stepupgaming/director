# Native HTML compiler

`director-html` uses html5ever/scraper, Lightning CSS and Servo selectors to create Director nodes. Taffy lays them out; SkParagraph, Skia and resvg paint them; Director's FFmpeg exporter writes MP4. It generates neither Rust nor Rhai.

```bash
cargo build --release -p director-cli --bin director-engine
director-engine html check examples/html/native-motion.html
director-engine html still examples/html/native-motion.html -t 1 -o target/native.png
director-engine html render examples/html/native-motion.html -o target/native.mp4
```

`compile`, `compile_file`, `compile_capture` and `compile_capture_file` return `CompiledHtml` with a normal `Director`, static HTML ID mappings and metadata. Existing frame/export functions render it. Capture JSON is a separate interchange format; core project serialization does not serialize HTML adapters.

`NativeProject::from_file`, `load`, `compile`, `apply_edits` and `save` own portable authoring projects. `director-native-project/1` stores HTML/options or sampled DOM, validated element-ID edits and deduplicated SHA-256/base64 asset blobs. Packing includes consumed local CSS/fonts/images/audio/video and required bundled fonts. Frozen compilation rejects missing or altered blobs and never consults original files. Video materialization is temporary and survives through the Director asset-loader lifetime.

```bash
director-engine project pack examples/html/native-motion.html -o target/movie.dirproj
director-engine inspect target/movie.dirproj -t 1
director-engine project edit target/movie.dirproj --patch edits.json
director-engine render target/movie.dirproj --gpu -o target/movie.mp4
director-engine preview target/movie.dirproj --gpu --no-open
```

Loading recompiles stored native authoring input; it does not deserialize HTML adapters as core node kinds. HTML edits support literal text, CSS overrides and supported timing/media attributes. Captures retain their sample rate and rectangles; text/paint edits cannot reconstruct uncaptured layout or continuous JS animation. Inspection exposes unique authored IDs and generated structural paths for anonymous/duplicate capture IDs; path identity assumes stable sampled topology. The format embeds media in JSON, with base64 overhead; it is not a binary compiled-scene cache. Rhai remains on the core project contract. See the [native CLI/backend workflow](../director-cli/README.md).

Vulkan rendering and NVIDIA encoding are independent:

```bash
cargo build --release -p director-cli --bin director-engine --features vulkan
director-engine html render examples/html/native-motion.html --gpu -o target/native-gpu.mp4
director-engine html render examples/html/native-motion.html --gpu --encoder nvenc -o target/native-gpu-nvenc.mp4
director-engine html benchmark examples/html/native-motion.html --gpu --export target/gpu-benchmark.mp4
```

`still` also accepts `--gpu`. Explicit GPU requests fail when the build/device/surface is unavailable. CPU rendering and software encoding remain the defaults. Direct GPU readback and the vendored Skia resolver fix keep rendering native through export.

## Implemented subset

| Input | Native behavior |
| --- | --- |
| HTML | Block containers, headings/paragraphs, rich inline span/bold/italic text, raster images, SVG shapes and attribute dimensions, muted video and separate audio |
| Cascade | Servo selectors, specificity/source order, inline style, important, inheritance and custom properties substituted before shorthand ranking |
| Layout | Block/Flexbox/Grid via Taffy; dimensions/min/max, spacing, relative/absolute positioning, grow/shrink/wrap, numeric tracks/repeat/minmax/placement; px/percent/em/rem/viewport/absolute units |
| Typography | Bundled fonts and local/embedded font-face, WOFF/WOFF2 decoding, weight/italic/size/line-height/spacing/alignment, whitespace and uppercase |
| Paint | Multiple linear/radial/conic gradients, including repeating gradients without color hints, elliptical corner radii, solid borders and per-side dashed or dotted borders, outer/inset shadows and spread, grouped opacity, percentage polygon clips, overflow clips and alpha gradient masks |
| Effects | Ordered blur/brightness/contrast/saturation/grayscale/hue/drop-shadow filters, backdrop blur, common blend modes |
| Transforms | 2D and 3D lists in authored order, origin, percentage translation on X and Y, absolute px on Z and perspective. Parent perspective projects descendants. Backface hiding uses the element's own matrix. No shared depth sort |
| CSS animation | Opacity/color/background-color, matching translate/scale/rotate lists including rotateX/Y/Z, translateZ, translate3d and perspective, px width/height/left/top and uniform radius; cubic/steps easing, delay, fractional/infinite iterations, direction and fill |
| HyperFrames | Numeric start/duration/end, composition-local timing, hidden clips, dimensions/FPS, `data-no-timeline`, source start and constant positive video rate. Track indexes do not set paint order |

CSS animations evaluate immutable tracks at arbitrary times. Paint-only animation does not dirty Taffy. Paragraphs, decoded images and SVG rasters retain native caches. Compilation uses native adapters within the existing core pipeline. Native defaults use Roboto. Ship exact fonts and author a sized root with explicit backgrounds and body margin reset for Chromium comparisons.

## Compile-time extraction

Hyper CN's private JS frame renderers and dynamic GSAP DOM/SVG can be frozen:

```bash
npm --prefix crates/director-html/tools ci
director-engine html capture path/to/block.html -o target/scene.json --chrome /path/to/chrome
director-engine html render target/scene.json -o target/scene.mp4
```

`--modules` selects existing Node dependencies. Tools pin GSAP 3.14.2 and use that version for matching jsDelivr imports. Chromium is used only for capture/testing. Captures contain styles, layout, text, SVG, fonts and images, never screenshots. Native render/still never launches a browser.

Captures hold the preceding sample at arbitrary times and seek backwards deterministically. They do not claim continuous interpolation. Layout is frozen, identical frames are shared, and no JS or internal Taffy runs per rendered capture frame. Changing text/SVG can produce large files.

Standalone/template blocks and one-level external template assembly with numeric host timing work. Repeated IDs and deeper nesting require upstream assembly. This transport does not replace upstream's bundler, variable resolver or runtime; supply assembled HTML for those contracts.

## Remaining incompatibilities

- Static JS, dynamic transitions, CSS nesting/media queries and unsupported properties require explicit extraction when its DOM subset covers the result.
- Canvas/WebGL/Three.js/shader blocks need native translators. Extraction refuses screenshots as scene content.
- Named grid areas, auto-fit/auto-fill, mixed inline/block formatting, interactive scrolling/fixed/sticky, mixed calc/clamp lengths, general stacking-context escapes and browser margin collapsing are outside the static subset. Perspective does not depth-sort, and a three-value transform-origin fails.
- Gradient color hints, non-alpha masks, text decoration and image backgrounds are not translated.
- SVG text, foreignObject, SMIL, scripts and nested images are rejected. An external stylesheet may set `filter` and `mix-blend-mode` on SVG descendants. Other external SVG presentation properties are diagnosed. SVG attributes, inline/internal styles and inherited currentColor use usvg/resvg. Extraction resolves SVG colors and planar child transforms.
- Captured media still needs integration; static native media works. Video loops and audio rate changes are rejected.
- Font metrics/rasterization and complex stacking can differ from Chromium. Compilation success is not a parity certificate.

Unsupported parsed CSS/animation properties, selectors, at-rules, media behavior and HyperFrames attributes fail with context. Read each input's diagnostics; arbitrary browser content is not supported.

## Verification and measurement

```bash
cargo test -p director-html
cargo clippy -p director-html --all-targets --no-deps -- -D warnings
cargo bench -p director-html --bench html
node --test crates/director-html/tools/tools.test.mjs
node crates/director-html/tools/parity.mjs --input examples/html/native-motion.html --root . --output target/parity --director target/release/director-engine --chrome /path/to/chrome
node crates/director-html/tools/parity.mjs --input path/to/hypercn/block.html --capture --fps 30 --output target/hypercn-parity --director target/release/director-engine --chrome /path/to/chrome
node crates/director-html/tools/registry-audit.mjs --registry /path/to/hypercn/registry --output target/hypercn-audit --director target/release/director-engine --fps 2 --chrome /path/to/chrome
node crates/director-html/tools/hyperframes-benchmark.mjs --project /path/to/portable-project --output target/engine-benchmark --director target/release/director-engine --hyperframes-cli /path/to/hyperframes/bin/hyperframes.mjs --chrome /path/to/chrome
node crates/director-html/tools/gpu-benchmark.mjs --input examples/html/native-motion.html --director target/release/director-engine --output target/gpu-comparison
```

FFmpeg/ffprobe are required for the real video test and export. Tests cover pixels, cascade, grid/flex, group opacity, gradients/masks, borders, keyframes/reverse seeks, captures, images, diagnostics and video offsets. Registry smoke auditing uses a low sample rate; `--portrait` selects portrait files. Run separate 30fps parity for motion evidence.

Parity writes both PNG sequences, MP4s and JSON to a new directory. Identical timestamps are compared by RGB MAE/RMSE/max-channel/changed pixels and FFmpeg SSIM. Default gates are per-frame MAE <= 2 and aggregate SSIM >= .99. Failures exit nonzero; baselines are never auto-replaced.

Reports separate first/warm compile/render, browser startup/capture, extraction and export costs. Native FPS includes layout, paint, submission and synchronous RGBA readback; PNG encoding is excluded. Browser FPS includes screenshot encoding. Compare export totals for end-to-end work. Native RSS is a process high-water mark; the Windows browser metric sums live process high-water marks, an upper bound for those processes. FFmpeg and GPU VRAM are excluded. OS caches are not purged. The shared seeker does not certify every upstream engine feature.

The separate HyperFrames benchmark invokes the installed upstream CLI, records fresh-process first/warm export wall times, verifies both videos with ffprobe, and compares encoded SSIM. Preserve its logs to identify capture/GPU paths and correctness warnings. `gpu-benchmark.mjs` exercises CPU/Vulkan and software/NVENC independently, checks every timestamp, and rejects MP4 audio/video/container duration mismatches.

Contracts were studied against [HyperFrames](https://github.com/heygen-com/hyperframes) commit `8147f7b940aaccde963a3ed5a3ee6099abf2b8cf` and the local [Hyper CN registry](https://github.com/stepupgaming/hypercn). Upstream JS is confined to optional reference/extraction tooling.

## CPU/GPU measurements

Measured on 2026-10-13 from the staged source, excluding unrelated working-tree edits, with the same Ryzen 9 9900X and RTX 5060 Ti, NVIDIA driver 616.64. The complete native-motion clip is 1920x1080, 90 frames at 30fps. All MP4 streams and container durations are verified at 3 seconds. Frame FPS includes synchronous RGBA readback in both modes. Export wall times use fresh CLI processes and include startup, compilation, GPU initialization, painting, encoding and muxing.

| Renderer / encoder | Frame FPS, first / warm | Complete export, first / warm |
| --- | ---: | ---: |
| CPU / software H.264 | 95.54 / 95.79 | 1.246 / 1.232 s |
| CPU / NVENC | 97.41 / 97.36 | 1.434 / 1.464 s |
| Vulkan / software H.264 | 311.50 / 390.61 | .781 / .764 s |
| Vulkan / NVENC | 373.64 / 370.15 | .960 / .947 s |

CPU/GPU PNG parity passes with SSIM .999778 and mean RGB MAE .039063 over all 90 timestamps. GPU setup took about 133–145 ms per benchmark process. CPU process peak RSS was about 63 MiB; GPU process peak RSS about 139 MiB, excluding FFmpeg and VRAM. NVENC was slower than software encoding on this short clip; hardware encoding is not a universal speed claim.

The full captured Hyper CN original blur-in animation also passes CPU/GPU parity over all 40 timestamps, SSIM .999979 and mean MAE .002033. The harness is reproducible with an existing capture JSON and performs no browser work. Raw reports and MP4s are in `target/html-gpu-staged-comparison` and `target/html-gpu-staged-hypercn-comparison`.

The shared exporter now supplies sample frames, rather than interleaved samples, to the audio mixer. A real regression test reproduced the previous doubled audio duration and verifies matching audio/video/container durations after the fix.

CPU and Vulkan CLI tests pass, including an explicitly run live-device readback test. HTML compatibility/media tests, the seven core determinism/media-clock tests, Node harness tests, strict primary-CLI and HTML-crate Clippy, and workspace all-target checks with and without Vulkan pass.

## Earlier Chromium/HyperFrames measurements

The results below were collected from the development working tree before the GPU/readback and audio-duration fixes. Native MP4s from that run contain the correct video frames but doubled audio duration; their old export times are historical, not current complete-export baselines. The unencoded visual parity and registry coverage remain separate valid measurements. The earlier native FPS excludes readback and is not directly comparable to the synchronized CPU/GPU FPS above.

Measured on 2026-10-13 with Windows x64, Ryzen 9 9900X, Node 26.3.1, Chromium headless shell 152.0.7928.2 and HyperFrames CLI 0.8.107. Director used CPU Skia and software H.264. HyperFrames detected an RTX 5060 Ti and used screenshot capture for CSS animations. These results describe the tested compositions; OS caches were not purged.

| 3-second 1920x1080/30fps sample | First | Warm |
| --- | ---: | ---: |
| Native complete export, fresh processes | 1.990 s | 1.667 s |
| HyperFrames complete export, automatic workers | 7.863 s | 7.462 s |
| Native complete export, separate one-worker comparison | 2.232 s | 1.871 s |
| HyperFrames complete export, one worker | 8.627 s | 8.346 s |
| Native compilation in the PNG parity run | 12.024 ms | 9.622 ms |
| Native paint/layout FPS in the PNG parity run | 133.88 | 141.82 |

Automatic HyperFrames rendering selected three workers. Both MP4s contain 90 frames at 30fps; their encoded SSIM is .991153. The native process high-water mark in the PNG parity run was 47.40 MiB. The browser's sampled sum of live-process high-water marks was 293.29 MiB. This browser metric is not a simultaneous whole-tree peak, and neither metric includes the separate FFmpeg process.

| Unencoded PNG parity, every timestamp | Frames | SSIM | Mean RGB MAE, 0–255 | Gate |
| --- | ---: | ---: | ---: | --- |
| Native CSS/Flexbox/Grid/typography/SVG, 1920x1080 | 90 | .990279 | .650255 | Pass |
| Hyper CN `blur-in`, original 1280x720 | 40 | .998416 | .067675 | Pass |
| Hyper CN `blur-in`, portrait 1080x1920 | 40 | .997298 | .150835 | Pass |

The Hyper CN captures cover the complete 1.333-second animation at 30fps. Extraction took 1.490 s for original and 1.479 s for portrait before native compilation. Original native paint/layout ran at 127.61/130.29 FPS; portrait at 68.27/70.97 FPS. Captured-scene export times exclude extraction, so a single conversion/export can cost more than reusing an existing capture. The registry smoke audit renders 205/239 original and 205/239 portrait files at 2fps. The remaining 34 per profile were diagnosed as WebGL/shaders, perspective, some SVG filters/blending, a nonuniform dashed border and a repeating conic gradient. Plan 09 slice 1 translates those paint cases. The 205/239 count stands until the audit is run again. Smoke coverage is not a visual parity claim.

Criterion with 10 samples, one-second warm-up and two-second measurement reported compile time 8.067 ms and native frame time 7.510 ms, with intervals 7.888–8.160 ms and 6.675–8.224 ms. Raw local artifacts are in `target/html-criterion-final.log`, `target/html-hf-auto-final`, `target/html-hf-engine-final`, `target/html-parity-native-final`, `target/html-parity-hypercn-final`, `target/html-parity-portrait-final`, `target/html-hypercn-verified` and `target/html-hypercn-portrait`.

To reproduce the official CLI comparison, stage the same sample as a portable HyperFrames project with its two font files, then run the benchmark tool. In PowerShell from the workspace root:

```powershell
$stage = 'target/html-hf-project'
New-Item -ItemType Directory -Path "$stage/assets/fonts" -Force | Out-Null
$html = (Get-Content examples/html/native-motion.html -Raw).Replace('../../crates/director-core/assets/fonts/', 'assets/fonts/')
Set-Content -LiteralPath "$stage/index.html" -Value $html -Encoding utf8 -NoNewline
Copy-Item crates/director-core/assets/fonts/Roboto-Regular.ttf,crates/director-core/assets/fonts/Roboto-Bold.ttf "$stage/assets/fonts/"
node crates/director-html/tools/hyperframes-benchmark.mjs --project $stage --output target/new-engine-comparison --director target/release/director-engine.exe --hyperframes-cli C:/Projects/hypercn/node_modules/hyperframes/bin/hyperframes.mjs --workers auto
```

Full workspace tests previously stopped on four unmodified Rhai examples. A run excluding that example-validation test reached six core visual-suite failures, including a bundled-font path failure and snapshot differences. Those runs do not certify a fully green workspace; unrelated source and baselines were preserved.
