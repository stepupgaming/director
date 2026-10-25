//! Seekable SVG SMIL for authored HTML.
//!
//! usvg drops `animate`, `animateTransform`, `animateMotion`, and `set`. This
//! module samples a bounded subset at the element's local time, patches the
//! serialized markup, and replaces the resolved tree.

use crate::compiler::{escape_xml, parse_svg};
use crate::CompileError;
use director_core::scene_export::{channel, ElementExport, ExportContext, ExportError};
use director_core::{node::VectorNode, Element, RenderError};
use director_scene::{html as doc, NodeBody};
use scraper::ElementRef;
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) struct SmilPlan {
    anims: Vec<SmilAnim>,
    targets: HashMap<String, u32>,
    next_target: u32,
}

impl SmilPlan {
    pub(crate) fn is_empty(&self) -> bool {
        self.anims.is_empty()
    }

    pub(crate) fn target_id(&self, path: &str) -> Option<u32> {
        self.targets.get(path).copied()
    }
}

#[derive(Clone, Debug)]
struct SmilAnim {
    target: u32,
    timing: Timing,
    effect: Effect,
}

#[derive(Clone, Debug)]
struct Timing {
    begin: f64,
    dur: f64,
    end: Option<f64>,
    repeat: Repeat,
    repeat_dur: Option<f64>,
    fill_freeze: bool,
}

#[derive(Clone, Copy, Debug)]
enum Repeat {
    Once,
    Count(f64),
    Forever,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CalcMode {
    Linear,
    Discrete,
    Paced,
}

#[derive(Clone, Debug)]
enum Effect {
    Attr {
        name: String,
        mode: CalcMode,
        values: Vec<SmilValue>,
        key_times: Option<Vec<f64>>,
        hold: bool,
    },
    Transform {
        kind: &'static str,
        mode: CalcMode,
        frames: Vec<Vec<f64>>,
        key_times: Option<Vec<f64>>,
    },
    Motion {
        points: Vec<(f32, f32)>,
        rotate: f32,
    },
}

#[derive(Clone, Debug)]
enum SmilValue {
    Number { value: f64, unit: String },
    Color { r: f32, g: f32, b: f32, a: f32 },
    Raw(String),
}

impl SmilValue {
    fn format(&self) -> String {
        match self {
            Self::Number { value, unit } => format!("{}{unit}", fmt_num(*value)),
            Self::Color { r, g, b, a } => fmt_color(*r, *g, *b, *a),
            Self::Raw(value) => value.clone(),
        }
    }
}

pub(crate) fn child_path(parent: &str, index: usize) -> String {
    if parent.is_empty() {
        index.to_string()
    } else {
        format!("{parent}/{index}")
    }
}

pub(crate) fn collect(svg: ElementRef<'_>) -> Result<SmilPlan, CompileError> {
    let mut plan = SmilPlan {
        anims: Vec::new(),
        targets: HashMap::new(),
        next_target: 1,
    };
    walk(svg, "", &mut plan)?;
    check_unique(&plan)?;
    Ok(plan)
}

fn walk(el: ElementRef<'_>, path: &str, plan: &mut SmilPlan) -> Result<(), CompileError> {
    if el.value().name().eq_ignore_ascii_case("foreignobject") {
        return Ok(());
    }
    let mut index = 0usize;
    for child in el.children() {
        let Some(child) = ElementRef::wrap(child) else {
            continue;
        };
        let child_path = child_path(path, index);
        index += 1;
        if smil_tag(child.value().name()) {
            if smil_tag(el.value().name()) {
                return smil_err("SMIL nested animation requires extraction");
            }
            let target = target_id(plan, path);
            plan.anims.push(parse_anim(child, target)?);
        } else {
            walk(child, &child_path, plan)?;
        }
    }
    Ok(())
}

fn target_id(plan: &mut SmilPlan, path: &str) -> u32 {
    if let Some(id) = plan.targets.get(path) {
        return *id;
    }
    let id = plan.next_target;
    plan.next_target += 1;
    plan.targets.insert(path.to_string(), id);
    id
}

fn smil_tag(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "animate" | "animatetransform" | "animatemotion" | "set"
    )
}

fn parse_anim(el: ElementRef<'_>, target: u32) -> Result<SmilAnim, CompileError> {
    if el.value().attrs().any(|(name, _)| {
        let name = name.to_ascii_lowercase();
        name == "href" || name.ends_with(":href")
    }) {
        return smil_err("SMIL target reference requires extraction");
    }
    for child in el.children().filter_map(ElementRef::wrap) {
        if child.value().name().eq_ignore_ascii_case("mpath") {
            return smil_err("SMIL mpath requires extraction");
        }
        if smil_tag(child.value().name()) {
            return smil_err("SMIL nested animation requires extraction");
        }
    }
    reject_unknown(el)?;
    let timing = parse_timing(el)?;
    let tag = el.value().name().to_ascii_lowercase();
    let effect = match tag.as_str() {
        "animate" => parse_attr(el, false)?,
        "set" => parse_attr(el, true)?,
        "animatetransform" => parse_transform(el)?,
        "animatemotion" => parse_motion(el)?,
        _ => return smil_err("SMIL attributeName is required"),
    };
    Ok(SmilAnim {
        target,
        timing,
        effect,
    })
}

fn reject_unknown(el: ElementRef<'_>) -> Result<(), CompileError> {
    let tag = el.value().name().to_ascii_lowercase();
    for (name, _value) in el.value().attrs() {
        let name = name.to_ascii_lowercase();
        if matches!(
            name.as_str(),
            "id" | "attributename"
                | "attributetype"
                | "from"
                | "to"
                | "by"
                | "values"
                | "begin"
                | "dur"
                | "end"
                | "repeatcount"
                | "repeatdur"
                | "fill"
                | "calcmode"
                | "keytimes"
                | "additive"
                | "accumulate"
                | "restart"
                | "class"
        ) {
            continue;
        }
        if tag == "animatetransform" && name == "type" {
            continue;
        }
        if tag == "animatemotion"
            && matches!(name.as_str(), "path" | "rotate" | "origin" | "keypoints")
        {
            if name == "keypoints" {
                return smil_err("SMIL keyPoints requires extraction");
            }
            continue;
        }
        if name == "keysplines" {
            return smil_err("SMIL keySplines requires extraction");
        }
        if name == "min" || name == "max" {
            return smil_err("SMIL min and max require extraction");
        }
        return Err(CompileError::Input(format!(
            "SMIL attribute '{name}' requires extraction"
        )));
    }
    if attr(el, "keysplines").is_some() {
        return smil_err("SMIL keySplines requires extraction");
    }
    Ok(())
}

