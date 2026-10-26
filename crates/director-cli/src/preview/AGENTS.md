# Purpose

- Serve the native preview API (frames, inspection, audio and reload) for editors and agents.

## Ownership

- `mod.rs` owns Axum routes, response contracts and backend acceptance tests.
- `worker.rs` owns the bounded job queue, the `director_host::Session` it drives on the worker thread, `/api/meta` metadata and PNG encoding.

## Local Contracts

- Create, use and drop the `director_host::Session` and its Vulkan context on one native worker thread. Async handlers exchange owned results through the bounded queue; never move the GPU context into a Tokio task.
- Open the input as a `Session` with export media semantics. Frames paint through `Session::render_frame` on the export sampling clock, on the owned GPU context under `--gpu`, and encode as PNG.
- Before each request the worker reloads when `Session::source_changed`. A successful reload replaces the scene document, drops warm frames and audio and advances the revision, which starts at 0. A failed reload retains the accepted scene and exposes `reload_error`; forced `/api/reload` returns the error.
- `/api/meta` reports the actual backend and accepted revision. `/api/inspect` reports layout/timeline, authored IDs, sampled capture elements and audio metadata; these are layout bounds rather than final pixel extents.
- `/api/audio` serves `Session::audio_wav`: one full float-stereo WAV in export-sized sample chunks, cached per accepted revision. A request with no Range header returns that body. A single `bytes` range returns 206 and a slice of the same cache. An unsatisfiable range returns 416. Malformed ranges return the full body.
- The server serves API routes only; there is no `/` page and no browser launch.
- Worker teardown closes its queue and joins the owner thread so native resources finish on that thread.

## Work Guidance

- Source errors must not destroy the accepted scene.
- Verify source/dependency reload, concurrent request ownership, real frame content and audio sample counts through the endpoints.
- Plan 10 phase 4 replaces this HTTP PNG transport with the host session editor protocol.

## Verification

- `cargo test --release -p director-cli --features vulkan --bin director-engine preview::tests`
- `cargo test --release -p director-cli --features vulkan --bin director-engine gpu_preview_worker -- --ignored` requires a live Vulkan device.

## Child DOX Index

- None.
