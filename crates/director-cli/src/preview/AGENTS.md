# Purpose

- Serve the native preview API (frames, inspection, audio and reload) for editors and agents.

## Ownership

- `mod.rs` owns Axum routes, response contracts and backend acceptance tests.
- `worker.rs` owns the bounded job queue, accepted scene revisions, reload and audio caching.

## Local Contracts

- Create, use and drop the Director, render targets and Vulkan context on one native worker thread. Async handlers exchange owned results through the bounded queue; never move the GPU context into a Tokio task.
- Load inputs through `input.rs` with export media semantics. Render PNGs through `renderer.rs`, which paints with `director-render` and the export sampling clock.
- Before requests, compare input/native dependency modification time and length. A successful reload replaces the complete scene, resets render targets/audio and advances its revision. A failed reload retains the accepted scene and exposes `reload_error`; forced `/api/reload` returns the error.
- `/api/meta` reports the actual backend and accepted revision. `/api/inspect` reports layout/timeline, authored IDs, sampled capture elements and audio metadata; these are layout bounds rather than final pixel extents.
- `/api/audio` builds one full float-stereo WAV in export-sized sample chunks and caches it per accepted revision. A request with no Range header returns that body. A single `bytes` range returns 206 and a slice of the same cache. An unsatisfiable range returns 416. Malformed ranges return the full body.
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