fn parse_timing(el: ElementRef<'_>) -> Result<Timing, CompileError> {
    if let Some(raw) = attr(el, "attributetype") {
        if !matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "auto" | "xml" | "css"
        ) {
            return smil_err("SMIL attributeType requires auto, XML, or CSS");
        }
    }
    if let Some(raw) = attr(el, "additive") {
        if !raw.trim().eq_ignore_ascii_case("replace") {
            return smil_err("SMIL additive requires replace");
        }
    }
    if let Some(raw) = attr(el, "accumulate") {
        if !raw.trim().eq_ignore_ascii_case("none") {
            return smil_err("SMIL accumulate requires none");
        }
    }
    if let Some(raw) = attr(el, "restart") {
        if !raw.trim().eq_ignore_ascii_case("always") {
            return smil_err("SMIL restart requires always");
        }
    }
    let begin = match attr(el, "begin") {
        Some(raw) if raw.contains(';') => {
            return smil_err("SMIL begin list requires extraction");
        }
        Some(raw) => clock(raw).ok_or_else(|| {
            CompileError::Input("SMIL begin requires one clock value".into())
        })?,
        None => 0.0,
    };
    let dur = match attr(el, "dur") {
        Some(raw) => clock(raw).filter(|value| *value > 0.0).ok_or_else(|| {
            CompileError::Input("SMIL dur requires a positive clock value".into())
        })?,
        None => return smil_err("SMIL dur requires a positive clock value"),
    };
    let end = match attr(el, "end") {
        Some(raw) if raw.contains(';') => {
            return smil_err("SMIL begin list requires extraction");
        }
        Some(raw) => Some(clock(raw).ok_or_else(|| {
            CompileError::Input("SMIL end requires one clock value".into())
        })?),
        None => None,
    };
    let repeat = match attr(el, "repeatcount") {
        Some(raw) if raw.trim().eq_ignore_ascii_case("indefinite") => Repeat::Forever,
        Some(raw) => {
            let count: f64 = raw.trim().parse().map_err(|_| {
                CompileError::Input(
                    "SMIL repeatCount requires a positive number or indefinite".into(),
                )
            })?;
            if !count.is_finite() || count <= 0.0 {
                return smil_err("SMIL repeatCount requires a positive number or indefinite");
            }
            Repeat::Count(count)
        }
        None => Repeat::Once,
    };
    let repeat_dur = match attr(el, "repeatdur") {
        Some(raw) => Some(
            clock(raw)
                .filter(|value| *value > 0.0)
                .ok_or_else(|| {
                    CompileError::Input("SMIL dur requires a positive clock value".into())
                })?,
        ),
        None => None,
    };
    let fill_freeze = match attr(el, "fill") {
        Some(raw) if raw.trim().eq_ignore_ascii_case("freeze") => true,
        Some(raw) if raw.trim().eq_ignore_ascii_case("remove") => false,
        Some(_) => return smil_err("SMIL fill requires freeze or remove"),
        None => false,
    };
    Ok(Timing {
        begin,
        dur,
        end,
        repeat,
        repeat_dur,
        fill_freeze,
    })
}

fn parse_attr(el: ElementRef<'_>, set: bool) -> Result<Effect, CompileError> {
    let name = attr(el, "attributename")
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| CompileError::Input("SMIL attributeName is required".into()))?
        .to_string();
    if set && attr(el, "from").is_some() {
        return smil_err("SMIL set uses to");
    }
    let mode = if set {
        CalcMode::Discrete
    } else {
        parse_calc_mode(el, CalcMode::Linear)?
    };
    let key_times = attr(el, "keytimes").map(|raw| raw.to_string());
    if mode == CalcMode::Paced && key_times.is_some() {
        return smil_err("SMIL paced does not use keyTimes");
    }
    let values = if set {
        let to = attr(el, "to")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| CompileError::Input("SMIL to is required".into()))?;
        vec![parse_smil_value(to)]
    } else {
        resolve_values(el)?
    };
    if !set && matches!(mode, CalcMode::Linear | CalcMode::Paced) && !same_numeric_family(&values)
    {
        return smil_err(if mode == CalcMode::Paced {
            "SMIL paced requires numbers or colors"
        } else {
            "SMIL linear animation requires numbers or colors"
        });
    }
    let key_times = match key_times {
        Some(raw) => Some(parse_key_times(&raw, values.len(), mode)?),
        None => None,
    };
    Ok(Effect::Attr {
        name,
        mode,
        values,
        key_times,
        hold: set,
    })
}

fn parse_transform(el: ElementRef<'_>) -> Result<Effect, CompileError> {
    let attribute = attr(el, "attributename").unwrap_or("");
    if !attribute.trim().eq_ignore_ascii_case("transform") {
        return smil_err("SMIL animateTransform attributeName requires transform");
    }
    let kind = match attr(el, "type").map(|value| value.trim().to_ascii_lowercase()) {
        Some(ref value) if value == "translate" => "translate",
        Some(ref value) if value == "scale" => "scale",
        Some(ref value) if value == "rotate" => "rotate",
        Some(ref value) if value == "skewx" => "skewX",
        Some(ref value) if value == "skewy" => "skewY",
        _ => {
            return smil_err(
                "SMIL animateTransform type requires translate, scale, rotate, skewX, or skewY",
            )
        }
    };
    let mode = parse_calc_mode(el, CalcMode::Linear)?;
    if mode == CalcMode::Paced {
        return smil_err("SMIL paced requires numbers or colors");
    }
    if attr(el, "keytimes").is_some() && mode == CalcMode::Paced {
        return smil_err("SMIL paced does not use keyTimes");
    }
    let lists = resolve_values(el)?;
    let mut frames = Vec::with_capacity(lists.len());
    let mut width = None;
    for value in &lists {
        let text = match value {
            SmilValue::Raw(raw) => raw.clone(),
            SmilValue::Number { value, unit } if unit.is_empty() => fmt_num(*value),
            _ => return smil_err("SMIL animateTransform values require the same count"),
        };
        let nums = normalize_transform(kind, number_list(&text)?)?;
        if width.is_some_and(|width| nums.len() != width) {
            return smil_err("SMIL animateTransform values require the same count");
        }
        width = Some(nums.len());
        frames.push(nums);
    }
    let key_times = match attr(el, "keytimes") {
        Some(raw) => Some(parse_key_times(raw, frames.len(), mode)?),
        None => None,
    };
    Ok(Effect::Transform {
        kind,
        mode,
        frames,
        key_times,
    })
}

