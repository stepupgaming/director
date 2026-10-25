# Plan 05 — Single Scene Model

> **Status:** Done (2026-07-19)  
> **Priority:** P1  
> **Depends on:** 02 (stable ids), 01 (tracks-only timeline)  
> **Blocks:** honest project I/O; stopping schema/Rhai drift

## Problem

Three (plus) representations describe overlapping truth:

1. **Live runtime** — `Director` + `SceneGraph` + `Element` trait objects  
2. **Project JSON** — `project.rs` save/load (improved kind coverage)  
3. **director-schema / pipeline** — `MovieRequest` → `load_movie` (historically drifted)  
4. **Rhai authoring** — imperative builders  
5. **api_catalog** — partial single source for effects/transitions/helpers  

Drift symptoms: schema transitions subset, pipeline match arms hand-maintained, some kinds still placeholder on failure, catalog vs registry duplication risk.

This plan is **engine coherence**, not agent product features.

## Goals

- One **canonical serializable scene document** that round-trips the supported runtime.
- Rhai bindings and schema types **validated against** the same catalog (generate or exhaustively test).
- Explicit **supported kind matrix** with no silent Box fallback for “supported” kinds.
- Versioned document (`version` field) with migrations.

## Non-goals

- New `.rsx` language.
- Human visual editor format.
- Recipes/templates product layer.
- Perfect parity with every experimental node on day one — unknowns must be **explicit unsupported**, not quiet Boxes for supported tiers.

## Target model

### Document shape (logical)

```text
ProjectDocument vN
  meta: { width, height, fps: f64, … }
  tracks: [ Track { name, enabled, opacity, clips: [TrackClip] } ]
  nodes: [ NodeRecord { id, parent, children, transform, layout, element: Enum } ]
  assets: [ … references … ]
  audio: [ … ]
  entities: [ … ]   // optional section; already UUID-stable
  transitions: [ … ]
```

Runtime load: document → `Director`.  
Runtime save: `Director` → document (lossless for supported kinds).

### Kind policy matrix

Maintain a table in `core-contracts.md` / this plan:

| Kind | Round-trip | Notes |
|------|------------|-------|
| Box | required | |
| Text | required | spans + style |
| Image | required | path + fit |
| Video | required | path; frames reload from disk |
| Caption / SmartCaption | required | |
| Effect | required | all catalog effects |
| Composition | required | nested document |
| Lottie | required or explicit optional | embed JSON |
| SpectrumVisualizer | required or optional | |
| Shape/Vector | declare | |
| ActiveSpeakerCamera | declare | |
| WgpuNode | optional / non-portable | |

**Rule:** if kind is `required`, load failure is hard error, not Box placeholder.  
Placeholder only for `optional` with warning in manifest.

### Catalog as contract

`api_catalog.rs` remains SSOT for:

- effect names/params  
- transitions  
- lifecycle helpers worth reflecting  

CI tests:

- Every catalog effect has pipeline convert arm **or** project serialize arm  
- Every `EffectType` variant appears in catalog  
- Schema `build.rs` / generated enums stay in sync (existing direction)

Longer term: generate Rhai registration tables from catalog where mechanical.

### Pipeline vs project

Pick one inbound JSON story:

**Recommended:** Project document is canonical; `MovieRequest` either:

- becomes a thin subset that lowers to Project document, or  
- is frozen/deprecated for new features  

Do not add new features only to `MovieRequest` without project parity.

## Implementation steps

1. Write kind matrix from current `project.rs` reality (code audit).
2. Bump project `version`; add migration module.
3. Replace silent Box fallback for required kinds with `Result` errors.
4. Align `TrackClip` / fps / freeze fields with plan 01 in JSON.
5. Generational or stable ids in JSON (plan 02).
6. Schema: fps float; transition/effect enums from catalog.
7. Pixel round-trip test: build runtime → save → load → `director_render::render_frame` compare (plan 03 helper).
8. Deprecation note on pipeline-only paths in crate READMEs.

## Tests (acceptance)

| Test | Asserts |
|------|---------|
| `project_roundtrip_ obligatory_kinds` | each required kind field-level |
| `project_roundtrip_pixels_smoke` | ≥1 scene pixel match after reload |
| `required_kind_missing_data_errors` | no silent Box |
| `catalog_effect_parity` | enum ↔ catalog  
| `schema_fps_f64` | deser 23.976 |
| `legacy_timeline_json_migrates` | old files load to tracks |

## Files likely touched

- `crates/director-core/src/project.rs`
- `crates/director-core/src/api_catalog.rs`
- `crates/director-schema/**`
- `crates/director-pipeline/**`
- `crates/director-core/tests/project_roundtrip.rs`
- Docs: `core-contracts.md`, crate READMEs

## Risks

- Large JSON churn → version + migrate.  
- Pipeline users external to repo — document breakage.  
- Nested composition document size.

## Done when

- [x] Kind matrix published and enforced in tests  
- [x] Required kinds never become Box  
- [x] Catalog parity CI green  
- [x] fps/tracks/freeze fields match plan 01 in documents  
- [x] At least one pixel round-trip smoke test  

## Implementation notes (landed)

- `NodeKindTier` / `REQUIRED_NODE_KINDS` / `node_kind_tier` in `project.rs`.
- Load: Lottie / Effect / Composition rebuild failure → `anyhow` error (no Box).
- Caption kind fully round-tripped (`caption_props_to_json`).
- `EffectType::type_tag` + `api_catalog::project_effect_type_tags` for enum ↔ JSON tags.
- Schema `MovieRequest.fps` already `f64`; project JSON fps NTSC test added.
- Pipeline `MovieRequest` remains secondary; project document is canonical.
- Tests: `required_kind_missing_data_errors`, `node_kind_tier_matrix`,
  `catalog_effect_parity`, `schema_fps_f64_project_json`,
  `project_roundtrip_pixels_smoke`.
