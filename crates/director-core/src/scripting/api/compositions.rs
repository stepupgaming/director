//! # Named Compositions API
//!
//! Remotion-style composition registry: scripts register named builders with
//! default props; hosts (CLI, tests) list and invoke them with overrides.
//!
//! ## Rhai surface
//! ```rhai
//! register_composition("intro", #{
//!     width: 1920, height: 1080, fps: 30, duration: 5.0,
//!     default_props: #{ title: "Hello" }
//! }, |props| {
//!     let movie = new_director(1920, 1080, 30);
//!     // ... use props ...
//!     movie
//! });
//! ```
//!
//! ## Host contract
//! 1. Create a [`CompositionRegistry`] and pass it to [`register`].
//! 2. Compile the script once, `eval_ast` as `Dynamic` (registration scripts
//!    often return unit), and **keep the AST alive**.
//! 3. Invoke builders with [`CompositionRegistry::build`] / [`build_entry`] —
//!    never `call_fn` by name (closure names are mangled; captures won't bind).
//!
//! Props merge is **shallow** (top-level keys only), matching Remotion
//! `inputProps` semantics.

use crate::scripting::types::MovieHandle;
use rhai::{Dynamic, Engine, EvalAltResult, FnPtr, Map, AST};
use serde_json::{json, Value as JsonValue};
use std::sync::{Arc, Mutex};
use tracing::warn;

/// One registered composition: metadata + builder closure.
#[derive(Clone)]
pub struct CompositionEntry {
    pub name: String,
    pub width: i64,
    pub height: i64,
    /// Frame rate; stored as f64 so fractional rates (23.976) are allowed.
    pub fps: f64,
    /// Nominal duration in seconds (metadata for hosts; builder owns real timeline).
    pub duration: f64,
    pub default_props: Map,
    pub builder: FnPtr,
}

/// Engine-scoped registry of named compositions.
///
/// Cheap to clone (`Arc`); the host and the Rhai registration closure share it.
#[derive(Clone, Default)]
pub struct CompositionRegistry {
    inner: Arc<Mutex<Vec<CompositionEntry>>>,
}

impl CompositionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().map(|g| g.is_empty()).unwrap_or(true)
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn names(&self) -> Vec<String> {
        self.inner
            .lock()
            .map(|g| g.iter().map(|e| e.name.clone()).collect())
            .unwrap_or_default()
    }

    pub fn get(&self, name: &str) -> Option<CompositionEntry> {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.iter().find(|e| e.name == name).cloned())
    }

    /// When exactly one composition is registered, return it.
    pub fn sole(&self) -> Option<CompositionEntry> {
        let guard = self.inner.lock().ok()?;
        if guard.len() == 1 {
            guard.first().cloned()
        } else {
            None
        }
    }

    pub fn entries(&self) -> Vec<CompositionEntry> {
        self.inner.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// JSON array: `[{name, width, height, fps, duration, default_props}, ...]`.
    pub fn to_json(&self) -> JsonValue {
        let entries = self.entries();
        let arr: Vec<JsonValue> = entries
            .iter()
            .map(|e| {
                json!({
                    "name": e.name,
                    "width": e.width,
                    "height": e.height,
                    "fps": e.fps,
                    "duration": e.duration,
                    "default_props": dynamic_to_json(&Dynamic::from(e.default_props.clone())),
                })
            })
            .collect();
        JsonValue::Array(arr)
    }

    pub fn to_json_string_pretty(&self) -> String {
        serde_json::to_string_pretty(&self.to_json()).unwrap_or_else(|_| "[]".into())
    }

    /// Resolve which composition to build.
    ///
    /// - `Some(name)` → that entry or error.
    /// - `None` + exactly one registered → that entry.
    /// - `None` + multiple → error listing names.
    /// - empty registry → `None` (caller should use legacy eval return).
    pub fn resolve_entry(&self, name: Option<&str>) -> Result<Option<CompositionEntry>, String> {
        if self.is_empty() {
            return Ok(None);
        }
        match name {
            Some(n) => self
                .get(n)
                .map(Some)
                .ok_or_else(|| {
                    format!(
                        "composition '{}' not found; registered: {}",
                        n,
                        self.names().join(", ")
                    )
                }),
            None => {
                if let Some(sole) = self.sole() {
                    Ok(Some(sole))
                } else {
                    Err(format!(
                        "multiple compositions registered ({}); pass --composition <name>",
                        self.names().join(", ")
                    ))
                }
            }
        }
    }

    /// Build a composition with optional shallow prop overrides.
    pub fn build(
        &self,
        engine: &Engine,
        ast: &AST,
        name: Option<&str>,
        override_props: Option<&Map>,
    ) -> Result<MovieHandle, String> {
        let entry = self
            .resolve_entry(name)?
            .ok_or_else(|| "no compositions registered".to_string())?;
        build_entry(engine, ast, &entry, override_props)
    }

    fn push(&self, entry: CompositionEntry) -> Result<(), String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| "composition registry lock poisoned".to_string())?;
        if guard.iter().any(|e| e.name == entry.name) {
            return Err(format!("composition '{}' is already registered", entry.name));
        }
        guard.push(entry);
        Ok(())
    }
}