fn parse_motion(el: ElementRef<'_>) -> Result<Effect, CompileError> {
    let mode = parse_calc_mode(el, CalcMode::Paced)?;
    if mode != CalcMode::Paced {
        return smil_err("SMIL animateMotion requires paced motion");
    }
    if attr(el, "keytimes").is_some() || attr(el, "keypoints").is_some() {
        return smil_err("SMIL keyPoints requires extraction");
    }
    let rotate = match attr(el, "rotate") {
        None => 0.0,
        Some(raw) if raw.trim().eq_ignore_ascii_case("auto") => {
            return smil_err("SMIL animateMotion rotate auto requires extraction");
        }
        Some(raw) if raw.trim().eq_ignore_ascii_case("auto-reverse") => {
            return smil_err("SMIL animateMotion rotate auto requires extraction");
        }
        Some(raw) => clock_number(raw).ok_or_else(|| {
            CompileError::Input("SMIL animateMotion rotate auto requires extraction".into())
        })? as f32,
    };
    if let Some(raw) = attr(el, "origin") {
        if !raw.trim().eq_ignore_ascii_case("default") {
            return smil_err("SMIL attribute 'origin' requires extraction");
        }
    }
    let points = if let Some(path) = attr(el, "path") {
        parse_polyline(path)?
    } else {
        let values = resolve_values(el)?;
        let mut points = Vec::with_capacity(values.len());
        for value in values {
            let text = match value {
                SmilValue::Raw(raw) => raw,
                _ => return smil_err("SMIL animateMotion path requires a polyline"),
            };
            let nums = number_list(&text)?;
            if nums.len() != 2 {
                return smil_err("SMIL animateMotion path requires a polyline");
            }
            points.push((nums[0] as f32, nums[1] as f32));
        }
        if points.len() < 2 {
            return smil_err("SMIL animateMotion path requires a polyline");
        }
        points
    };
    Ok(Effect::Motion { points, rotate })
}

fn resolve_values(el: ElementRef<'_>) -> Result<Vec<SmilValue>, CompileError> {
    let has_values = attr(el, "values").is_some();
    let has_from = attr(el, "from").is_some();
    let has_to = attr(el, "to").is_some();
    let has_by = attr(el, "by").is_some();
    if has_values && (has_from || has_to || has_by) {
        return smil_err("SMIL values cannot combine with from, to, or by");
    }
    if has_to && has_by {
        return smil_err("SMIL to and by together require extraction");
    }
    if has_values {
        let mut values = Vec::new();
        for part in attr(el, "values").unwrap().split(';') {
            let part = part.trim();
            if part.is_empty() {
                return smil_err("SMIL values require a value");
            }
            values.push(parse_smil_value(part));
        }
        if values.is_empty() {
            return smil_err("SMIL values require a value");
        }
        return Ok(values);
    }
    let from = if let Some(from) = attr(el, "from") {
        from.trim().to_string()
    } else if let Some(base) = base_attr(el, attr(el, "attributename").unwrap_or("")) {
        base
    } else {
        return smil_err("SMIL from requires a base value");
    };
    let to = if let Some(to) = attr(el, "to") {
        to.trim().to_string()
    } else if let Some(by) = attr(el, "by") {
        add_by(&from, by.trim())?
    } else {
        return smil_err("SMIL to is required");
    };
    Ok(vec![parse_smil_value(&from), parse_smil_value(&to)])
}

fn base_attr(el: ElementRef<'_>, name: &str) -> Option<String> {
    let parent = el.parent().and_then(ElementRef::wrap)?;
    attr(parent, name).map(|value| value.trim().to_string())
}

fn add_by(from: &str, by: &str) -> Result<String, CompileError> {
    let from = parse_smil_value(from);
    let by = parse_smil_value(by);
    match (from, by) {
        (
            SmilValue::Number {
                value: left,
                unit: left_unit,
            },
            SmilValue::Number {
                value: right,
                unit: right_unit,
            },
        ) if left_unit == right_unit => Ok(format!("{}{left_unit}", fmt_num(left + right))),
        _ => smil_err("SMIL by requires a number"),
    }
}

fn parse_calc_mode(el: ElementRef<'_>, default: CalcMode) -> Result<CalcMode, CompileError> {
    let Some(raw) = attr(el, "calcmode") else {
        return Ok(default);
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "linear" => Ok(CalcMode::Linear),
        "discrete" => Ok(CalcMode::Discrete),
        "paced" => Ok(CalcMode::Paced),
        "spline" => smil_err("SMIL calcMode spline requires extraction"),
        _ => smil_err("SMIL calcMode requires linear, discrete, or paced"),
    }
}

fn parse_key_times(raw: &str, count: usize, mode: CalcMode) -> Result<Vec<f64>, CompileError> {
    let mut times = Vec::new();
    for part in raw.split(';') {
        let part = part.trim();
        let time: f64 = part.parse().map_err(|_| {
            CompileError::Input("SMIL keyTimes require one time per value".into())
        })?;
        if !time.is_finite() {
            return smil_err("SMIL keyTimes require one time per value");
        }
        times.push(time);
    }
    if times.len() != count || count == 0 {
        return smil_err("SMIL keyTimes require one time per value");
    }
    if (times[0] - 0.0).abs() > 1e-4 {
        return smil_err("SMIL keyTimes require a start at 0");
    }
    for pair in times.windows(2) {
        if pair[1] < pair[0] {
            return smil_err("SMIL keyTimes must increase");
        }
    }
    let last = *times.last().unwrap();
    if mode == CalcMode::Discrete {
        if last > 1.0 + 1e-4 {
            return smil_err("SMIL keyTimes require an end at 1");
        }
    } else if (last - 1.0).abs() > 1e-4 {
        return smil_err("SMIL keyTimes require an end at 1");
    }
    Ok(times)
}

