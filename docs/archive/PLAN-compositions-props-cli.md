# Director: Named Compositions + Typed Props + Subcommand CLI

> **ARCHIVED / DEFERRED (2026-07-19).** Valid product plan, but **not** current priority.  
> Resume only after core-hardening plans 01–03 (at least) land. See [`docs/plans/`](../plans/00-core-hardening-index.md).

## Context & decisions made

Director (Rust/Skia/Rhai video engine) has a strong engine but lacks Remotion's authoring/DX surface. Agreed direction:

- **Skip `.rsx`** — Rhai stays the authoring DSL; no new file format/parser. A lossless scene serialization can be a later declarative layer.
- **This plan's scope**: named compositions with props (Remotion's `<Composition>` + `inputProps` analog) and a proper subcommand CLI.
- Live preview server (Axum + minimal web player) is the agreed *next* milestone, not in this plan.

## Part 1 — Compositions registry (director-core)

New file `crates/director-core/src/scripting/api/compositions.rs`:

```rust
pub struct CompositionEntry {
    pub name: String,
    pub width: i64, pub height: i64, pub fps: i64,
    pub duration: f64,             // seconds
    pub default_props: rhai::Map,
    pub builder: rhai::FnPtr,      // Rhai closure |props| -> MovieHandle
}
#[derive(Clone, Default)]
pub struct CompositionRegistry(pub Arc<Mutex<Vec<CompositionEntry>>>);
```

Script-facing API:

```rhai
register_composition("intro", #{
    width: 1920, height: 1080, fps: 30, duration: 5.0,
    default_props: #{ title: "Hello" }
}, |props| { /* build */ movie });
```

- Registry is **engine-scoped, not thread-local**: host creates `CompositionRegistry` and the Rust closure captures a clone (same pattern lifecycle.rs uses for `loader`). New hook `scripting::register_composition_api(engine, registry)` so other hosts are untouched.
- Duplicate names error at registration. Metadata accepts INT or FLOAT dynamics.
- **Critical Rhai mechanic**: closures desugar into the AST — compile once (`engine.compile`), eval as `Dynamic` (`eval_ast`), keep the AST, and invoke builders via `FnPtr::call::<MovieHandle>(&engine, &ast, (props,))`. Never `call_fn` by name (mangled, captures won't bind). One eval registers all compositions; no re-eval per composition.
- Props: `merged_props(entry, override_json)` — `serde_json` → `rhai::serde::to_dynamic` → **shallow merge** over `default_props` (Remotion semantics). rhai's `metadata` feature (already enabled) transitively provides serde — no Cargo changes needed in core.
- `CompositionRegistry::to_json()` → `[{name, width, height, fps, duration, default_props}]`.
- After building, warn if built movie's w/h/fps mismatch the declared metadata.

## Part 2 — Subcommand CLI (crates/director-cli)

Restructure `main.rs` with clap derive + `args_conflicts_with_subcommands`:

- `render <SCRIPT> [--composition <name>] [--props <file-or-inline-json>] [-o OUT]` + existing flags (`--alpha --gpu --manifest --manifest-time --log-*`). Default output adds `_<composition>` suffix when a named composition is chosen without `-o`.
- `still <SCRIPT> (-t <secs> | --frame <N>) [--composition] [--props] -o out.png` — absorbs the `preview_frame` bin (keep a thin deprecated shim after grepping for references).
- `compositions <SCRIPT> [--json]` — lists registered compositions; nonzero exit + hint if none.
- **Backward compat**: bare `director-engine script.rhai out.mp4` synthesizes `Render` from legacy positionals. Edge case: a script literally named `render.rhai` needs the explicit `render render.rhai` form — document it.

Shared flow (new `run.rs`): `eval_script → (Engine, AST, Dynamic, registry)` then `resolve_movie`:
1. Registry non-empty → named composition (or the sole one if unnamed; else error listing names) → merge props → `FnPtr::call`.
2. Registry empty → `result.try_cast::<MovieHandle>()` (legacy scripts still work; eval return type changes to `Dynamic` since registration scripts return unit).

Also: move `create_vulkan_context()` into `gpu.rs`; route JSON↔Map conversion through core helpers so the CLI never touches `rhai::serde` directly.

## Part 3 — Examples & tests

- New `examples/features/compositions.rhai` registering two compositions with `default_props`.
- **Update `tests/examples.rs`** (it currently hard-requires every example to eval to `MovieHandle`): eval as `Dynamic`; pass if result casts to `MovieHandle` OR the registry is non-empty — and in that case invoke each builder with default props and assert it returns a `MovieHandle`.
- Unit tests in `compositions.rs`: metadata extraction, duplicate-name error, props merge, FnPtr invocation via compiled AST, malformed metadata.
- CLI smoke: `compositions --json`, `still -t 1 --composition intro`, `render --composition intro --props '{"title":"X"}'`, legacy invocation.

## Step order

1. Core: `compositions.rs` + exports + unit tests.
2. Example script + examples-test harness update.
3. CLI: subcommand restructure + eval/resolve helpers + legacy fallback.
4. `still` absorption + `preview_frame` shim.
5. Docs: CLI README, examples/AGENTS.md.

## Pitfalls

- FnPtr closures need the originating AST kept alive; call via `FnPtr::call`.
- Eval return type must become `Dynamic`.
- clap positional/subcommand ambiguity handled by `args_conflicts_with_subcommands`.
- Shallow props merge only (document).