/// Invoke a composition builder with shallow-merged props.
pub fn build_entry(
    engine: &Engine,
    ast: &AST,
    entry: &CompositionEntry,
    override_props: Option<&Map>,
) -> Result<MovieHandle, String> {
    let props = match override_props {
        Some(over) => merged_props(&entry.default_props, over),
        None => entry.default_props.clone(),
    };

    let movie = entry
        .builder
        .call::<MovieHandle>(engine, ast, (props,))
        .map_err(|e| format!("composition '{}' builder failed: {}", entry.name, e))?;

    warn_metadata_mismatch(entry, &movie);
    Ok(movie)
}

/// Shallow merge: override keys replace defaults; nested maps are not deep-merged.
pub fn merged_props(defaults: &Map, overrides: &Map) -> Map {
    let mut out = defaults.clone();
    for (k, v) in overrides.iter() {
        out.insert(k.clone(), v.clone());
    }
    out
}

/// Parse a JSON object (string) into a Rhai `Map` for prop overrides.
pub fn props_from_json_str(s: &str) -> Result<Map, String> {
    let value: JsonValue =
        serde_json::from_str(s).map_err(|e| format!("invalid props JSON: {}", e))?;
    props_from_json_value(&value)
}

pub fn props_from_json_value(value: &JsonValue) -> Result<Map, String> {
    match value {
        JsonValue::Object(obj) => {
            let mut map = Map::new();
            for (k, v) in obj {
                map.insert(k.clone().into(), json_value_to_dynamic(v));
            }
            Ok(map)
        }
        JsonValue::Null => Ok(Map::new()),
        _ => Err("props JSON must be an object".into()),
    }
}

/// Load props from a file path or inline JSON object string.
pub fn props_from_file_or_inline(spec: &str) -> Result<Map, String> {
    let trimmed = spec.trim();
    if trimmed.starts_with('{') {
        return props_from_json_str(trimmed);
    }
    let text = std::fs::read_to_string(trimmed)
        .map_err(|e| format!("failed to read props file '{}': {}", trimmed, e))?;
    props_from_json_str(&text)
}

fn warn_metadata_mismatch(entry: &CompositionEntry, movie: &MovieHandle) {
    let Ok(d) = movie.director.lock() else {
        return;
    };
    if d.width as i64 != entry.width || d.height as i64 != entry.height {
        warn!(
            "composition '{}': built movie size {}x{} != declared {}x{}",
            entry.name, d.width, d.height, entry.width, entry.height
        );
    }
    let fps_delta = (d.fps - entry.fps).abs();
    if fps_delta > 0.01 {
        warn!(
            "composition '{}': built movie fps {} != declared {}",
            entry.name, d.fps, entry.fps
        );
    }
}

/// Extract composition metadata from a Rhai map.
pub fn parse_composition_meta(meta: &Map) -> Result<(i64, i64, f64, f64, Map), String> {
    let width = required_number(meta, "width")?.round() as i64;
    let height = required_number(meta, "height")?.round() as i64;
    if width <= 0 || height <= 0 {
        return Err("width and height must be positive".into());
    }
    let fps = required_number(meta, "fps")?;
    if !fps.is_finite() || fps <= 0.0 {
        return Err("fps must be a positive finite number".into());
    }
    let duration = required_number(meta, "duration")?;
    if !duration.is_finite() || duration < 0.0 {
        return Err("duration must be a non-negative finite number".into());
    }
    let default_props = match meta.get("default_props") {
        Some(v) => v
            .clone()
            .try_cast::<Map>()
            .ok_or_else(|| "default_props must be a map".to_string())?,
        None => Map::new(),
    };
    Ok((width, height, fps, duration, default_props))
}

fn required_number(meta: &Map, key: &str) -> Result<f64, String> {
    let v = meta
        .get(key)
        .ok_or_else(|| format!("composition metadata missing '{}'", key))?;
    dynamic_as_f64(v).ok_or_else(|| format!("composition metadata '{}' must be a number", key))
}

fn dynamic_as_f64(v: &Dynamic) -> Option<f64> {
    if let Some(i) = v.clone().try_cast::<i64>() {
        return Some(i as f64);
    }
    if let Some(f) = v.clone().try_cast::<f64>() {
        return Some(f);
    }
    if let Ok(i) = v.as_int() {
        return Some(i as f64);
    }
    if let Ok(f) = v.as_float() {
        return Some(f);
    }
    None
}