fn same_numeric_family(values: &[SmilValue]) -> bool {
    if values.is_empty() {
        return false;
    }
    match &values[0] {
        SmilValue::Color { .. } => values.iter().all(|value| matches!(value, SmilValue::Color { .. })),
        SmilValue::Number { unit, .. } => values.iter().all(|value| {
            matches!(value, SmilValue::Number { unit: other, .. } if other == unit)
        }),
        SmilValue::Raw(_) => false,
    }
}

fn parse_smil_value(raw: &str) -> SmilValue {
    let raw = raw.trim();
    if let Some(color) = parse_color(raw) {
        return SmilValue::Color {
            r: color.0,
            g: color.1,
            b: color.2,
            a: color.3,
        };
    }
    if let Some((value, unit)) = parse_number(raw) {
        return SmilValue::Number { value, unit };
    }
    SmilValue::Raw(raw.to_string())
}

fn number_list(raw: &str) -> Result<Vec<f64>, CompileError> {
    let mut scan = Scan {
        bytes: raw.as_bytes(),
        index: 0,
    };
    let mut nums = Vec::new();
    while let Some(value) = scan.number() {
        nums.push(value);
    }
    scan.skip_sep();
    if scan.index != scan.bytes.len() || nums.is_empty() {
        return smil_err("SMIL values require a number");
    }
    Ok(nums)
}

fn normalize_transform(kind: &str, nums: Vec<f64>) -> Result<Vec<f64>, CompileError> {
    let ok = match kind {
        "translate" | "scale" => matches!(nums.len(), 1 | 2),
        "rotate" => matches!(nums.len(), 1 | 3),
        "skewX" | "skewY" => nums.len() == 1,
        _ => false,
    };
    if !ok {
        return smil_err("SMIL animateTransform values require the same count");
    }
    let mut nums = nums;
    if kind == "translate" && nums.len() == 1 {
        nums.push(0.0);
    }
    if kind == "scale" && nums.len() == 1 {
        nums.push(nums[0]);
    }
    Ok(nums)
}

fn parse_polyline(path: &str) -> Result<Vec<(f32, f32)>, CompileError> {
    let mut scan = Scan {
        bytes: path.as_bytes(),
        index: 0,
    };
    let mut points = Vec::new();
    let mut cx = 0.0f32;
    let mut cy = 0.0f32;
    let mut start = (0.0f32, 0.0f32);
    let mut cmd: Option<u8> = None;
    while scan.index < scan.bytes.len() {
        scan.skip_sep();
        if scan.index >= scan.bytes.len() {
            break;
        }
        let byte = scan.bytes[scan.index];
        if byte.is_ascii_alphabetic() {
            cmd = Some(byte);
            scan.index += 1;
            if matches!(byte, b'Z' | b'z') {
                if points.last().copied() != Some(start) {
                    points.push(start);
                }
                cx = start.0;
                cy = start.1;
                continue;
            }
        }
        let Some(cmd_byte) = cmd else {
            return smil_err("SMIL animateMotion path requires a polyline");
        };
        match cmd_byte {
            b'M' | b'm' => {
                let (x, y) = rel_pair(&mut scan, cmd_byte == b'm', cx, cy).ok_or_else(|| {
                    CompileError::Input("SMIL animateMotion path requires a polyline".into())
                })?;
                if points.is_empty() {
                    start = (x, y);
                }
                points.push((x, y));
                cx = x;
                cy = y;
                cmd = Some(if cmd_byte == b'M' { b'L' } else { b'l' });
            }
            b'L' | b'l' => {
                let (x, y) = rel_pair(&mut scan, cmd_byte == b'l', cx, cy).ok_or_else(|| {
                    CompileError::Input("SMIL animateMotion path requires a polyline".into())
                })?;
                points.push((x, y));
                cx = x;
                cy = y;
            }
            b'H' | b'h' => {
                let x = scan.number().ok_or_else(|| {
                    CompileError::Input("SMIL animateMotion path requires a polyline".into())
                })? as f32;
                let x = if cmd_byte == b'h' { cx + x } else { x };
                points.push((x, cy));
                cx = x;
            }
            b'V' | b'v' => {
                let y = scan.number().ok_or_else(|| {
                    CompileError::Input("SMIL animateMotion path requires a polyline".into())
                })? as f32;
                let y = if cmd_byte == b'v' { cy + y } else { y };
                points.push((cx, y));
                cy = y;
            }
            _ => return smil_err("SMIL animateMotion path requires a polyline"),
        }
    }
    if points.len() < 2 {
        return smil_err("SMIL animateMotion path requires a polyline");
    }
    Ok(points)
}

fn rel_pair(scan: &mut Scan<'_>, relative: bool, cx: f32, cy: f32) -> Option<(f32, f32)> {
    let x = scan.number()? as f32;
    let y = scan.number()? as f32;
    if relative {
        Some((cx + x, cy + y))
    } else {
        Some((x, y))
    }
}

struct Scan<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl Scan<'_> {
    fn skip_sep(&mut self) {
        while let Some(byte) = self.bytes.get(self.index) {
            if matches!(byte, b',' | b' ' | b'\n' | b'\r' | b'\t') {
                self.index += 1;
            } else {
                break;
            }
        }
    }

    fn number(&mut self) -> Option<f64> {
        self.skip_sep();
        let start = self.index;
        if matches!(self.bytes.get(self.index), Some(b'+' | b'-')) {
            self.index += 1;
        }
        let mut saw = false;
        while self.bytes.get(self.index).is_some_and(|byte| byte.is_ascii_digit()) {
            saw = true;
            self.index += 1;
        }
        if self.bytes.get(self.index) == Some(&b'.') {
            self.index += 1;
            while self.bytes.get(self.index).is_some_and(|byte| byte.is_ascii_digit()) {
                saw = true;
                self.index += 1;
            }
        }
        if self.bytes.get(self.index).is_some_and(|byte| matches!(byte, b'e' | b'E')) {
            let exp = self.index;
            self.index += 1;
            if matches!(self.bytes.get(self.index), Some(b'+' | b'-')) {
                self.index += 1;
            }
            let mut exp_digit = false;
            while self.bytes.get(self.index).is_some_and(|byte| byte.is_ascii_digit()) {
                exp_digit = true;
                self.index += 1;
            }
            if !exp_digit {
                self.index = exp;
            }
        }
        if !saw {
            self.index = start;
            return None;
        }
        std::str::from_utf8(&self.bytes[start..self.index])
            .ok()?
            .parse()
            .ok()
    }
}

