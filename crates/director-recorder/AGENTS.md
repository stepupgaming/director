# Purpose

- Run a page's scripts for `director-html` in a separate process. `director-cli` ships it as the `director-recorder` binary, and the compiler spawns one per script run.

## Ownership

- `src/lib.rs` owns `serve`: read `Load` and `Span` from stdin, record on a 64 MB stack thread, write `Ask::Done`, and the `ask` round trip to the compiler on stdout and stdin.
- `src/natives.rs` owns the DOM natives over a `director_dom::Doc<Spot>`, written once against the `Args` trait so the JavaScript engine binding stays thin. Computed style, matrices, path geometry, media queries and beats ask the compiler, and computed answers cache per probe.
- `src/engine.rs` owns the engine binding (`v8` 152.2.0, the prebuilt rusty_v8 static library) and the run sequence: evaluate the scripts, check the GSAP major version, fire the load events, play, warm a mid-timeline start, seek each sample and count random draws.
- `src/dom.js` owns the browser globals over the natives.

## Local Contracts

- Never depend on Skia, `director-core`, `director-html` or any crate that links them. A JavaScript engine's prebuilt library clashes with Skia's at link time.
- Every native keeps the name and arity `dom.js` calls. A native error throws a JavaScript `Error`, a negative node index a `RangeError`, and an exception from a JavaScript conversion passes through unchanged.
- stdout carries only protocol frames. Diagnostics go to stderr.
- V8 starts once per process with `--stack-size=32768` to use the 64 MB run thread, and runs promise jobs explicitly after each evaluation. Natives are non-enumerable globals that dispatch on their index in `NATIVES`.
- rusty_v8 links the static C runtime, so linking the binary reports LNK4098 on Windows. The warning is expected.

## Work Guidance

## Verification

- `cargo test -p director-html --release --test integration gsap::` exercises it end to end after `cargo build --release -p director-cli`.

## Child DOX Index

- None.
