# Purpose

- Serve native preview, inspection, audio and reload endpoints without requiring frontend work.

## Ownership

- `mod.rs` owns Axum routes, response contracts and backend acceptance tests.
- `worker.rs` owns the bounded job queue, accepted scene revisions, reload and audio caching.
- `player.html` is the existing optional client; its design and playback controls are outside this phase.

## Local Contracts

- Create, use and drop the Director, render targets and Vulkan context on one native worker thread. Async handlers exchange owned results through the bounded queue; never move the GPU context into a Tokio task.
- Load inputs through `input.rs` with export media semantics. Render PNGs through `renderer.rs` and shared core shutter sampling.
- Before requests, compare input/native dependency modification time and length. A successful reload replaces the complete scene, resets render targets/audio and advances its revision. A failed reload retains the accepted scene and exposes `reload_error`; forced `/api/reload` returns the error.
- `/api/meta` reports the actual backend and accepted revision. `/api/inspect` reports layout/timeline, authored IDs, sampled capture elements and audio metadata; these are layout bounds rather than final pixel extents.
- `/api/audio` builds a full float-stereo WAV in export-sized sample chunks and caches it per revision. Streaming transport and frontend synchronization controls remain separate work.
- Worker teardown closes its queue and joins the owner thread so native resources finish on that thread.

## Work Guidance

- Keep backend operations independent of the optional player. Source errors must not destroy the accepted scene.
- Verify source/dependency reload, concurrent request ownership, real frame content and audio sample counts through the endpoints.

## Verification

- `cargo test --release -p director-cli --features vulkan --bin director-engine preview::tests`
- `cargo test --release -p director-cli --features vulkan --bin director-engine gpu_preview_worker -- --ignored` requires a live Vulkan device.

## Child DOX Index

- None.