fn clock(raw: &str) -> Option<f64> {
    let raw = raw.trim();
    let lower = raw.to_ascii_lowercase();
    let value = if let Some(rest) = lower.strip_suffix("ms") {
        rest.trim().parse::<f64>().ok()? / 1000.0
    } else if let Some(rest) = lower.strip_suffix("min") {
        rest.trim().parse::<f64>().ok()? * 60.0
    } else if let Some(rest) = lower.strip_suffix('h') {
        rest.trim().parse::<f64>().ok()? * 3600.0
    } else if let Some(rest) = lower.strip_suffix('s') {
        rest.trim().parse::<f64>().ok()?
    } else if lower.contains(':') {
        let parts: Vec<&str> = lower.split(':').collect();
        if parts.len() == 2 {
            parts[0].trim().parse::<f64>().ok()? * 60.0 + parts[1].trim().parse::<f64>().ok()?
        } else if parts.len() == 3 {
            parts[0].trim().parse::<f64>().ok()? * 3600.0
                + parts[1].trim().parse::<f64>().ok()? * 60.0
                + parts[2].trim().parse::<f64>().ok()?
        } else {
            return None;
        }
    } else {
        lower.parse().ok()?
    };
    value.is_finite().then_some(value)
}

fn clock_number(raw: &str) -> Option<f64> {
    raw.trim().parse::<f64>().ok().filter(|value| value.is_finite())
}

fn parse_number(raw: &str) -> Option<(f64, String)> {
    let mut end = 0usize;
    let bytes = raw.as_bytes();
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    let mut saw = false;
    while bytes.get(end).is_some_and(|byte| byte.is_ascii_digit()) {
        saw = true;
        end += 1;
    }
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        while bytes.get(end).is_some_and(|byte| byte.is_ascii_digit()) {
            saw = true;
            end += 1;
        }
    }
    if !saw {
        return None;
    }
    let value: f64 = raw[..end].parse().ok()?;
    let unit = raw[end..].trim().to_string();
    if unit.chars().any(|ch| !(ch.is_ascii_alphabetic() || ch == '%')) {
        return None;
    }
    value.is_finite().then_some((value, unit))
}

fn parse_color(raw: &str) -> Option<(f32, f32, f32, f32)> {
    let raw = raw.trim();
    if let Some(hex) = raw.strip_prefix('#') {
        return parse_hex(hex);
    }
    let lower = raw.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("rgba(").and_then(|rest| rest.strip_suffix(')')) {
        return parse_rgb_fn(rest, true);
    }
    if let Some(rest) = lower.strip_prefix("rgb(").and_then(|rest| rest.strip_suffix(')')) {
        return parse_rgb_fn(rest, false);
    }
    named_color(&lower)
}

fn parse_hex(hex: &str) -> Option<(f32, f32, f32, f32)> {
    let byte = |s: &str| u8::from_str_radix(s, 16).ok().map(|v| v as f32 / 255.0);
    let expand = |ch: u8| {
        let v = u8::from_str_radix(std::str::from_utf8(&[ch, ch]).ok()?, 16).ok()?;
        Some(v as f32 / 255.0)
    };
    match hex.len() {
        3 | 4 => {
            let bytes = hex.as_bytes();
            Some((
                expand(bytes[0])?,
                expand(bytes[1])?,
                expand(bytes[2])?,
                if hex.len() == 4 { expand(bytes[3])? } else { 1.0 },
            ))
        }
        6 | 8 => Some((
            byte(&hex[0..2])?,
            byte(&hex[2..4])?,
            byte(&hex[4..6])?,
            if hex.len() == 8 { byte(&hex[6..8])? } else { 1.0 },
        )),
        _ => None,
    }
}

fn parse_rgb_fn(raw: &str, rgba: bool) -> Option<(f32, f32, f32, f32)> {
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    if parts.len() != if rgba { 4 } else { 3 } {
        return None;
    }
    let channel = |part: &str| -> Option<f32> {
        if let Some(percent) = part.strip_suffix('%') {
            let value: f32 = percent.trim().parse().ok()?;
            return value.is_finite().then_some((value / 100.0).clamp(0.0, 1.0));
        }
        let value: f32 = part.parse().ok()?;
        value.is_finite().then_some((value / 255.0).clamp(0.0, 1.0))
    };
    Some((
        channel(parts[0])?,
        channel(parts[1])?,
        channel(parts[2])?,
        if rgba {
            let alpha: f32 = parts[3].parse().ok()?;
            if !alpha.is_finite() {
                return None;
            }
            alpha.clamp(0.0, 1.0)
        } else {
            1.0
        },
    ))
}

