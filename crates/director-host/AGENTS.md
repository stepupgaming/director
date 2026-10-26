# Purpose

- Own the host session API of plan 10 phase 4: one open project as a revisioned `director_scene::SceneDocument` with patch, outline, diff, inspect, the QA report, frame render and export. The CLI, an MCP server and a future Tauri editor link this crate.

## Ownership

- `src/load.rs` owns `Source` (an input path plus HTML compile settings, the Rhai composition and props) and `Loaded`: loading Rhai, HTML, capture JSON, portable native projects and versioned core projects into a live `Director`, and `Loaded::from_compiled` for compiled HTML, captures and native projects.
- `src/script.rs` owns Rhai evaluation with the composition registry (`eval_script`) and movie resolution (`resolve_movie`, `ResolveOpts`).
- `src/session.rs` owns `Session`, `ExportSettings`, the `Outline` types and `import_registry` (core kinds plus `director-html` kinds).
- `src/diff.rs` owns `diff` and the `Diff` types: the structural diff of two sessions' documents and the patch between them.
- `src/qa.rs` owns `Session::qa`, `QaOptions`, `QaReport`, `QaIssue`, `QaAudio` and `sample_times` (the layout sampling grid shared with the CLI `check`).
- `src/patch.rs` owns `PatchOp`, `NodeSelector`, `Patch`, selector resolution and op application.
- `src/error.rs` owns `HostError`, `ErrorCode` (with a JSON schema whose variant docs describe each code to agents) and the JSON error envelope.
- `tests/session.rs` covers open, outline, accepted and rejected patches, conflicts, removal, retiming, frame repaint, inspection, export progress and cancellation on `examples/html/native-motion.html`, plus dependency reload, the cached WAV, native project edits, diffs and QA scores on generated HTML.

## Local Contracts

- `Session::open` loads in export mode, exports the live `Director` to a scene document and holds it at revision 0 with the authored names (HTML `id`s) mapped to `NodeRef`s. `Session::from_document` opens a document directly; both require the document to import.
- `patch(base_revision, ops)` requires `base_revision` to equal the current revision (`conflict` otherwise). Ops apply in order to a copy; the copy must deserialize and import before it replaces the document. Any failure rejects the whole patch and keeps the revision. An empty patch keeps the revision. Errors from an op start with `op <index>:`.
- `set` replaces a JSON pointer inside one node record; `/id`, `/parent` and `/children` are fixed. `set_document` replaces a JSON pointer of the document; `/version` and `/nodes` are fixed. `remove` drops a node and its subtree and refuses a subtree holding a clip's scene root or a mask a remaining node uses. Names of removed nodes leave the name table.
- A selector is an authored name string or a `{"index","generation"}` node id. An unknown name or id is `not_found`.
- `render_frame` paints on one `FrameSession` kept warm until the next revision. `inspect` builds the render manifest on a `Director` imported from the current revision, with `revision`, `source_kind`, `sample_rate`, `element_ids` (the authored names), sampled capture elements, missing fonts and audio tracks.
- `use_gpu` (feature `vulkan`) gives the session a GPU context. `render_frame` then paints on a warm `director_render::TargetSession` on that context, and the session stays on the context's thread. `backend` reports `vulkan` or `cpu`.
- `reload` loads the input again as the next revision and drops unsaved project edits. A failure keeps the document and sets `reload_error` until a reload succeeds. `source_changed` compares the modification time and length of the input and its dependency paths with the last load or reload attempt.
- `edit_project` applies `director_html::ElementEdit`s by element id to the native project input and holds the compiled result as the next revision. Edits that do not compile fail with `invalid_patch` and leave the session unchanged. `save_project` writes the edited project atomically.
- `manifest` returns the render manifest at a time and `audio_mixer` the decoded tracks, both on the `Director` imported from the current revision. `missing_fonts` lists required bundled fonts the system lacks.
- `audio_wav` mixes the timeline into one float-stereo WAV in export-sized chunks on the export clock and caches it until the next revision.
- `export` paints the current revision through `director_render::Renderer::export_options` at `ExportSettings::scale` with an optional alpha override, on a CPU pool or on a supplied GPU context. Missing required fonts fail with `missing_font`. A set cancel flag fails with `cancelled` and leaves nothing at the path. Progress reports encoded frames on the writer thread.
- `diff(before, after)` matches nodes by authored name, then by node id, when kinds agree. It reports document changes outside `/nodes`, added, removed and moved nodes (another parent or sibling order), and node record changes outside `/id`, `/parent` and `/children`, each at the deepest JSON pointer where both sides share the shape. `patchable` holds when no node was added or moved and every match keeps its node id; `patch` (document sets, node sets by name or id, then removals of removed subtree roots) applied to `before` then yields `after`.
- `qa(options)` reports on the current revision: `score` is 100 minus 20 per error and 5 per warning, floored at 0, and `pass` holds with no errors. Layout samples follow `sample_times(duration, every)` (at most 240). errors (20 points each) are missing required fonts, nodes outside the canvas at every layout sample (reported at the topmost such node), clipped mix samples and non-finite audio; warnings (5 points each) are `text`, `caption` or `smart_caption` nodes crossing the canvas edge or inside the `safe_zone` inset for at least one second of samples, the manifest's caption-over-face and subject-out-of-frame detectors on a one-second grid, true peak above -1 dBTP, a silent mix and integrated loudness more than 2 LU from the target; info covers static stretches (painted frames 0.5 s apart with no visible change for 2 s or more) and mixes above 57600000 samples, which skip analysis. Issues merge per kind and node with `start`, `end`, `count`, the authored `name` and a `fix`, sorted errors first. `audio` holds the mix loudness when the timeline has tracks. A nonpositive or non-finite `every`, a non-finite `loudness_target` or an unknown safe-zone preset fail with `invalid_argument`.
- Times must be finite and nonnegative (`invalid_argument`). Every operation fails with a `HostError`; `envelope()` is `{"error":{"code","message"}}` with snake_case codes.

## Work Guidance

- Keep frontends out of the session: new authoring inputs load in `load.rs` and reach the session as a scene document.
- New agent and editor commands call `Session` methods and print `HostError::envelope` on failure.

## Verification

- `cargo test -p director-host --release` (export tests need FFmpeg on the path)
- `cargo clippy -p director-host --all-targets --release -- -D warnings`

## Child DOX Index

- None.
