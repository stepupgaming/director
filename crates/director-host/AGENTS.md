# Purpose

- Own the host session API of plan 10 phase 4: one open project as a revisioned `director_scene::SceneDocument` with patch, outline, inspect, frame render and export. The CLI, an MCP server and a future Tauri editor link this crate.

## Ownership

- `src/load.rs` owns `Source` (an input path plus HTML compile settings, the Rhai composition and props) and `Loaded`: loading Rhai, HTML, capture JSON, portable native projects and versioned core projects into a live `Director`, and `Loaded::from_compiled` for compiled HTML, captures and native projects.
- `src/script.rs` owns Rhai evaluation with the composition registry (`eval_script`) and movie resolution (`resolve_movie`, `ResolveOpts`).
- `src/session.rs` owns `Session`, `ExportSettings`, the `Outline` types and `import_registry` (core kinds plus `director-html` kinds).
- `src/patch.rs` owns `PatchOp`, `NodeSelector`, `Patch`, selector resolution and op application.
- `src/error.rs` owns `HostError`, `ErrorCode` and the JSON error envelope.
- `tests/session.rs` covers open, outline, accepted and rejected patches, conflicts, removal, retiming, frame repaint, inspection, export progress and cancellation on `examples/html/native-motion.html`, plus dependency reload, the cached WAV and native project edits on generated HTML.

## Local Contracts

- `Session::open` loads in export mode, exports the live `Director` to a scene document and holds it at revision 0 with the authored names (HTML `id`s) mapped to `NodeRef`s. `Session::from_document` opens a document directly; both require the document to import.
- `patch(base_revision, ops)` requires `base_revision` to equal the current revision (`conflict` otherwise). Ops apply in order to a copy; the copy must deserialize and import before it replaces the document. Any failure rejects the whole patch and keeps the revision. An empty patch keeps the revision. Errors from an op start with `op <index>:`.
- `set` replaces a JSON pointer inside one node record; `/id`, `/parent` and `/children` are fixed. `set_document` replaces a JSON pointer of the document; `/version` and `/nodes` are fixed. `remove` drops a node and its subtree and refuses a subtree holding a clip's scene root or a mask a remaining node uses. Names of removed nodes leave the name table.
- A selector is an authored name string or a `{"index","generation"}` node id. An unknown name or id is `not_found`.
- `render_frame` paints on one `FrameSession` kept warm until the next revision. `inspect` builds the render manifest on a `Director` imported from the current revision, with `revision`, `source_kind`, `sample_rate`, `element_ids` (the authored names), sampled capture elements, missing fonts and audio tracks.
- `use_gpu` (feature `vulkan`) gives the session a GPU context. `render_frame` then paints on a warm `director_render::TargetSession` on that context, and the session stays on the context's thread. `backend` reports `vulkan` or `cpu`.
- `reload` loads the input again as the next revision and drops unsaved project edits. A failure keeps the document and sets `reload_error` until a reload succeeds. `source_changed` compares the modification time and length of the input and its dependency paths with the last load or reload attempt.
- `edit_project` applies `director_html::ElementEdit`s by element id to the native project input and holds the compiled result as the next revision. Edits that do not compile fail with `invalid_patch` and leave the session unchanged. `save_project` writes the edited project atomically.
- `audio_wav` mixes the timeline into one float-stereo WAV in export-sized chunks on the export clock and caches it until the next revision.
- `export` paints the current revision through `director_render::Renderer::export_options` at `ExportSettings::scale` with an optional alpha override, on a CPU pool or on a supplied GPU context. Missing required fonts fail with `missing_font`. A set cancel flag fails with `cancelled` and leaves nothing at the path. Progress reports encoded frames on the writer thread.
- Times must be finite and nonnegative (`invalid_argument`). Every operation fails with a `HostError`; `envelope()` is `{"error":{"code","message"}}` with snake_case codes.

## Work Guidance

- Keep frontends out of the session: new authoring inputs load in `load.rs` and reach the session as a scene document.
- New agent and editor commands call `Session` methods and print `HostError::envelope` on failure.

## Verification

- `cargo test -p director-host --release` (export tests need FFmpeg on the path)
- `cargo clippy -p director-host --all-targets --release -- -D warnings`

## Child DOX Index

- None.