pub fn json_value_to_dynamic(value: &JsonValue) -> Dynamic {
    match value {
        JsonValue::Null => Dynamic::UNIT,
        JsonValue::Bool(b) => Dynamic::from_bool(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Dynamic::from_int(i)
            } else {
                Dynamic::from_float(n.as_f64().unwrap_or(0.0))
            }
        }
        JsonValue::String(s) => Dynamic::from(s.clone()),
        JsonValue::Array(arr) => {
            let mut rhai_arr = rhai::Array::new();
            for item in arr {
                rhai_arr.push(json_value_to_dynamic(item));
            }
            Dynamic::from(rhai_arr)
        }
        JsonValue::Object(obj) => {
            let mut map = Map::new();
            for (k, v) in obj {
                map.insert(k.clone().into(), json_value_to_dynamic(v));
            }
            Dynamic::from(map)
        }
    }
}

pub fn dynamic_to_json(value: &Dynamic) -> JsonValue {
    if value.is_unit() {
        return JsonValue::Null;
    }
    if let Some(b) = value.clone().try_cast::<bool>() {
        return JsonValue::Bool(b);
    }
    if let Ok(i) = value.as_int() {
        return json!(i);
    }
    if let Ok(f) = value.as_float() {
        return json!(f);
    }
    if let Some(s) = value.clone().try_cast::<String>() {
        return JsonValue::String(s);
    }
    if let Some(arr) = value.clone().try_cast::<rhai::Array>() {
        return JsonValue::Array(arr.iter().map(dynamic_to_json).collect());
    }
    if let Some(map) = value.clone().try_cast::<Map>() {
        let mut obj = serde_json::Map::new();
        for (k, v) in map.iter() {
            obj.insert(k.to_string(), dynamic_to_json(v));
        }
        return JsonValue::Object(obj);
    }
    JsonValue::String(value.to_string())
}

