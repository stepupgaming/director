//! Canvas-edge findings for a loaded composition.
//! `--safe-zone` adds a warning when text, caption, or smart_caption enters that inset.

use crate::input::Input;
use clap::{Parser, ValueEnum};
use std::fmt;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum FailOn {
    Error,
    Warning,
    Never,
}

impl fmt::Display for FailOn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FailOn::Error => "error",
            FailOn::Warning => "warning",
            FailOn::Never => "never",
        })
    }
}

#[derive(Parser, Debug)]
pub struct CheckArgs {
    #[command(flatten)]
    pub input: Input,
    /// Seconds between layout samples
    #[arg(long, default_value_t = 0.25)]
    pub every: f64,
    #[arg(long, value_enum, default_value_t = FailOn::Error)]
    pub fail_on: FailOn,
    /// Warn when text, caption, or smart_caption enters this platform inset.
    #[arg(long)]
    pub safe_zone: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub time: f64,
    pub node_id: u64,
    pub kind: &'static str,
    pub severity: &'static str,
    pub detail: String,
}

pub fn sample_times(duration: f64, every: f64) -> Vec<f64> {
    if every <= 0.0 || !every.is_finite() || !duration.is_finite() || duration <= 0.0 {
        return vec![0.0];
    }
    let mut step = every;
    while (duration / step).ceil() as usize + 2 > 240 && step < duration {
        step *= 2.0;
    }
    let mut times = Vec::new();
    let mut t = 0.0;
    while t < duration && times.len() < 240 {
        times.push(t);
        t += step;
    }
    let end = duration * 0.999;
    let already = times.iter().any(|sample| (sample - end).abs() <= 1e-6);
    if !already {
        if times.len() < 240 {
            times.push(end);
        } else if let Some(last) = times.last_mut() {
            if *last < end {
                *last = end;
            }
        }
    }
    if times.is_empty() {
        times.push(0.0);
    }
    times
}

pub fn canvas_findings(
    nodes: &[(u64, f32, f32, f32, f32)],
    width: f32,
    height: f32,
    time: f64,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (id, x, y, w, h) in nodes {
        if *w <= 0.0 || *h <= 0.0 || !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite()
        {
            continue;
        }
        let intersects = *x < width && *y < height && x + w > 0.0 && y + h > 0.0;
        let inside = *x >= 0.0 && *y >= 0.0 && x + w <= width && y + h <= height;
        if inside {
            continue;
        }
        let (kind, severity) = if intersects {
            ("clipped", "warning")
        } else {
            ("off_canvas", "error")
        };
        findings.push(Finding {
            time,
            node_id: *id,
            kind,
            severity,
            detail: format!("x={x} y={y} w={w} h={h}"),
        });
    }
    findings
}

pub fn safe_zone_findings(
    nodes: &[(u64, &str, f32, f32, f32, f32)],
    zone: &director_core::types::SafeZone,
    width: f32,
    height: f32,
    time: f64,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (id, kind, x, y, w, h) in nodes {
        if !matches!(*kind, "text" | "caption" | "smart_caption") {
            continue;
        }
        if *w <= 0.0 || *h <= 0.0 || !x.is_finite() || !y.is_finite() || !w.is_finite() || !h.is_finite()
        {
            continue;
        }
        let right = x + w;
        let bottom = y + h;
        let outside = *x < zone.left
            || *y < zone.top
            || right > width - zone.right
            || bottom > height - zone.bottom;
        if !outside {
            continue;
        }
        findings.push(Finding {
            time,
            node_id: *id,
            kind: "safe_zone",
            severity: "warning",
            detail: format!("{kind} x={x} y={y} w={w} h={h}"),
        });
    }
    findings
}

pub fn exit_code(findings: &[Finding], fail_on: FailOn) -> i32 {
    let has_error = findings.iter().any(|finding| finding.severity == "error");
    let has_warning = findings.iter().any(|finding| finding.severity == "warning");
    match fail_on {
        FailOn::Never => 0,
        FailOn::Warning if has_error || has_warning => 2,
        FailOn::Error if has_error => 2,
        FailOn::Warning | FailOn::Error => 0,
    }
}

