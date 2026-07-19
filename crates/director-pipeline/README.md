# director-pipeline

Asset pipeline utilities for Director Engine.

## Purpose

Provides tools for:
- Asset preprocessing
- Font bundling
- Resource optimization
- Declarative `MovieRequest` → `Director` loading (`load_movie`)

**Note:** The versioned project document in `director-core` (`save_project_json` / `load_project_json`) is the canonical scene I/O path. Prefer it for round-trips; treat `MovieRequest` as a secondary inbound format.

## Usage

```toml
[dependencies]
director-pipeline = "1.1"
```

---

*Part of the Director Engine monorepo.*
