# Purpose

- Hold the GSAP page document and the pipe protocol shared by `director-html` and the `director-recorder` helper. It links no Skia, so the helper can link a JavaScript engine beside it.

## Ownership

- `src/doc.rs` owns `Doc<P: Placement>`: the scraper tree page scripts mutate, node ids, inline-style bookkeeping, probe reuse and the tree natives (`kind`, `insert`, `select`, `markup` and the rest). `Placement` is what a probe placed per box: the helper's `Spot` (an arena index plus `Placed`) or the compiler's full `Probed`. `compact` and `declarations` live here too.
- `src/wire.rs` owns `Tree`, the exact wire form of a scraper `Html`: nodes in ego_tree arena order with child index lists, detached orphans included, so `load` rebuilds the same arena and NodeIds.
- `src/protocol.rs` owns the framed messages: `Load` and `Span` start a run, the helper sends `Ask`s (probe, computed style, matrix, path, media query, beats, `Done`) and the compiler sends `Answer`s. A frame is a little-endian u32 length and a postcard body, so floats and NaN cross exactly.

## Local Contracts

- No dependency here may link Skia or `director-core`.
- `Doc::save` and `Doc::load` carry ids, generation, layout, paint and undo state, the last probe and `playing`, so the compiler's `collect` reuses the run's last probe exactly as one in-process run would.
- Attribute order survives the pipe: scraper sorts attributes on element creation and `string_cache` atoms compare by text.
- `Attr::attr_text` compares attribute names as text. `Element::attr` interns its argument, and a name outside html5ever's static atoms takes string_cache's process-wide lock.

## Work Guidance

## Verification

- `cargo test -p director-dom --release`

## Child DOX Index

- None.