pub fn run(args: CheckArgs) -> i32 {
    let mut session = match args.input.open() {
        Ok(session) => session,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let duration = session.duration();
    let mut findings = session
        .missing_fonts()
        .iter()
        .map(|font| Finding {
            time: 0.0,
            node_id: 0,
            kind: "missing_font",
            severity: "error",
            detail: font.clone(),
        })
        .collect::<Vec<_>>();
    let width = session.document().width as f32;
    let height = session.document().height as f32;
    let zone = match args.safe_zone.as_deref() {
        None => None,
        Some(name) => match director_core::node::caption::scaled_safe_zone(name, height) {
            Some(zone) => Some(zone),
            None => {
                eprintln!("unknown safe zone '{name}'");
                return 1;
            }
        },
    };
    for time in sample_times(duration, args.every) {
        let manifest = match session.manifest(time) {
            Ok(manifest) => manifest,
            Err(error) => {
                eprintln!("{error}");
                return 1;
            }
        };
        let canvas_nodes = manifest
            .nodes
            .iter()
            .map(|node| {
                (
                    node.id.index as u64,
                    node.bounds.x,
                    node.bounds.y,
                    node.bounds.w,
                    node.bounds.h,
                )
            })
            .collect::<Vec<_>>();
        findings.extend(canvas_findings(&canvas_nodes, width, height, time));
        if let Some(zone) = &zone {
            let zoned = manifest
                .nodes
                .iter()
                .map(|node| {
                    (
                        node.id.index as u64,
                        node.kind,
                        node.bounds.x,
                        node.bounds.y,
                        node.bounds.w,
                        node.bounds.h,
                    )
                })
                .collect::<Vec<_>>();
            findings.extend(safe_zone_findings(&zoned, zone, width, height, time));
        }
    }
    let code = exit_code(&findings, args.fail_on);
    let json_findings = findings
        .iter()
        .map(|finding| {
            serde_json::json!({
                "time": finding.time,
                "node_id": finding.node_id,
                "kind": finding.kind,
                "severity": finding.severity,
                "detail": finding.detail,
            })
        })
        .collect::<Vec<_>>();
    println!(
        "{}",
        serde_json::json!({
            "exit_code": code,
            "findings": json_findings,
        })
    );
    code
}

#[cfg(test)]
mod tests {
    use super::{canvas_findings, exit_code, sample_times, FailOn};

    #[test]
    fn agent_check_partial_overflow_is_a_warning() {
        let findings = canvas_findings(&[(1, -10.0, 0.0, 20.0, 10.0)], 64.0, 64.0, 0.0);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, "clipped");
        assert_eq!(findings[0].severity, "warning");
        assert_eq!(exit_code(&findings, FailOn::Error), 0);
    }

    #[test]
    fn agent_check_fully_outside_is_an_error() {
        let findings = canvas_findings(&[(7, -100.0, -100.0, 10.0, 10.0)], 64.0, 64.0, 0.0);
        assert_eq!(findings[0].kind, "off_canvas");
        assert_eq!(exit_code(&findings, FailOn::Error), 2);
        assert_eq!(exit_code(&findings, FailOn::Never), 0);
    }

    #[test]
    fn agent_check_samples_include_start_and_end() {
        let times = sample_times(1.0, 0.25);
        assert!(times.contains(&0.0));
        assert!(times.iter().any(|time| *time > 0.99));
        assert!(times.len() <= 240);
        assert_eq!(sample_times(1.0, f64::NAN), vec![0.0]);
        assert_eq!(sample_times(f64::INFINITY, 0.25), vec![0.0]);
    }

    #[test]
    fn agent_check_ignores_zero_and_non_finite_bounds() {
        let findings = canvas_findings(
            &[(1, 0.0, 0.0, 0.0, 10.0), (2, f32::NAN, 0.0, 10.0, 10.0)],
            64.0,
            64.0,
            0.0,
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn agent_check_safe_zone_warns_text_inside_the_inset() {
        let zone = director_core::node::caption::scaled_safe_zone("tiktok", 1920.0)
            .expect("tiktok");
        assert!((zone.top - 100.0).abs() < 0.01);
        let hit = super::safe_zone_findings(
            &[
                (1, "text", 40.0, 10.0, 100.0, 40.0),
                (2, "caption", 40.0, 10.0, 100.0, 40.0),
            ],
            &zone,
            1080.0,
            1920.0,
            0.0,
        );
        assert_eq!(hit.len(), 2);
        assert!(hit.iter().all(|finding| finding.kind == "safe_zone"));
        assert!(hit.iter().all(|finding| finding.severity == "warning"));
        let boxed = super::safe_zone_findings(
            &[(1, "box", 40.0, 10.0, 100.0, 40.0)],
            &zone,
            1080.0,
            1920.0,
            0.0,
        );
        assert!(boxed.is_empty());
        let inside = super::safe_zone_findings(
            &[(1, "text", 40.0, 120.0, 100.0, 40.0)],
            &zone,
            1080.0,
            1920.0,
            0.0,
        );
        assert!(inside.is_empty());
        let skipped = super::safe_zone_findings(
            &[
                (1, "text", 0.0, 0.0, 0.0, 10.0),
                (2, "smart_caption", f32::NAN, 0.0, 10.0, 10.0),
            ],
            &zone,
            1080.0,
            1920.0,
            0.0,
        );
        assert!(skipped.is_empty());
        assert!(director_core::node::caption::scaled_safe_zone("nope", 1920.0).is_none());
    }
}
