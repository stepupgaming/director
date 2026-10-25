# Purpose

- Own the host session API of plan 10 phase 4: one open project as a revisioned `director_scene::SceneDocument` with patch, outline, inspect, frame render and export. The CLI, an MCP server and a future Tauri editor link this crate.

## Ownership

- `src/load.rs` owns `Source` (an input path plus HTML compile settings, the Rhai composition and props) and `Loaded`: loading Rhai, HTML, capture JSON, portable native projects and versioned core projects into a live `Director`, and `Loaded::inspect`.
- `src/script.rs` owns Rhai evaluation with the composition registry (`eval_script`) and movie resolution (`resolve_movie`, `ResolveOpts`).
- `src/session.rs` owns `Session`, `ExportSettings`, the `Outline` types and `import_registry` (core kinds plus `director-html` kinds).
- `src/patch.rs` owns `PatchOp`, `NodeSelector`, `Patch`, selector resolution and op application.
- `src/error.rs` owns `HostError`, `ErrorCode` and the JSON error envelope.
- `tests/session.rs` covers open, outline, accepted and rejected patches, conflicts, removal, retiming, frame repaint, inspection, export progress and cancellation on `examples/html/native-motion.html`.

## Local Contracts

- `Session::open` loads in export mode, exports the live `Director` to a scene document and holds it at revision 0 with the authored names (HTML `id`s) mapped to `NodeRef`s. `Session::from_document` opens a document directly; both require the document to import.
- `patch(base_revision, ops)` requires `base_revision` to equal the current revision (`conflict` otherwise). Ops apply in order to a copy; the copy must deserialize and import before it replaces the document. Any failure rejects the whole patch and keeps the revision. An empty patch keeps the revision. Errors from an op start with `op <index>:`.
- `set` replaces a JSON pointer inside one node record; `/id`, `/parent` and `/children` are fixed. `set_document` replaces a JSON pointer of the document; `/version` and `/nodes` are fixed. `remove` drops a node and its subtree and refuses a subtree holding a clip's scene root or a mask a remaining node uses. Names of removed nodes leave the name table.
- A selector is an authored name string or a `{"index","generation"}` node id. An unknown name or id is `not_found`.
- `render_frame` paints on one `FrameSession` kept warm until the next accepted patch. `inspect` builds the render manifest on a `Director` imported from the current revision, plus names, sampled capture elements and missing fonts.
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