fn named_color(name: &str) -> Option<(f32, f32, f32, f32)> {
    let (r, g, b) = match name {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "lime" => (0, 255, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" | "aqua" => (0, 255, 255),
        "magenta" | "fuchsia" => (255, 0, 255),
        "gray" | "grey" => (128, 128, 128),
        "green" => (0, 128, 0),
        "purple" => (128, 0, 128),
        "orange" => (255, 165, 0),
        "transparent" => return Some((0.0, 0.0, 0.0, 0.0)),
        _ => return None,
    };
    Some((r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0))
}

fn phase(timing: &Timing, time: f64) -> Option<f64> {
    if !time.is_finite() {
        return None;
    }
    let local = time - timing.begin;
    if local < 0.0 {
        return None;
    }
    let mut active = match timing.repeat {
        Repeat::Once => timing.dur,
        Repeat::Count(count) => timing.dur * count,
        Repeat::Forever => f64::INFINITY,
    };
    if let Some(repeat_dur) = timing.repeat_dur {
        active = active.min(repeat_dur);
    }
    if let Some(end) = timing.end {
        let cap = end - timing.begin;
        if cap <= 0.0 {
            return None;
        }
        active = active.min(cap);
    }
    if local >= active {
        return timing.fill_freeze.then_some(1.0);
    }
    let into = local % timing.dur;
    Some((into / timing.dur).clamp(0.0, 1.0))
}

fn sample_values(values: &[SmilValue], key_times: Option<&[f64]>, u: f64, mode: CalcMode) -> String {
    if values.is_empty() {
        return String::new();
    }
    if values.len() == 1 || mode == CalcMode::Discrete && values.len() == 1 {
        return values[0].format();
    }
    let u = u.clamp(0.0, 1.0);
    match mode {
        CalcMode::Discrete => {
            let index = if let Some(keys) = key_times {
                let mut index = 0usize;
                for (i, time) in keys.iter().enumerate() {
                    if u + 1e-6 >= *time {
                        index = i;
                    }
                }
                index
            } else {
                ((u * values.len() as f64).floor() as usize).min(values.len() - 1)
            };
            values[index].format()
        }
        CalcMode::Linear | CalcMode::Paced => {
            let (index, span) = if mode == CalcMode::Paced {
                paced_span(values, u)
            } else if let Some(keys) = key_times {
                keyed_span(keys, u)
            } else {
                let pos = u * (values.len() - 1) as f64;
                let index = (pos.floor() as usize).min(values.len() - 2);
                (index, pos - index as f64)
            };
            lerp_value(&values[index], &values[index + 1], span)
        }
    }
}

fn keyed_span(keys: &[f64], u: f64) -> (usize, f64) {
    if keys.len() < 2 {
        return (0, 0.0);
    }
    for index in 0..keys.len() - 1 {
        if u <= keys[index + 1] {
            let width = keys[index + 1] - keys[index];
            let span = if width <= 1e-8 {
                0.0
            } else {
                ((u - keys[index]) / width).clamp(0.0, 1.0)
            };
            return (index, span);
        }
    }
    (keys.len() - 2, 1.0)
}

fn paced_span(values: &[SmilValue], u: f64) -> (usize, f64) {
    let mut weights = Vec::new();
    let mut total = 0.0;
    for pair in values.windows(2) {
        let weight = distance(&pair[0], &pair[1]).max(0.0);
        weights.push(weight);
        total += weight;
    }
    if total <= 1e-8 {
        let pos = u * (values.len() - 1) as f64;
        let index = (pos.floor() as usize).min(values.len() - 2);
        return (index, pos - index as f64);
    }
    let mut remain = u.clamp(0.0, 1.0) * total;
    for (index, weight) in weights.iter().enumerate() {
        if remain <= *weight || index + 1 == weights.len() {
            let span = if *weight <= 1e-8 {
                0.0
            } else {
                (remain / weight).clamp(0.0, 1.0)
            };
            return (index, span);
        }
        remain -= weight;
    }
    (values.len() - 2, 1.0)
}

fn distance(left: &SmilValue, right: &SmilValue) -> f64 {
    match (left, right) {
        (SmilValue::Number { value: a, .. }, SmilValue::Number { value: b, .. }) => (a - b).abs(),
        (
            SmilValue::Color {
                r: ar,
                g: ag,
                b: ab,
                a: aa,
            },
            SmilValue::Color {
                r: br,
                g: bg,
                b: bb,
                a: ba,
            },
        ) => {
            let dr = f64::from(ar - br);
            let dg = f64::from(ag - bg);
            let db = f64::from(ab - bb);
            let da = f64::from(aa - ba);
            (dr * dr + dg * dg + db * db + da * da).sqrt()
        }
        _ => 0.0,
    }
}

fn lerp_value(left: &SmilValue, right: &SmilValue, span: f64) -> String {
    if span <= 0.0 {
        return left.format();
    }
    if span >= 1.0 {
        return right.format();
    }
    match (left, right) {
        (
            SmilValue::Number {
                value: a,
                unit,
            },
            SmilValue::Number { value: b, .. },
        ) => format!("{}{unit}", fmt_num(a + (b - a) * span)),
        (
            SmilValue::Color {
                r: ar,
                g: ag,
                b: ab,
                a: aa,
            },
            SmilValue::Color {
                r: br,
                g: bg,
                b: bb,
                a: ba,
            },
        ) => {
            let mix = |a: f32, b: f32| a + (b - a) * span as f32;
            fmt_color(mix(*ar, *br), mix(*ag, *bg), mix(*ab, *bb), mix(*aa, *ba))
        }
        _ => right.format(),
    }
}

fn sample_frames(frames: &[Vec<f64>], key_times: Option<&[f64]>, u: f64, mode: CalcMode) -> Vec<f64> {
    if frames.is_empty() {
        return Vec::new();
    }
    if frames.len() == 1 {
        return frames[0].clone();
    }
    let u = u.clamp(0.0, 1.0);
    if mode == CalcMode::Discrete {
        let index = ((u * frames.len() as f64).floor() as usize).min(frames.len() - 1);
        return frames[index].clone();
    }
    let (index, span) = if let Some(keys) = key_times {
        keyed_span(keys, u)
    } else {
        let pos = u * (frames.len() - 1) as f64;
        let index = (pos.floor() as usize).min(frames.len() - 2);
        (index, pos - index as f64)
    };
    let left = &frames[index];
    let right = &frames[index + 1];
    left.iter()
        .zip(right)
        .map(|(a, b)| a + (b - a) * span)
        .collect()
}

fn point_at(points: &[(f32, f32)], u: f64) -> (f32, f32) {
    if points.is_empty() {
        return (0.0, 0.0);
    }
    if points.len() == 1 {
        return points[0];
    }
    let u = u.clamp(0.0, 1.0) as f32;
    let mut lengths = Vec::new();
    let mut total = 0.0f32;
    for pair in points.windows(2) {
        let dx = pair[1].0 - pair[0].0;
        let dy = pair[1].1 - pair[0].1;
        let len = (dx * dx + dy * dy).sqrt();
        lengths.push(len);
        total += len;
    }
    if total <= f32::EPSILON {
        return points[0];
    }
    let mut remain = u * total;
    for (index, len) in lengths.iter().enumerate() {
        if remain <= *len || index + 1 == lengths.len() {
            let span = if *len <= f32::EPSILON {
                0.0
            } else {
                (remain / len).clamp(0.0, 1.0)
            };
            let start = points[index];
            let end = points[index + 1];
            return (
                start.0 + (end.0 - start.0) * span,
                start.1 + (end.1 - start.1) * span,
            );
        }
        remain -= len;
    }
    *points.last().unwrap()
}

fn fmt_num(value: f64) -> String {
    if !value.is_finite() {
        return "0".into();
    }
    if (value - value.round()).abs() < 1e-4 {
        return format!("{}", value.round() as i64);
    }
    let mut text = format!("{value:.4}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn fmt_color(r: f32, g: f32, b: f32, a: f32) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    if a >= 0.999 {
        format!(
            "#{:02x}{:02x}{:02x}",
            channel(r),
            channel(g),
            channel(b)
        )
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            channel(r),
            channel(g),
            channel(b),
            channel(a)
        )
    }
}

