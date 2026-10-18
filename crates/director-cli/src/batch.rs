//! Batch planning, status lines, and the JSON report.
//! `tests/repair_batch.rs` includes this file, so it must stay dependency-free.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub struct PlannedItem {
    pub input: PathBuf,
    pub output: PathBuf,
}

/// `{stem}.mp4` in `output_dir`. A stem already used in this batch becomes
/// `{stem}-2.mp4`, then `{stem}-3.mp4`.
pub fn plan_batch(inputs: &[PathBuf], output_dir: &Path) -> Vec<PlannedItem> {
    let mut used = BTreeSet::new();
    let mut planned = Vec::with_capacity(inputs.len());
    for input in inputs {
        let stem = file_stem(input);
        let mut name = format!("{stem}.mp4");
        let mut suffix = 2u32;
        while !used.insert(name.clone()) {
            name = format!("{stem}-{suffix}.mp4");
            suffix = suffix.saturating_add(1);
        }
        planned.push(PlannedItem {
            input: input.clone(),
            output: output_dir.join(name),
        });
    }
    planned
}

fn file_stem(input: &Path) -> String {
    input
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "out".to_string())
}

pub struct BatchItem {
    pub name: String,
    pub output: String,
    pub error: Option<String>,
    pub exit_code: i32,
}

pub struct BatchReport {
    pub text: String,
    pub exit_code: i32,
}

/// One status line per item. Exit code is the highest item code.
/// A line with an error and code 0 is treated as 1.
pub fn batch_report(items: &[BatchItem]) -> BatchReport {
    let mut text = items
        .iter()
        .map(status_line)
        .collect::<Vec<_>>()
        .join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    BatchReport {
        text,
        exit_code: report_exit_code(items),
    }
}

pub struct BatchJsonReport {
    pub json: String,
    pub exit_code: i32,
}

/// One JSON object. Item `input` is [`BatchItem::name`]. Exit code matches [`batch_report`].
pub fn batch_report_json(items: &[BatchItem]) -> BatchJsonReport {
    let exit_code = report_exit_code(items);
    BatchJsonReport {
        json: format_batch_json(items, exit_code),
        exit_code,
    }
}

fn report_exit_code(items: &[BatchItem]) -> i32 {
    items.iter().fold(0, |highest, item| {
        let code = if item.error.is_some() {
            item.exit_code.max(1)
        } else {
            item.exit_code
        };
        highest.max(code)
    })
}

fn format_batch_json(items: &[BatchItem], exit_code: i32) -> String {
    let mut out = String::new();
    out.push_str("{\"exit_code\":");
    out.push_str(&exit_code.to_string());
    out.push_str(",\"items\":[");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"input\":");
        push_json_string(&mut out, &item.name);
        out.push_str(",\"output\":");
        push_json_string(&mut out, &item.output);
        out.push_str(",\"status\":");
        out.push_str(if item.error.is_some() {
            "\"fail\""
        } else {
            "\"ok\""
        });
        out.push_str(",\"error\":");
        match &item.error {
            Some(error) => push_json_string(&mut out, error),
            None => out.push_str("null"),
        }
        out.push_str(",\"exit_code\":");
        out.push_str(&item.exit_code.to_string());
        out.push('}');
    }
    out.push_str("]}");
    out
}

fn push_json_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn status_line(item: &BatchItem) -> String {
    match &item.error {
        None => format!(
            "ok {} {}",
            single_line(&item.name),
            single_line(&item.output)
        ),
        Some(error) => format!("fail {} {}", single_line(&item.name), single_line(error)),
    }
}

fn single_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}
