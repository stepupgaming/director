# Plan 08 — Native Projects and Runtime

> **Status:** Done (2026-10-14)
> **Product direction:** A superior native platform for agents and developers.

## Scope

- Portable, editable HTML/capture authoring projects and bundled assets.
- Common native input loading for CLI export, stills, inspection and backend preview.
- GPU execution on one owner thread, source/dependency reload and export-clock audio.
- Real Hyper CN material and meaningful regression checks.
- Frontend/editor design, TypeScript/JSX authoring and Remotion source compatibility are outside this phase.

## Implementation contracts

- `director-html::NativeProject` stores `director-native-project/1`: original HTML and compile options or sampled capture data, element-ID keyed edits, asset references and deduplicated SHA-256/base64 blobs.
- Required bundled fonts and consumed local fonts/stylesheets/images/audio/video travel with the project. Reopening does not require its original directory. Frozen asset resolution rejects missing or altered blobs; temporary video files live with the Director asset loader.
- Loading recompiles the stored authoring input. This preserves HTML adapters without passing through the core serializer's node-kind policy. Rhai retains its core project contract and is not packaged by this format.
- Immutable source and encoded blobs are shared across edit candidates. Captured anonymous/duplicate element IDs receive noncolliding structural paths; unique authored IDs remain intact.
- `project edit` validates a complete candidate before accepting it and atomically saving. HTML edits support literal text, CSS and supported timing/media attributes; captured text/paint edits retain fixed geometry and sampled motion.
- CLI `render`, `still`, `inspect` and the backend share `input.rs`. JSON format identifiers select capture/native inputs; a versioned core project uses core loading.
- Still/backend and export share `director_render::FrameState` for shutter accumulation and transitions. Explicit GPU selection requires native GPU targets.
- The backend uses a bounded queue and a single native worker owning the Director, targets and Vulkan context. Source/dependency reload accepts a complete new scene or retains the accepted scene with diagnostics.
- Backend inspection exposes layout/timeline, authored IDs, sampled element information and audio metadata. Full stereo float WAV mixes use the export sample clock and cache by accepted revision.

## Acceptance

1. Pack an authored HTML input, delete its source/assets, reopen elsewhere, and match native pixels across forward/reverse seeks.
2. Preserve fonts, images, stylesheet references and real media; deduplicate equal bytes; detect hash/format errors.
3. Apply edits, save/reopen, and prove invalid edits leave accepted state and files unchanged.
4. Preserve a sampled Hyper CN composition across packing/rendering and expose its element IDs and text for inspection.
5. Common still and HTML still match; real export has matching frame count and audio/video/container duration.
6. Backend reloads changed native dependencies; invalid source retains the prior frame; corrected source advances its revision.
7. Concurrent backend requests execute on the same owner thread. Explicit live Vulkan verification includes shutter sampling and readback.

## Verification

```powershell
cargo test --release -p director-html -p director-cli --features director-cli/vulkan
cargo test --release -p director-cli --features vulkan --bin director-engine gpu_preview_worker -- --ignored
cargo clippy --release -p director-html --all-targets --no-deps -- -D warnings
cargo clippy --release -p director-cli --features vulkan --bin director-engine --test project_workflow --no-deps -- -D warnings
cargo check --release --workspace --all-targets --features director-cli/vulkan
```

Package tests require FFmpeg/ffprobe; the explicit Vulkan test requires a device. Raw projects, videos and benchmark artifacts stay under ignored `target/`. Existing full-workspace example/visual failures remain separate from this phase's acceptance checks.

Acceptance checks pass, including portable real media and a live Vulkan worker with shutter sampling. The real Hyper CN `blur-in` capture was packed, inspected, edited by its generated text ID, reopened and GPU-exported as 40 frames with matching 1.333333-second streams. Unedited portable CPU/GPU parity covers all 40 frames, SSIM .999979 and mean RGB MAE .002033. Local artifacts are under `target/native-project-proof/`. Full workspace tests still fail on four unmodified Rhai scripts; workspace lint still reports existing core warnings. Package lint and workspace all-target checking pass.

## Remaining product work

- Native paint and media fidelity continued in [plan 09](09-native-fidelity.md).
- Recipes, agent repair tooling, batch and deployed rendering, and preview audio delivery continued in GitHub issue #3. Plan 09 tracks what has landed.