fn render_markup(base: &str, anims: &[SmilAnim], time: f64) -> String {
    let mut markup = base.to_string();
    for anim in anims {
        let name = match &anim.effect {
            Effect::Attr { name, .. } => name.as_str(),
            Effect::Transform { .. } => "transform",
            Effect::Motion { .. } => continue,
        };
        let Some(value) = sample_effect(anim, time) else {
            continue;
        };
        markup = patch_tag(&markup, anim.target, |tag| set_attr(tag, name, &value));
    }
    for anim in anims {
        let Effect::Motion { .. } = &anim.effect else {
            continue;
        };
        let Some(value) = sample_effect(anim, time) else {
            continue;
        };
        markup = patch_tag(&markup, anim.target, |tag| add_motion(tag, &value));
    }
    strip_smil(&markup)
}

fn sample_effect(anim: &SmilAnim, time: f64) -> Option<String> {
    let u = phase(&anim.timing, time)?;
    Some(match &anim.effect {
        Effect::Attr {
            mode,
            values,
            key_times,
            hold,
            ..
        } => {
            if *hold {
                values.last()?.format()
            } else {
                sample_values(values, key_times.as_deref(), u, *mode)
            }
        }
        Effect::Transform {
            kind,
            mode,
            frames,
            key_times,
        } => {
            let nums = sample_frames(frames, key_times.as_deref(), u, *mode);
            let body = nums
                .iter()
                .map(|value| fmt_num(*value))
                .collect::<Vec<_>>()
                .join(",");
            format!("{kind}({body})")
        }
        Effect::Motion { points, rotate } => {
            let (x, y) = point_at(points, u);
            let mut text = format!(" translate({},{})", fmt_num(f64::from(x)), fmt_num(f64::from(y)));
            if rotate.abs() > 1e-4 {
                text.push_str(&format!(" rotate({})", fmt_num(f64::from(*rotate))));
            }
            text
        }
    })
}

fn patch_tag(markup: &str, id: u32, edit: impl Fn(&str) -> String) -> String {
    let needle = format!("director-smil-id=\"{id}\"");
    let Some(pos) = markup.find(&needle) else {
        return markup.to_string();
    };
    let Some(start) = markup[..pos].rfind('<') else {
        return markup.to_string();
    };
    let Some(rel_end) = markup[pos..].find('>') else {
        return markup.to_string();
    };
    let end = pos + rel_end;
    let tag = &markup[start..=end];
    let new_tag = edit(tag);
    let mut out = String::with_capacity(markup.len() + new_tag.len());
    out.push_str(&markup[..start]);
    out.push_str(&new_tag);
    out.push_str(&markup[end + 1..]);
    out
}

fn set_attr(tag: &str, name: &str, value: &str) -> String {
    let tag = rewrite_attr(tag, name, |_| value.to_string(), true);
    rewrite_style(&tag, name, |_| value.to_string())
}

fn add_motion(tag: &str, suffix: &str) -> String {
    if style_has(tag, "transform") {
        return rewrite_style(tag, "transform", |current| format!("{current}{suffix}"));
    }
    rewrite_attr(
        tag,
        "transform",
        |current| match current {
            Some(value) if !value.trim().is_empty() => format!("{value}{suffix}"),
            _ => suffix.trim().to_string(),
        },
        true,
    )
}

fn rewrite_attr(
    tag: &str,
    name: &str,
    edit: impl FnOnce(Option<&str>) -> String,
    insert: bool,
) -> String {
    let key = format!(" {name}=\"");
    if let Some(rel) = tag.find(&key) {
        let value_start = rel + key.len();
        if let Some(rel_end) = tag[value_start..].find('"') {
            let value_end = value_start + rel_end;
            let next = edit(Some(&tag[value_start..value_end]));
            let mut out = String::new();
            out.push_str(&tag[..value_start]);
            out.push_str(&escape_xml(&next, true));
            out.push_str(&tag[value_end..]);
            return out;
        }
    }
    if !insert {
        return tag.to_string();
    }
    let next = edit(None);
    let mut out = String::new();
    out.push_str(&tag[..tag.len() - 1]);
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    out.push_str(&escape_xml(&next, true));
    out.push_str("\">");
    out
}

fn style_has(tag: &str, name: &str) -> bool {
    style_body(tag)
        .and_then(|style| decl_range(style, name))
        .is_some()
}

fn rewrite_style(tag: &str, name: &str, edit: impl Fn(&str) -> String) -> String {
    let Some(style) = style_body(tag) else {
        return tag.to_string();
    };
    let Some((start, end)) = decl_range(style, name) else {
        return tag.to_string();
    };
    let next = edit(style[start..end].trim());
    let mut body = String::new();
    body.push_str(&style[..start]);
    body.push_str(&next);
    body.push_str(&style[end..]);
    rewrite_attr(tag, "style", |_| body, false)
}

fn style_body(tag: &str) -> Option<&str> {
    let key = " style=\"";
    let rel = tag.find(key)?;
    let start = rel + key.len();
    let end = start + tag[start..].find('"')?;
    Some(&tag[start..end])
}

fn decl_range(style: &str, name: &str) -> Option<(usize, usize)> {
    let lower = style.to_ascii_lowercase();
    let needle = name.to_ascii_lowercase();
    let mut search = 0usize;
    while let Some(rel) = lower[search..].find(&needle) {
        let index = search + rel;
        let boundary = index == 0
            || matches!(style.as_bytes()[index - 1], b';' | b' ' | b'\t' | b'\n');
        let after = index + needle.len();
        if boundary && style.as_bytes().get(after) == Some(&b':') {
            let mut value_start = after + 1;
            while style.as_bytes().get(value_start) == Some(&b' ') {
                value_start += 1;
            }
            let value_end = style[value_start..]
                .find(';')
                .map(|offset| value_start + offset)
                .unwrap_or(style.len());
            return Some((value_start, value_end));
        }
        search = index + needle.len();
    }
    None
}

