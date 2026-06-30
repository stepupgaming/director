# Purpose

- Own core Lottie animation logic, interpolation, expressions, path morphing, modifiers, and renderer-facing primitives.

## Ownership

- `src/` owns Lottie animation behavior.
- `src/expressions/` owns optional expression support.
- `tests/` owns behavior tests.
- `benches/` owns performance benchmarks.

## Local Contracts

- Data model compatibility with `lottie-data` must be preserved when parsing or animation behavior changes.
- Expression behavior is feature-gated through `expressions`.
- Performance-sensitive animation code should avoid unnecessary allocation in frame paths.

## Work Guidance

- Add focused tests for animation, bezier, path, dash, or expression changes.
- Use benchmarks when changing hot interpolation or rendering-prep paths.

## Verification

- `cargo test -p lottie-core`
- `cargo test -p lottie-core --features expressions` when expression code changes.
- `cargo bench -p lottie-core` for benchmark-relevant changes.

## Child DOX Index

- No child AGENTS.md files currently required.
