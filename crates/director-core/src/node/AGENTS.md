# Node — effects and shader library

This directory owns the scene-graph nodes (`BoxNode`, `ImageNode`, `TextNode`,
`EffectNode`, etc.) and the SkSL shader/effect system that `EffectNode` applies.

## Purpose

- `effect.rs` defines `EffectType` (every visual effect) and `build_effect_filter`,
  the dispatch that threads image filters through a chain of effects.
- `effect_shaders.rs` holds one `build_*` function per runtime shader, each
  compiling an SkSL string once and caching it via `cached_runtime_shader`.
- `scripting/api/effects.rs` maps Rhai `apply_effect(name, map)` to `EffectType`.
- `api_catalog.rs` is the **single source of truth** for effect names, params,
  aliases, and JSON schema tags (`crates/AGENTS.md` enforces this).

## Ownership

`effect.rs` + `effect_shaders.rs` + `scripting/api/effects.rs` + `api_catalog.rs`
form one shader contract: adding a shader requires editing all of them or the
build/tests fail. Plan 10 phase 2 collapses this into one effect table.

## Local Contracts

### Caption layout

- `caption.rs` measures content height only. Anchor margins belong to Taffy insets, not the measured caption box.
- Measurement and rendering both compute paragraph width as `max(rect_width * layout_width_fraction, 20)` so narrow boxes preserve the same wrapping and height.
- Regression checks: `cargo test -p director-core --lib node::caption::tests`, including margin/inset separation and narrow measure/render equivalence.

### Text stroke pass

- `TextNode::stroke_paragraph` is an optional stroke-only paragraph that the fast render path lays out at the fill paragraph's width and paints with it. `stroke_over_fill` picks the order: `true` paints the stroke over the fill. Shadows cast from both. The per-glyph animator path paints the fill paragraph only.
- The HTML compiler sets both fields. Regression checks: `cargo test -p director-html --release --test text_stroke`.

### Adding a runtime shader touches 5 places

1. `effect_shaders.rs` — `pub(crate) fn build_<name>(...)` + SkSL string. Use the
   shared `cached_runtime_shader(shader_cache, "__<name>__", sksl, "<Name>", ...)`
   helper. Declare only the uniforms you set.
2. `effect.rs` —
   - `EffectType::<Name> { ... }` enum variant (scalars as `Animated<f32>`,
     colors as bare `Color`).
   - arm in `update()` ticking each `Animated` field.
   - arm in `build_effect_filter` calling `super::effect_shaders::build_<name>(...)`
     and unpacking `.current_value`.
   - a case in `shader_compile_tests` (e.g. `batch3_shader_effects_compile`) so
     the SkSL is asserted to compile.
3. `scripting/api/effects.rs` — `"<name>" => { ... }` arm in the map overload,
   reading params via `map.get("x").and_then(|v| v.as_float().ok()).unwrap_or(default)`
   and colors via `parse_hex_color`. Add aliases here.
4. `api_catalog.rs` — `e("<name>", "<schema_tag>", "desc", &[("param","f32"), ...],
   &["alias", ...])`. This entry **auto-generates** the `EffectConfig` serde enum
   in `director-schema` via `build.rs`; do not hand-edit that enum.
5. `examples/features/*.rhai` — at least one `apply_effect("name", #{...})` call
   so the new shader is exercised as executable reference material.

### SkSL conventions (load-bearing — keep consistent)

- Entry point: `half4 main(float2 pos)`. `pos` is in **pixels**;
  normalize with `float2 uv = pos / u_resolution;` when you need UVs.
- Input texture: `uniform shader image;` sampled with `image.eval(pos)`.
  The standard input name is `"image"` (set by `image_filters::runtime_shader`).
- Per-effect uniforms are `u_`-prefixed: `float` scalars and `float3` colors
  (`[r, g, b]`). Animated values are passed as `Animated<f32>` and read via
  `.current_value`; colors via `Color` (Copy) as `[c.r, c.g, c.b]`.
- **Resolution-relative sampling**: any neighbor offset must be derived from
  `1.0 / u_resolution` (e.g. `float2 px = 1.0 / u_resolution;`), never a fixed
  `1` pixel — otherwise edges/blur strength change with canvas size. See
  `edge_detect`, `sharpen`, `emboss`.
- **Luma**: use Rec.709 coefficients `dot(c.rgb, half3(0.2126, 0.7152, 0.0722))`
  everywhere for consistency.
- SkSL has **no dynamic loop bounds**: blur/hatch/smooth kernels use fixed
  constant loops (existing convention).
- The `u_time` and `u_resolution` uniforms are injected automatically only when
  your SkSL declares them; set exactly the uniforms the shader declares.
- Cinematic effects (`DirectionalBlur`, `FilmGrain`, `Vignette`,
  `ChromaticAberration`, `Pixelate`, `Sharpen`, `Glow`) live in `effect_shaders.rs`
  via `build_*` fns (refactored out of `effect.rs`) — keep them there, do not
  re-inline their SkSL.

## Work Guidance

- Color grades should preserve luminance (avoid `color * grade / luma` divides
  that blow out near-black pixels). See `orange_teal` vs the older `teal_orange`.
- Animated shaders must declare `u_time`; the runtime advances `current_time` each
  frame and threads it through `build_effect_filter`.
- After editing `api_catalog.rs`, the generated `EffectConfig` enum changes; run
  `cargo test -p director-schema` — its round-trip test asserts every catalog tag
  parses.

## Verification

- `cargo check --workspace --all-targets`
- `cargo test -p director-core --lib shader_compile_tests` — every shader SkSL
  compiles to an `ImageFilter`.
- `cargo test -p director-schema` — catalog↔schema round-trip (incl. new tags).
- `cargo clippy --workspace --all-targets -- -D warnings`

## Child DOX Index

None. This leaf contract owns caption layout and the shader/effect system.
