//! # Render Manifest API
//!
//! Rhai bindings for building a render manifest and running the
//! verification/repair pipeline from scripts.
//!
//! ## Responsibilities
//! - `build_render_manifest(movie, sample_time)` — build a manifest handle.
//! - `manifest.verify()` — emit machine-actionable verification issues.
//! - `manifest.repair()` — recompile affected regions and re-solve smart captions.
//!
//! ## Verification config map keys (for `verify(config)` / `repair(config)`)
//! - `safe_zone_preset`: platform preset name, e.g. `"tiktok"`, `"youtube_shorts"`.
//! - `static_stretch_threshold`: minimum motion energy before a stretch is flagged.
//! - `static_stretch_window`: duration in seconds over which to measure motion.
//! - `caption_face_overlap_threshold`: minimum overlap area (px²) to flag a caption-over-face issue.

use crate::render_manifest::{
    RepairResult, VerificationConfig, VerificationIssue,
    VerificationIssueType, build_render_manifest,
};
use crate::scripting::types::{MovieHandle, RenderManifestHandle};
use rhai::{Array, Engine, Map};

fn parse_verification_config(map: &Map) -> VerificationConfig {
    let mut config = VerificationConfig::default();
    if let Some(v) = map.get("safe_zone_preset").and_then(|v| v.clone().into_string().ok()) {
        config.safe_zone_preset = v;
    }
    if let Some(v) = map.get("static_stretch_threshold").and_then(|v| v.as_float().ok()) {
        config.static_stretch_threshold = v as f32;
    }
    if let Some(v) = map.get("static_stretch_window").and_then(|v| v.as_float().ok()) {
        config.static_stretch_window = v;
    }
    if let Some(v) = map.get("caption_face_overlap_threshold").and_then(|v| v.as_float().ok()) {
        config.caption_face_overlap_threshold = v as f32;
    }
    config
}

/// Register render-manifest-related Rhai functions.
pub fn register(engine: &mut Engine) {
    engine.register_type_with_name::<RenderManifestHandle>("RenderManifest");

    engine.register_fn(
        "build_render_manifest",
        |movie: &mut MovieHandle, sample_time: f64|
         -> Result<RenderManifestHandle, Box<rhai::EvalAltResult>> {
            let mut d = movie.lock_director()?;
            let manifest = build_render_manifest(&mut d, sample_time);
            Ok(RenderManifestHandle {
                director: movie.director.clone(),
                manifest: std::sync::Arc::new(std::sync::Mutex::new(manifest)),
            })
        },
    );

    engine.register_fn(
        "verify",
        |manifest: &mut RenderManifestHandle|
         -> Result<Array, Box<rhai::EvalAltResult>> {
            verify_with_config(manifest, VerificationConfig::default())
        },
    );

    engine.register_fn(
        "verify",
        |manifest: &mut RenderManifestHandle, config_map: Map|
         -> Result<Array, Box<rhai::EvalAltResult>> {
            verify_with_config(manifest, parse_verification_config(&config_map))
        },
    );

    engine.register_fn(
        "repair",
        |manifest: &mut RenderManifestHandle|
         -> Result<Map, Box<rhai::EvalAltResult>> {
            repair_with_config(manifest, VerificationConfig::default())
        },
    );

    engine.register_fn(
        "repair",
        |manifest: &mut RenderManifestHandle, config_map: Map|
         -> Result<Map, Box<rhai::EvalAltResult>> {
            repair_with_config(manifest, parse_verification_config(&config_map))
        },
    );
}

fn verify_with_config(
    manifest: &mut RenderManifestHandle,
    config: VerificationConfig,
) -> Result<Array, Box<rhai::EvalAltResult>> {
    let mut d = manifest.lock_director()?;
    let m = manifest
        .manifest
        .lock()
        .map_err(|_| "manifest lock poisoned")?;
    let issues = m.verify(&mut d, &config);
    drop(m);
    Ok(issues.into_iter().map(verification_issue_to_map).collect())
}

fn repair_with_config(
    manifest: &mut RenderManifestHandle,
    config: VerificationConfig,
) -> Result<Map, Box<rhai::EvalAltResult>> {
    let mut d = manifest.lock_director()?;
    let m = manifest
        .manifest
        .lock()
        .map_err(|_| "manifest lock poisoned")?;
    let issues = m.verify(&mut d, &config);
    let result = m.repair(&mut d, &issues);
    drop(m);
    Ok(repair_result_to_map(result))
}

fn issue_type_str(ty: VerificationIssueType) -> &'static str {
    match ty {
        VerificationIssueType::CaptionOverFace => "caption_over_face",
        VerificationIssueType::SafeZoneViolation => "safe_zone_violation",
        VerificationIssueType::StaticStretch => "static_stretch",
        VerificationIssueType::SubjectOutOfFrame => "subject_out_of_frame",
    }
}

fn verification_issue_to_map(issue: VerificationIssue) -> rhai::Dynamic {
    let mut map = Map::new();
    map.insert("issue_type".into(), issue_type_str(issue.issue_type).into());
    map.insert("severity".into(), issue.severity.as_str().into());
    map.insert("start_time".into(), issue.start_time.into());
    map.insert("end_time".into(), issue.end_time.into());
    map.insert(
        "node_ids".into(),
        issue
            .node_ids
            .into_iter()
            .map(|id| rhai::Dynamic::from_int(id as i64))
            .collect::<Array>()
            .into(),
    );
    map.insert("description".into(), issue.description.into());
    if let Some(fix) = issue.suggested_fix {
        map.insert("suggested_fix".into(), fix.into());
    }
    let mut region = Map::new();
    region.insert("x".into(), (issue.region.x as f64).into());
    region.insert("y".into(), (issue.region.y as f64).into());
    region.insert("w".into(), (issue.region.w as f64).into());
    region.insert("h".into(), (issue.region.h as f64).into());
    map.insert("region".into(), region.into());
    rhai::Dynamic::from_map(map)
}

fn repair_result_to_map(result: RepairResult) -> Map {
    let mut map = Map::new();
    map.insert(
        "affected_nodes".into(),
        result
            .affected_nodes
            .into_iter()
            .map(|id| rhai::Dynamic::from_int(id as i64))
            .collect::<Array>()
            .into(),
    );
    let regions: Array = result
        .affected_regions
        .into_iter()
        .map(|(start, end)| {
            let mut m = Map::new();
            m.insert("start_time".into(), start.into());
            m.insert("end_time".into(), end.into());
            rhai::Dynamic::from_map(m)
        })
        .collect();
    map.insert("affected_regions".into(), regions.into());
    map.insert(
        "queued_issue_count".into(),
        (result.queued_issue_count as i64).into(),
    );
    map
}