fn strip_smil(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let name_len = after
            .find(|ch: char| ch.is_whitespace() || ch == '>' || ch == '/')
            .unwrap_or(after.len());
        let name = &after[..name_len];
        if smil_tag(name) {
            let close = format!("</{name}>");
            if let Some(rel) = rest[start..].find(&close) {
                rest = &rest[start + rel + close.len()..];
                continue;
            }
        }
        out.push('<');
        rest = after;
    }
    out.push_str(rest);
    out
}

fn attr<'a>(el: ElementRef<'a>, name: &str) -> Option<&'a str> {
    el.value()
        .attrs()
        .find(|(attr, _)| attr.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}

fn smil_err<T>(message: &str) -> Result<T, CompileError> {
    Err(CompileError::Input(message.into()))
}

pub(crate) fn check_unique(plan: &SmilPlan) -> Result<(), CompileError> {
    let mut seen = HashSet::new();
    for anim in &plan.anims {
        let key = match &anim.effect {
            Effect::Attr { name, .. } => (anim.target, name.clone()),
            Effect::Transform { .. } => (anim.target, "transform".to_string()),
            Effect::Motion { .. } => (anim.target, "animateMotion".to_string()),
        };
        if !seen.insert(key) {
            return smil_err("SMIL requires one animation per attribute");
        }
    }
    Ok(())
}

/// SVG markup plus SMIL samples. Each sample reparses usvg and clears the pixmap.
#[derive(Clone, Debug)]
pub(crate) struct AnimatedSvg {
    base: String,
    fonts: Arc<usvg::fontdb::Database>,
    anims: Vec<SmilAnim>,
    context: String,
    applied: String,
    width: f32,
    height: f32,
    node: VectorNode,
}

impl AnimatedSvg {
    pub(crate) fn new(
        base: String,
        fonts: Arc<usvg::fontdb::Database>,
        plan: &SmilPlan,
        context: &str,
    ) -> Result<Self, CompileError> {
        let anims = plan.anims.clone();
        let applied = render_markup(&base, &anims, 0.0);
        let tree = parse_svg(&applied, context, &fonts)?;
        let size = tree.size();
        Ok(Self {
            base,
            fonts,
            anims,
            context: context.to_string(),
            applied,
            width: size.width(),
            height: size.height(),
            node: VectorNode::from_tree(tree),
        })
    }

    pub(crate) fn size(&self) -> (f32, f32) {
        (self.width, self.height)
    }
}

impl Element for AnimatedSvg {
    fn kind(&self) -> &'static str {
        "svg_smil"
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn varies_with_time(&self) -> bool {
        true
    }
    fn export_body(&self, cx: &mut ExportContext) -> Result<ElementExport, ExportError> {
        Ok(ElementExport {
            opacity: channel(&self.node.opacity),
            body: NodeBody::SvgSmil(doc::SvgSmilBody {
                markup: cx.embedded_asset("svg", "image/svg+xml", self.base.as_bytes()),
                animations: self.anims.iter().map(SmilAnim::to_doc).collect(),
            }),
        })
    }
    fn layout_style(&self) -> taffy::Style {
        self.node.layout_style()
    }
    fn set_layout_style(&mut self, style: taffy::Style) {
        self.node.set_layout_style(style);
    }
    fn set_opacity(&mut self, opacity: f32) {
        self.node.set_opacity(opacity);
    }
    fn update(&mut self, time: f64) -> bool {
        let markup = render_markup(&self.base, &self.anims, time);
        if markup != self.applied {
            if let Ok(tree) = parse_svg(&markup, &self.context, &self.fonts) {
                self.node.set_tree(tree);
                self.applied = markup;
            }
        }
        self.node.update(time);
        true
    }
    fn render(
        &self,
        canvas: &skia_safe::Canvas,
        rect: skia_safe::Rect,
        opacity: f32,
        draw_children: &mut dyn FnMut(&skia_safe::Canvas),
    ) -> Result<(), RenderError> {
        self.node.render(canvas, rect, opacity, draw_children)
    }
    fn animate_property(
        &mut self,
        property: &str,
        start: f32,
        target: f32,
        duration: f64,
        easing: &str,
    ) -> Result<(), String> {
        self.node
            .animate_property(property, start, target, duration, easing)
    }
}


impl SmilAnim {
    fn to_doc(&self) -> doc::SmilAnimation {
        let mode = |mode: &CalcMode| match mode {
            CalcMode::Linear => doc::SmilCalcMode::Linear,
            CalcMode::Discrete => doc::SmilCalcMode::Discrete,
            CalcMode::Paced => doc::SmilCalcMode::Paced,
        };
        doc::SmilAnimation {
            target: self.target,
            begin: self.timing.begin,
            dur: self.timing.dur,
            end: self.timing.end,
            repeat: match self.timing.repeat {
                Repeat::Once => doc::SmilRepeat::Once,
                Repeat::Count(count) => doc::SmilRepeat::Count(count),
                Repeat::Forever => doc::SmilRepeat::Forever,
            },
            repeat_dur: self.timing.repeat_dur,
            freeze: self.timing.fill_freeze,
            effect: match &self.effect {
                Effect::Attr {
                    name,
                    mode: calc,
                    values,
                    key_times,
                    hold,
                } => doc::SmilEffect::Attribute {
                    name: name.clone(),
                    mode: mode(calc),
                    values: values
                        .iter()
                        .map(|value| match value {
                            SmilValue::Number { value, unit } => doc::SmilValue::Number {
                                value: *value,
                                unit: unit.clone(),
                            },
                            SmilValue::Color { r, g, b, a } => doc::SmilValue::Color(director_scene::Rgba {
                                r: *r,
                                g: *g,
                                b: *b,
                                a: *a,
                            }),
                            SmilValue::Raw(raw) => doc::SmilValue::Raw(raw.clone()),
                        })
                        .collect(),
                    key_times: key_times.clone(),
                    hold: *hold,
                },
                Effect::Transform {
                    kind,
                    mode: calc,
                    frames,
                    key_times,
                } => doc::SmilEffect::Transform {
                    kind: kind.to_string(),
                    mode: mode(calc),
                    frames: frames.clone(),
                    key_times: key_times.clone(),
                },
                Effect::Motion { points, rotate } => doc::SmilEffect::Motion {
                    points: points.iter().map(|(x, y)| [*x, *y]).collect(),
                    rotate: *rotate,
                },
            },
        }
    }
}
