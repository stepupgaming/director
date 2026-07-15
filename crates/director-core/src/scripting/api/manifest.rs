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
    IssueSeverity, RepairResult, VerificationConfig, VerificationIssue,
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

    engine.register_fn(
        "repair",
        |manifest: &mut RenderManifestHandle, issues: Array|
         -> Result<Map, Box<rhai::EvalAltResult>> {
            repair_with_issues(manifest, issues, None)
        },
    );

    engine.register_fn(
        "repair",
        |manifest: &mut RenderManifestHandle, issues: Array, config_map: Map|
         -> Result<Map, Box<rhai::EvalAltResult>> {
            repair_with_issues(manifest, issues, Some(parse_verification_config(&config_map)))
        },
    );

    engine.register_fn(
        "to_json",
        |manifest: &mut RenderManifestHandle|
         -> Result<String, Box<rhai::EvalAltResult>> {
            let m = manifest
                .manifest
                .lock()
                .map_err(|_| "manifest lock poisoned")?;
            serde_json::to_string_pretty(&*m)
                .map_err(|e| format!("failed to serialize manifest: {}", e).into())
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

fn repair_with_issues(
    manifest: &mut RenderManifestHandle,
    issues: Array,
    config: Option<VerificationConfig>,
) -> Result<Map, Box<rhai::EvalAltResult>> {
    let parsed_issues: Vec<VerificationIssue> = issues
        .into_iter()
        .map(|issue| {
            let map = issue
                .try_cast::<Map>()
                .ok_or_else(|| "expected issue map".to_string())?;
            verification_issue_from_map(&map)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut d = manifest.lock_director()?;
    let m = manifest
        .manifest
        .lock()
        .map_err(|_| "manifest lock poisoned")?;

    // When a config is supplied, re-verify the manifest under that config and
    // only repair issues that are still detected. This lets callers pass a
    // subset of issues while still honoring custom thresholds. Note that
    // re-verification mutates the Director's update/layout state, consistent
    // with the behavior of `verify()`.
    let filtered: Vec<VerificationIssue> = if let Some(config) = config {
        let verified = m.verify(&mut d, &config);
        parsed_issues
            .into_iter()
            .filter(|issue| verified.iter().any(|v| issues_match(v, issue)))
            .collect()
    } else {
        parsed_issues
    };

    let result = m.repair(&mut d, &filtered);
    drop(m);
    Ok(repair_result_to_map(result))
}

fn issues_match(a: &VerificationIssue, b: &VerificationIssue) -> bool {
    a.issue_type == b.issue_type
        && a.start_time == b.start_time
        && a.end_time == b.end_time
        && a.node_ids == b.node_ids
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

fn issue_type_from_str(s: &str) -> Result<VerificationIssueType, Box<rhai::EvalAltResult>> {
    match s {
        "caption_over_face" => Ok(VerificationIssueType::CaptionOverFace),
        "safe_zone_violation" => Ok(VerificationIssueType::SafeZoneViolation),
        "static_stretch" => Ok(VerificationIssueType::StaticStretch),
        "subject_out_of_frame" => Ok(VerificationIssueType::SubjectOutOfFrame),
        _ => Err(format!("unknown issue type: {}", s).into()),
    }
}

fn severity_from_str(s: &str) -> Result<IssueSeverity, Box<rhai::EvalAltResult>> {
    match s {
        "error" => Ok(IssueSeverity::Error),
        "warning" => Ok(IssueSeverity::Warning),
        "info" => Ok(IssueSeverity::Info),
        _ => Err(format!("unknown issue severity: {}", s).into()),
    }
}

fn verification_issue_from_map(map: &Map) -> Result<VerificationIssue, Box<rhai::EvalAltResult>> {
    let issue_type_str = map
        .get("issue_type")
        .and_then(|v| v.clone().into_string().ok())
        .ok_or_else(|| "issue_type is required".to_string())?;
    let issue_type = issue_type_from_str(&issue_type_str)?;
    let severity_str = map
        .get("severity")
        .and_then(|v| v.clone().into_string().ok())
        .ok_or_else(|| "severity is required".to_string())?;
    let severity = severity_from_str(&severity_str)?;
    let start_time = map
        .get("start_time")
        .and_then(|v| v.as_float().ok())
        .ok_or_else(|| "start_time is required".to_string())?;
    let end_time = map
        .get("end_time")
        .and_then(|v| v.as_float().ok())
        .ok_or_else(|| "end_time is required".to_string())?;
    let node_ids = map
        .get("node_ids")
        .and_then(|v| v.clone().try_cast::<Array>())
        .map(|arr| {
            arr.iter()
                .map(|v| {
                    let id = v.as_int()?;
                    if id < 0 {
                        return Err(Box::<rhai::EvalAltResult>::from(format!(
                            "node_id must be non-negative, got {}",
                            id
                        )));
                    }
                    Ok(id as usize)
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .ok_or_else(|| "node_ids is required".to_string())??;
    let description = map
        .get("description")
        .and_then(|v| v.clone().into_string().ok())
        .ok_or_else(|| "description is required".to_string())?;
    let suggested_fix = map
        .get("suggested_fix")
        .and_then(|v| v.clone().into_string().ok());
    let region = map
        .get("region")
        .and_then(|v| v.clone().try_cast::<Map>())
        .map(|region_map| bounds_rect_from_map(&region_map))
        .ok_or_else(|| "region is required".to_string())??;

    Ok(VerificationIssue {
        issue_type,
        severity,
        start_time,
        end_time,
        node_ids,
        description,
        suggested_fix,
        region,
    })
}

fn bounds_rect_from_map(
    region_map: &Map,
) -> Result<crate::types::BoundsRect, Box<rhai::EvalAltResult>> {
    let x = region_map
        .get("x")
        .and_then(|v| v.as_float().ok())
        .ok_or_else(|| "region.x is required".to_string())? as f32;
    let y = region_map
        .get("y")
        .and_then(|v| v.as_float().ok())
        .ok_or_else(|| "region.y is required".to_string())? as f32;
    let w = region_map
        .get("w")
        .and_then(|v| v.as_float().ok())
        .ok_or_else(|| "region.w is required".to_string())? as f32;
    let h = region_map
        .get("h")
        .and_then(|v| v.as_float().ok())
        .ok_or_else(|| "region.h is required".to_string())? as f32;
    Ok(crate::types::BoundsRect { x, y, w, h })
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