/// Register `register_composition` on the engine; entries go into `registry`.
pub fn register(engine: &mut Engine, registry: CompositionRegistry) {
    let reg = registry.clone();
    engine.register_fn(
        "register_composition",
        move |name: &str,
              meta: Map,
              builder: FnPtr|
              -> Result<(), Box<EvalAltResult>> {
            let (width, height, fps, duration, default_props) = parse_composition_meta(&meta)
                .map_err(Box::<EvalAltResult>::from)?;
            let entry = CompositionEntry {
                name: name.to_string(),
                width,
                height,
                fps,
                duration,
                default_props,
                builder,
            };
            reg.push(entry).map_err(Box::<EvalAltResult>::from)?;
            Ok(())
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::register_rhai_api_with_default_mode;
    use crate::video_wrapper::RenderMode;
    use crate::DefaultAssetLoader;
    use std::sync::Arc;

    fn engine_with_registry() -> (Engine, CompositionRegistry, AST) {
        let mut engine = Engine::new();
        let registry = CompositionRegistry::new();
        register_rhai_api_with_default_mode(
            &mut engine,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
        );
        register(&mut engine, registry.clone());
        // empty AST placeholder — tests that need real AST compile their own
        let ast = engine.compile("()").unwrap();
        (engine, registry, ast)
    }

    #[test]
    fn parse_meta_accepts_int_and_float() {
        let mut meta = Map::new();
        meta.insert("width".into(), Dynamic::from_int(1920));
        meta.insert("height".into(), Dynamic::from_float(1080.0));
        meta.insert("fps".into(), Dynamic::from_float(29.97));
        meta.insert("duration".into(), Dynamic::from_int(5));
        let (w, h, fps, dur, props) = parse_composition_meta(&meta).unwrap();
        assert_eq!(w, 1920);
        assert_eq!(h, 1080);
        assert!((fps - 29.97).abs() < 1e-9);
        assert_eq!(dur, 5.0);
        assert!(props.is_empty());
    }

    #[test]
    fn parse_meta_requires_fields() {
        let meta = Map::new();
        assert!(parse_composition_meta(&meta).unwrap_err().contains("width"));
    }

    #[test]
    fn parse_meta_rejects_bad_default_props() {
        let mut meta = Map::new();
        meta.insert("width".into(), Dynamic::from_int(100));
        meta.insert("height".into(), Dynamic::from_int(100));
        meta.insert("fps".into(), Dynamic::from_int(30));
        meta.insert("duration".into(), Dynamic::from_float(1.0));
        meta.insert("default_props".into(), Dynamic::from_int(1));
        assert!(parse_composition_meta(&meta)
            .unwrap_err()
            .contains("default_props"));
    }

    #[test]
    fn shallow_props_merge() {
        let mut defaults = Map::new();
        defaults.insert("title".into(), Dynamic::from("Hello"));
        defaults.insert("color".into(), Dynamic::from("#fff"));
        let mut over = Map::new();
        over.insert("title".into(), Dynamic::from("World"));
        let merged = merged_props(&defaults, &over);
        assert_eq!(
            merged.get("title").unwrap().clone().into_string().unwrap(),
            "World"
        );
        assert_eq!(
            merged.get("color").unwrap().clone().into_string().unwrap(),
            "#fff"
        );
    }

    #[test]
    fn props_from_json_object() {
        let map = props_from_json_str(r##"{"title":"X","n":3}"##).unwrap();
        assert_eq!(
            map.get("title").unwrap().clone().into_string().unwrap(),
            "X"
        );
        assert_eq!(map.get("n").unwrap().as_int().unwrap(), 3);
    }

    #[test]
    fn props_from_json_rejects_array() {
        assert!(props_from_json_str("[1,2]").is_err());
    }

    #[test]
    fn duplicate_name_errors() {
        let (engine, registry, _ast) = engine_with_registry();
        let script = r##"
            register_composition("intro", #{
                width: 100, height: 100, fps: 30, duration: 1.0
            }, |props| { new_director(100, 100, 30) });
            register_composition("intro", #{
                width: 100, height: 100, fps: 30, duration: 1.0
            }, |props| { new_director(100, 100, 30) });
        "##;
        let ast = engine.compile(script).unwrap();
        let err = engine.eval_ast::<Dynamic>(&ast).unwrap_err();
        assert!(
            err.to_string().contains("already registered"),
            "err={}",
            err
        );
        let _ = registry;
    }

    #[test]
    fn register_and_build_via_fnptr() {
        let mut engine = Engine::new();
        let registry = CompositionRegistry::new();
        register_rhai_api_with_default_mode(
            &mut engine,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
        );
        register(&mut engine, registry.clone());

        let script = r##"
            register_composition("intro", #{
                width: 320, height: 240, fps: 24, duration: 2.0,
                default_props: #{ title: "Default" }
            }, |props| {
                let movie = new_director(320, 240, 24);
                let scene = movie.add_scene(2.0);
                let root = scene.add_box(#{
                    width: "100%", height: "100%",
                    justify_content: "center", align_items: "center",
                    bg_color: "#000000"
                });
                let title = props["title"];
                root.add_text(#{ content: title, size: 24.0, color: "#ffffff" });
                movie
            });
            register_composition("outro", #{
                width: 320, height: 240, fps: 24, duration: 1.0
            }, |props| {
                new_director(320, 240, 24)
            });
        "##;

        let ast = engine.compile(script).expect("compile");
        engine.eval_ast::<Dynamic>(&ast).expect("eval register");

        assert_eq!(registry.len(), 2);
        assert_eq!(registry.names(), vec!["intro", "outro"]);

        let json = registry.to_json();
        assert_eq!(json.as_array().unwrap().len(), 2);
        assert_eq!(json[0]["name"], "intro");
        assert_eq!(json[0]["default_props"]["title"], "Default");

        let mut over = Map::new();
        over.insert("title".into(), Dynamic::from("Override"));
        let movie = registry
            .build(&engine, &ast, Some("intro"), Some(&over))
            .expect("build intro");
        let d = movie.director.lock().unwrap();
        assert_eq!(d.width, 320);
        assert_eq!(d.height, 240);
        assert!((d.fps - 24.0).abs() < 1e-9);

        // unnamed with multiple → error
        let err = match registry.build(&engine, &ast, None, None) {
            Ok(_) => panic!("expected multiple-compositions error"),
            Err(e) => e,
        };
        assert!(err.contains("multiple compositions"));

        // sole after we only had one would work; outro by name works
        let _ = registry
            .build(&engine, &ast, Some("outro"), None)
            .expect("build outro");
    }

    #[test]
    fn resolve_sole_when_one_registered() {
        let mut engine = Engine::new();
        let registry = CompositionRegistry::new();
        register_rhai_api_with_default_mode(
            &mut engine,
            Arc::new(DefaultAssetLoader),
            RenderMode::Preview,
        );
        register(&mut engine, registry.clone());

        let script = r##"
            register_composition("only", #{
                width: 100, height: 100, fps: 30, duration: 1.0
            }, |props| { new_director(100, 100, 30) });
        "##;
        let ast = engine.compile(script).unwrap();
        engine.eval_ast::<Dynamic>(&ast).unwrap();

        let movie = registry.build(&engine, &ast, None, None).unwrap();
        assert_eq!(movie.director.lock().unwrap().width, 100);
    }

    #[test]
    fn to_json_and_dynamic_roundtrip_scalars() {
        let v = json_value_to_dynamic(&json!({"a": 1, "b": true, "c": [1, "x"]}));
        let back = dynamic_to_json(&v);
        assert_eq!(back["a"], 1);
        assert_eq!(back["b"], true);
        assert_eq!(back["c"][1], "x");
    }
}
