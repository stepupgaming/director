//! Track lists for `grid-template-columns` and `grid-template-rows`.
//!
//! taffy 0.9.2 names the repetition enum `RepetitionCount`
//! (`AutoFit`, `AutoFill`, `Count`). It has no `GridTrackRepetition` type.
//! Line names live on `GridTemplateRepetition::line_names`: one set per grid line,
//! `line_names[i]` before track `i` and the last set after the final track.
//! A child custom ident resolves against the parent axis to the same 1-based
//! line index Taffy uses for a numeric grid line. An auto repeat keeps the name
//! for Taffy, because the repetition count is not known until layout.
//! `subgrid` copies the parent axis when that axis has a known track count.
//! A missing axis, an auto repeat, and `subgrid` inside `repeat()` fail.

use crate::style::{self, LengthParts, Metrics};
use std::cell::RefCell;
use taffy::style::{
    GridTemplateComponent, GridTemplateRepetition, RepetitionCount, TrackSizingFunction,
};

#[cfg(test)]
fn grid_template(
    source: &str,
    metrics: Metrics,
) -> Result<Vec<GridTemplateComponent<String>>, String> {
    grid_template_axis(source, metrics, true)
}

pub(crate) fn grid_template_axis(
    source: &str,
    metrics: Metrics,
    columns: bool,
) -> Result<Vec<GridTemplateComponent<String>>, String> {
    let source = source.trim();
    if source.is_empty() || source == "none" {
        return Ok(Vec::new());
    }
    if let Some(tail) = subgrid_tail(source) {
        return copy_subgrid(tail, columns);
    }
    components_from_parts(parse_parts(source, metrics)?)
}

fn subgrid_tail(source: &str) -> Option<&str> {
    let rest = source.get("subgrid".len()..)?;
    if !source[.."subgrid".len()].eq_ignore_ascii_case("subgrid") {
        return None;
    }
    if rest.is_empty() || rest.starts_with(char::is_whitespace) || rest.starts_with('[') {
        Some(rest.trim())
    } else {
        None
    }
}

fn subgrid_line_names(tail: &str) -> Result<Vec<Vec<String>>, String> {
    if tail.is_empty() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for token in crate::css::split(tail, false) {
        for piece in explode_glued(&token) {
            if piece.is_empty() {
                continue;
            }
            if piece.starts_with('[') {
                names.push(line_names(piece)?);
            } else if piece.eq_ignore_ascii_case("subgrid") || piece.starts_with("repeat(") {
                return Err("subgrid requires extraction".into());
            } else {
                return Err("named grid lines require extraction".into());
            }
        }
    }
    if names.is_empty() {
        return Err("named grid lines require extraction".into());
    }
    Ok(names)
}

fn copy_subgrid(tail: &str, columns: bool) -> Result<Vec<GridTemplateComponent<String>>, String> {
    let extra = subgrid_line_names(tail)?;
    let template = parent_definite(columns)?;
    if extra.is_empty() {
        return Ok(template);
    }
    let (tracks, mut names) = expand_template(&template)?;
    if names.len() != extra.len() {
        return Err("named grid lines require extraction".into());
    }
    for (line, more) in names.iter_mut().zip(extra) {
        for name in more {
            if !line.iter().any(|existing| existing == &name) {
                line.push(name);
            }
        }
    }
    Ok(vec![GridTemplateComponent::Repeat(
        GridTemplateRepetition {
            count: RepetitionCount::Count(1),
            tracks,
            line_names: names,
        },
    )])
}

fn grid_track(source: &str, metrics: Metrics) -> Result<TrackSizingFunction, String> {
    match source {
        "auto" => Ok(taffy::prelude::auto()),
        "min-content" => Ok(taffy::prelude::min_content()),
        "max-content" => Ok(taffy::prelude::max_content()),
        _ if source.ends_with("fr") => Ok(taffy::prelude::fr(style::number(
            source.trim_end_matches("fr"),
        )?)),
        _ if source.starts_with("minmax(") => {
            let inner = source
                .strip_prefix("minmax(")
                .and_then(|rest| rest.strip_suffix(')'))
                .ok_or_else(|| "minmax requires two arguments".to_string())?;
            let args = crate::css::split(inner, true);
            if args.len() != 2 {
                return Err("minmax requires two arguments".into());
            }
            let min = grid_track(&args[0], metrics)?;
            let max = grid_track(&args[1], metrics)?;
            Ok(taffy::prelude::minmax(min.min, max.max))
        }
        _ => {
            let length = style::length(source, metrics)?;
            Ok(if length.is_percent() {
                taffy::prelude::percent(length.value())
            } else {
                taffy::prelude::length(length.value())
            })
        }
    }
}

enum Part {
    Names(Vec<String>),
    Track(TrackSizingFunction),
    Repeat(GridTemplateRepetition<String>),
}

/// Top-level `[name]` groups may be glued to a track. Brackets inside `()` stay in that piece.
fn explode_glued(token: &str) -> Vec<&str> {
    if !token.contains('[') {
        return vec![token];
    }
    let mut pieces = Vec::new();
    let mut depth = 0u32;
    let mut start = 0usize;
    let mut rest = token.char_indices();
    while let Some((index, ch)) = rest.next() {
        match ch {
            '(' => depth = depth.saturating_add(1),
            ')' => depth = depth.saturating_sub(1),
            '[' if depth == 0 => {
                let Some((end, _)) = rest.find(|(_, next)| *next == ']') else {
                    return vec![token];
                };
                if index > start {
                    pieces.push(&token[start..index]);
                }
                pieces.push(&token[index..=end]);
                start = end + 1;
            }
            _ => {}
        }
    }
    if start < token.len() {
        pieces.push(&token[start..]);
    }
    if pieces.is_empty() {
        vec![token]
    } else {
        pieces
    }
}

fn parse_parts(source: &str, metrics: Metrics) -> Result<Vec<Part>, String> {
    let mut parts = Vec::new();
    for token in crate::css::split(source, false) {
        for piece in explode_glued(&token) {
            if piece.eq_ignore_ascii_case("subgrid") {
                return Err("subgrid requires extraction".into());
            } else if piece.starts_with('[') {
                parts.push(Part::Names(line_names(piece)?));
            } else if piece.starts_with("repeat(") {
                parts.push(Part::Repeat(parse_repeat(piece, metrics)?));
            } else if bare_line_name(piece) {
                return Err("named grid lines require extraction".into());
            } else {
                parts.push(Part::Track(grid_track(piece, metrics)?));
            }
        }
    }
    Ok(parts)
}

fn parse_repeat(token: &str, metrics: Metrics) -> Result<GridTemplateRepetition<String>, String> {
    let inner = token
        .strip_prefix("repeat(")
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| "repeat requires count and tracks".to_string())?;
    let args = crate::css::split(inner, true);
    if args.len() != 2 {
        return Err("repeat requires count and tracks".into());
    }
    let count = repetition_count(&args[0])?;
    let (tracks, line_names) = tracks_from_parts(parse_parts(&args[1], metrics)?)?;
    if tracks.is_empty() {
        return Err("repeat requires count and tracks".into());
    }
    Ok(GridTemplateRepetition {
        count,
        tracks,
        line_names,
    })
}

fn repetition_count(source: &str) -> Result<RepetitionCount, String> {
    match source {
        "auto-fit" => Ok(RepetitionCount::AutoFit),
        "auto-fill" => Ok(RepetitionCount::AutoFill),
        _ => {
            let count = source
                .parse::<usize>()
                .map_err(|_| "auto-fill/auto-fit require extraction".to_string())?;
            if count > 4096 {
                return Err("grid repeat exceeds 4096 tracks".into());
            }
            Ok(RepetitionCount::Count(count as u16))
        }
    }
}

fn line_names(token: &str) -> Result<Vec<String>, String> {
    let inner = token
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| "named grid lines require extraction".to_string())?;
    let mut names = Vec::new();
    for name in crate::css::split(inner, false) {
        if !is_custom_ident(&name) {
            return Err("named grid lines require extraction".into());
        }
        names.push(name);
    }
    Ok(names)
}

fn bare_line_name(token: &str) -> bool {
    !matches!(token, "auto" | "min-content" | "max-content" | "none") && is_custom_ident(token)
}

pub(crate) fn is_custom_ident(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first == '-' {
        match chars.next() {
            Some(second) if ident_continue(second) && !second.is_ascii_digit() => {}
            _ => return false,
        }
    } else if !ident_start(first) {
        return false;
    }
    chars.all(ident_continue)
}

fn ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

fn ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

#[derive(Default)]
enum LineState {
    #[default]
    Ready,
    Named(Vec<String>),
}

#[derive(Default)]
struct Acc {
    tracks: Vec<TrackSizingFunction>,
    names: Vec<Vec<String>>,
    state: LineState,
    saw: bool,
}

impl Acc {
    fn has_pending_name(&self) -> bool {
        matches!(self.state, LineState::Named(ref names) if !names.is_empty())
    }

    fn push_names(&mut self, names: Vec<String>) -> Result<(), String> {
        if !names.is_empty() {
            self.saw = true;
        }
        // Adjacent `[a] [b]` groups are one line. CSS joins them.
        match &mut self.state {
            LineState::Named(pending) => pending.extend(names),
            LineState::Ready => self.state = LineState::Named(names),
        }
        Ok(())
    }

    fn push_track(&mut self, track: TrackSizingFunction) {
        let before = match std::mem::replace(&mut self.state, LineState::Ready) {
            LineState::Named(names) => names,
            LineState::Ready => Vec::new(),
        };
        if !before.is_empty() {
            self.saw = true;
        }
        self.names.push(before);
        self.tracks.push(track);
    }

    /// `names` is empty when the repeated tracks have no brackets, otherwise one set per line.
    fn push_expanded(
        &mut self,
        tracks: Vec<TrackSizingFunction>,
        names: Vec<Vec<String>>,
    ) -> Result<(), String> {
        if tracks.is_empty() {
            return Ok(());
        }
        if names.is_empty() {
            for track in tracks {
                self.push_track(track);
            }
            return Ok(());
        }
        if names.len() != tracks.len() + 1 {
            return Err("named grid lines require extraction".into());
        }
        let mut names = names.into_iter();
        let Some(first) = names.next() else {
            return Err("named grid lines require extraction".into());
        };
        match std::mem::replace(&mut self.state, LineState::Ready) {
            LineState::Named(mut pending) => {
                pending.extend(first);
                if !pending.is_empty() {
                    self.saw = true;
                }
                self.state = LineState::Named(pending);
            }
            LineState::Ready => {
                if !first.is_empty() {
                    self.saw = true;
                }
                self.state = LineState::Named(first);
            }
        }
        for track in tracks {
            let Some(after) = names.next() else {
                return Err("named grid lines require extraction".into());
            };
            self.push_track(track);
            if !after.is_empty() {
                self.saw = true;
            }
            self.state = LineState::Named(after);
        }
        Ok(())
    }

    fn detach_unattached_names(&mut self) -> Option<Vec<String>> {
        if !self.tracks.is_empty() {
            return None;
        }
        match std::mem::replace(&mut self.state, LineState::Ready) {
            LineState::Named(names) if !names.is_empty() => {
                self.saw = false;
                Some(names)
            }
            other => {
                self.state = other;
                None
            }
        }
    }

    fn into_tracks(self) -> Result<(Vec<TrackSizingFunction>, Vec<Vec<String>>), String> {
        if self.tracks.is_empty() {
            return if matches!(self.state, LineState::Named(ref names) if !names.is_empty()) {
                Err("repeat requires count and tracks".into())
            } else {
                Ok((Vec::new(), Vec::new()))
            };
        }
        if !self.saw {
            return Ok((self.tracks, Vec::new()));
        }
        let mut line_names = self.names;
        line_names.push(match self.state {
            LineState::Named(names) => names,
            LineState::Ready => Vec::new(),
        });
        Ok((self.tracks, line_names))
    }

    fn into_components(self) -> Result<Vec<GridTemplateComponent<String>>, String> {
        let (tracks, line_names) = self.into_tracks()?;
        if tracks.is_empty() {
            return Ok(Vec::new());
        }
        if line_names.is_empty() {
            return Ok(tracks
                .into_iter()
                .map(GridTemplateComponent::Single)
                .collect());
        }
        Ok(vec![GridTemplateComponent::Repeat(
            GridTemplateRepetition {
                count: RepetitionCount::Count(1),
                tracks,
                line_names,
            },
        )])
    }

    fn emit_repeat(
        &mut self,
        mut rep: GridTemplateRepetition<String>,
        next: Option<&Part>,
        out: &mut Vec<GridTemplateComponent<String>>,
    ) -> Result<(), String> {
        match rep.count {
            RepetitionCount::Count(times) => {
                // Expand when a bracket sits outside the repeat so that name is stored once.
                // A standalone integer repeat stays `RepetitionCount::Count`.
                let expand = times == 0
                    || self.saw
                    || self.has_pending_name()
                    || matches!(next, Some(Part::Names(_)));
                if expand {
                    let (tracks, names) = expand_count(times, &rep.tracks, &rep.line_names)?;
                    self.push_expanded(tracks, names)
                } else if !self.tracks.is_empty() {
                    out.extend(std::mem::take(self).into_components()?);
                    out.push(GridTemplateComponent::Repeat(rep));
                    Ok(())
                } else {
                    *self = Acc::default();
                    out.push(GridTemplateComponent::Repeat(rep));
                    Ok(())
                }
            }
            // An outside bracket whose only neighbor is an auto repeat is merged into that
            // repeat's first or last line. Taffy then repeats the name with every copy.
            RepetitionCount::AutoFit | RepetitionCount::AutoFill => {
                if let Some(names) = self.detach_unattached_names() {
                    attach_names(&mut rep, true, names);
                    out.push(GridTemplateComponent::Repeat(rep));
                } else if !self.tracks.is_empty() {
                    out.extend(std::mem::take(self).into_components()?);
                    out.push(GridTemplateComponent::Repeat(rep));
                } else {
                    out.push(GridTemplateComponent::Repeat(rep));
                }
                Ok(())
            }
        }
    }

    fn finish(mut self, out: &mut Vec<GridTemplateComponent<String>>) -> Result<(), String> {
        if let Some(names) = self.detach_unattached_names() {
            match out.last_mut() {
                Some(GridTemplateComponent::Repeat(rep)) => attach_names(rep, false, names),
                _ => return Err("named grid lines require extraction".into()),
            }
        }
        if !self.tracks.is_empty() {
            out.extend(self.into_components()?);
        }
        Ok(())
    }
}

fn components_from_parts(parts: Vec<Part>) -> Result<Vec<GridTemplateComponent<String>>, String> {
    let mut out = Vec::new();
    let mut acc = Acc::default();
    let mut parts = parts.into_iter().peekable();
    while let Some(part) = parts.next() {
        match part {
            Part::Names(names) => acc.push_names(names)?,
            Part::Track(track) => acc.push_track(track),
            Part::Repeat(rep) => acc.emit_repeat(rep, parts.peek(), &mut out)?,
        }
    }
    acc.finish(&mut out)?;
    Ok(out)
}

fn tracks_from_parts(
    parts: Vec<Part>,
) -> Result<(Vec<TrackSizingFunction>, Vec<Vec<String>>), String> {
    let mut acc = Acc::default();
    for part in parts {
        match part {
            Part::Names(names) => acc.push_names(names)?,
            Part::Track(track) => acc.push_track(track),
            Part::Repeat(rep) => {
                let times = match rep.count {
                    RepetitionCount::Count(times) => times,
                    RepetitionCount::AutoFit | RepetitionCount::AutoFill => {
                        return Err("auto-fill/auto-fit require extraction".into());
                    }
                };
                let (tracks, names) = expand_count(times, &rep.tracks, &rep.line_names)?;
                acc.push_expanded(tracks, names)?;
            }
        }
    }
    acc.into_tracks()
}

/// Repeat `tracks` `times` times. Boundary line names collapse the way CSS joins
/// `repeat(n, [a] ... [b])` into `[a] ... [b a] ... [b]`.
fn expand_count(
    times: u16,
    tracks: &[TrackSizingFunction],
    line_names: &[Vec<String>],
) -> Result<(Vec<TrackSizingFunction>, Vec<Vec<String>>), String> {
    if times == 0 || tracks.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    if line_names.is_empty() {
        let mut out = Vec::with_capacity(tracks.len() * usize::from(times));
        for _ in 0..times {
            out.extend(tracks.iter().copied());
        }
        return Ok((out, Vec::new()));
    }
    if line_names.len() != tracks.len() + 1 {
        return Err("named grid lines require extraction".into());
    }
    let mut out_tracks = Vec::with_capacity(tracks.len() * usize::from(times));
    let mut out_names = Vec::with_capacity(tracks.len() * usize::from(times) + 1);
    for index in 0..times {
        if index == 0 {
            out_names.push(line_names[0].clone());
        } else if let Some(last) = out_names.last_mut() {
            last.extend(line_names[0].iter().cloned());
        }
        out_tracks.extend(tracks.iter().copied());
        out_names.extend(line_names.iter().skip(1).cloned());
    }
    Ok((out_tracks, out_names))
}

/// A resolved custom ident. `Line` is the 1-based index a numeric line would use.
/// `Named` is an auto repeat: Taffy still has to pick the copy.
pub(crate) enum NamedPlacement {
    Line(i16),
    Named(String, i16),
}

#[derive(Default)]
struct AxisMap {
    lines: Vec<Vec<String>>,
    tracks: u16,
    auto: bool,
    known: Vec<String>,
    template: Vec<GridTemplateComponent<String>>,
}

#[derive(Default)]
struct Frame {
    columns: AxisMap,
    rows: AxisMap,
}

struct Slot {
    compile_id: usize,
    epoch: u64,
    stack: Vec<Frame>,
    /// Frame-stack length at the start of each entered element.
    scopes: Vec<usize>,
    /// The top frame belongs to the element currently being translated.
    open: bool,
}

thread_local! {
    static LINES: RefCell<Slot> = RefCell::new(Slot {
        compile_id: 0,
        epoch: 0,
        stack: Vec::new(),
        scopes: Vec::new(),
        open: false,
    });
}

/// Named-line scope for one element. Hold it until that element's children
/// have been translated. Dropping it removes the template this element installed.
pub(crate) struct GridScope {
    compile_id: usize,
    epoch: u64,
}

impl Drop for GridScope {
    fn drop(&mut self) {
        end_element(self.compile_id, self.epoch);
    }
}

/// Start one element. The parent template stays the current axis until the
/// returned scope drops.
pub(crate) fn begin_element(compile_id: usize) -> GridScope {
    LINES.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.compile_id != compile_id {
            slot.compile_id = compile_id;
            slot.epoch = slot.epoch.wrapping_add(1);
            slot.stack.clear();
            slot.scopes.clear();
            slot.open = false;
        } else if slot.open {
            slot.open = false;
        }
        let depth = slot.stack.len();
        slot.scopes.push(depth);
        GridScope {
            compile_id,
            epoch: slot.epoch,
        }
    })
}

fn end_element(compile_id: usize, epoch: u64) {
    LINES.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.compile_id != compile_id || slot.epoch != epoch {
            return;
        }
        if let Some(base) = slot.scopes.pop() {
            slot.stack.truncate(base);
            slot.open = false;
        }
    });
}

/// Store line names for later child placement and give Taffy one empty name
/// slot per component so it walks `repeat` line names.
pub(crate) fn install_tracks(
    columns: bool,
    mut tracks: Vec<GridTemplateComponent<String>>,
) -> (Vec<GridTemplateComponent<String>>, Vec<Vec<String>>) {
    for component in &mut tracks {
        if let GridTemplateComponent::Repeat(rep) = component {
            if rep.line_names.is_empty() && !rep.tracks.is_empty() {
                rep.line_names.resize(rep.tracks.len() + 1, Vec::new());
            }
        }
    }
    let map = axis_map(&tracks);
    LINES.with(|slot| {
        let mut slot = slot.borrow_mut();
        if !slot.open {
            slot.stack.push(Frame::default());
            slot.open = true;
        }
        if let Some(frame) = slot.stack.last_mut() {
            if columns {
                frame.columns = map;
            } else {
                frame.rows = map;
            }
        }
    });
    let slots = vec![Vec::new(); tracks.len()];
    (tracks, slots)
}

pub(crate) fn resolve_named(columns: bool, name: &str, nth: i16) -> Result<NamedPlacement, String> {
    LINES.with(|slot| {
        let slot = slot.borrow();
        let frame = parent_frame(&slot)?;
        let axis = if columns { &frame.columns } else { &frame.rows };
        if !axis.known.iter().any(|known| known == name) {
            return Err("named grid lines require extraction".into());
        }
        if axis.auto {
            let nth = if nth == 0 { 1 } else { nth };
            return Ok(NamedPlacement::Named(name.to_string(), nth));
        }
        Ok(NamedPlacement::Line(line_at(
            &axis.lines,
            axis.tracks,
            name,
            nth,
        )?))
    })
}

fn parent_frame(slot: &Slot) -> Result<&Frame, String> {
    let frame = if slot.open {
        slot.stack.get(slot.stack.len().wrapping_sub(2))
    } else {
        slot.stack.last()
    };
    frame.ok_or_else(|| "named grid lines require extraction".to_string())
}

fn parent_definite(columns: bool) -> Result<Vec<GridTemplateComponent<String>>, String> {
    LINES.with(|slot| {
        let slot = slot.borrow();
        let frame = match parent_frame(&slot) {
            Ok(frame) => frame,
            Err(_) => return Err("subgrid requires a definite parent grid".into()),
        };
        let axis = if columns { &frame.columns } else { &frame.rows };
        if axis.auto || axis.tracks == 0 || axis.template.is_empty() {
            return Err("subgrid requires a definite parent grid".into());
        }
        Ok(axis.template.clone())
    })
}

fn axis_map(components: &[GridTemplateComponent<String>]) -> AxisMap {
    let mut lines = Vec::new();
    let mut known = Vec::new();
    let mut auto = false;
    let mut tracks: u16 = 0;
    let mut current_line: i16 = 0;
    for component in components {
        current_line += 1;
        match component {
            GridTemplateComponent::Single(_) => tracks = tracks.saturating_add(1),
            GridTemplateComponent::Repeat(rep) => {
                let times = match rep.count {
                    RepetitionCount::Count(count) => count,
                    RepetitionCount::AutoFit | RepetitionCount::AutoFill => {
                        auto = true;
                        for set in &rep.line_names {
                            record(&mut lines, &mut known, 0, set);
                        }
                        continue;
                    }
                };
                tracks = tracks.saturating_add(times.saturating_mul(rep.tracks.len() as u16));
                let sets: Vec<Vec<String>> = if rep.line_names.is_empty() {
                    vec![Vec::new(); rep.tracks.len() + 1]
                } else {
                    rep.line_names.clone()
                };
                for _ in 0..times {
                    for set in &sets {
                        record(&mut lines, &mut known, current_line, set);
                        current_line = current_line.saturating_add(1);
                    }
                    current_line = current_line.saturating_sub(1);
                }
                current_line = current_line.saturating_sub(1);
            }
        }
    }
    AxisMap {
        lines,
        tracks,
        auto,
        known,
        template: components.to_vec(),
    }
}

/// Expand a definite template into one track list and one name set per line.
fn expand_template(
    components: &[GridTemplateComponent<String>],
) -> Result<(Vec<TrackSizingFunction>, Vec<Vec<String>>), String> {
    let mut tracks = Vec::new();
    let mut names = Vec::new();
    for component in components {
        let (more_tracks, more_names) = match component {
            GridTemplateComponent::Single(track) => (vec![*track], vec![Vec::new(), Vec::new()]),
            GridTemplateComponent::Repeat(rep) => {
                let times = match rep.count {
                    RepetitionCount::Count(times) => times,
                    RepetitionCount::AutoFit | RepetitionCount::AutoFill => {
                        return Err("subgrid requires a definite parent grid".into());
                    }
                };
                let (more_tracks, more_names) = expand_count(times, &rep.tracks, &rep.line_names)?;
                if more_names.is_empty() {
                    let slots = vec![Vec::new(); more_tracks.len() + 1];
                    (more_tracks, slots)
                } else {
                    (more_tracks, more_names)
                }
            }
        };
        if more_tracks.is_empty() {
            continue;
        }
        if names.is_empty() {
            names = more_names;
        } else {
            let mut more_names = more_names.into_iter();
            if let Some(first) = more_names.next() {
                if let Some(last) = names.last_mut() {
                    last.extend(first);
                }
            }
            names.extend(more_names);
        }
        tracks.extend(more_tracks);
    }
    if tracks.is_empty() {
        return Err("subgrid requires a definite parent grid".into());
    }
    Ok((tracks, names))
}

fn record(lines: &mut Vec<Vec<String>>, known: &mut Vec<String>, line: i16, names: &[String]) {
    if line > 0 {
        let index = (line as usize) - 1;
        if lines.len() <= index {
            lines.resize(index + 1, Vec::new());
        }
        for name in names {
            if !lines[index].iter().any(|existing| existing == name) {
                lines[index].push(name.clone());
            }
        }
    }
    for name in names {
        if !name.is_empty() && !known.iter().any(|existing| existing == name) {
            known.push(name.clone());
        }
    }
}

/// 1-based line index for `name`, or the Taffy fallback when the nth copy is missing.
fn line_at(lines: &[Vec<String>], tracks: u16, name: &str, nth: i16) -> Result<i16, String> {
    let mut found = Vec::new();
    for (index, set) in lines.iter().enumerate() {
        if set.iter().any(|candidate| candidate == name) {
            found.push((index as i16) + 1);
        }
    }
    if found.is_empty() {
        return Err("named grid lines require extraction".into());
    }
    let nth = if nth == 0 { 1 } else { nth };
    let abs = usize::from(nth.unsigned_abs());
    if abs == 0 || abs > found.len() {
        let explicit = i16::try_from(tracks).unwrap_or(i16::MAX);
        let remaining = (abs as i16 - found.len() as i16) * nth.signum();
        return Ok(if nth > 0 {
            (explicit + 1) + remaining
        } else {
            -((explicit + 1) + remaining)
        });
    }
    if nth > 0 {
        Ok(found[abs - 1])
    } else {
        Ok(found[found.len() - abs])
    }
}

#[cfg(test)]
fn line_index(
    components: &[GridTemplateComponent<String>],
    name: &str,
    nth: i16,
) -> Result<i16, String> {
    let axis = axis_map(components);
    if axis.auto || !axis.known.iter().any(|known| known == name) {
        return Err("named grid lines require extraction".into());
    }
    line_at(&axis.lines, axis.tracks, name, nth)
}

fn attach_names(rep: &mut GridTemplateRepetition<String>, at_start: bool, names: Vec<String>) {
    if names.is_empty() {
        return;
    }
    if rep.line_names.is_empty() {
        rep.line_names.resize(rep.tracks.len() + 1, Vec::new());
    }
    if rep.line_names.is_empty() {
        return;
    }
    let index = if at_start {
        0
    } else {
        rep.line_names.len() - 1
    };
    if at_start {
        let mut merged = names;
        merged.append(&mut rep.line_names[index]);
        rep.line_names[index] = merged;
    } else {
        rep.line_names[index].extend(names);
    }
}

#[cfg(test)]
mod tests {
    use super::grid_template;
    use crate::style::LengthParts;
    use taffy::prelude::{self, TrackSizingFunction};
    use taffy::style::{GridTemplateComponent, GridTemplateRepetition, RepetitionCount};

    fn metrics() -> crate::style::Metrics {
        crate::style::Metrics {
            width: 1920.0,
            height: 1080.0,
            font_size: 16.0,
        }
    }

    fn flex(value: f32) -> TrackSizingFunction {
        prelude::fr::<f32, TrackSizingFunction>(value)
    }

    fn px(value: f32) -> TrackSizingFunction {
        prelude::length::<f32, TrackSizingFunction>(value)
    }

    fn single(track: TrackSizingFunction) -> GridTemplateComponent<String> {
        GridTemplateComponent::Single(track)
    }

    #[test]
    fn none_is_empty() {
        let tracks = grid_template("none", metrics()).unwrap();
        assert!(tracks.is_empty());
    }

    #[test]
    fn one_fr() {
        let tracks = grid_template("1fr", metrics()).unwrap();
        assert_eq!(tracks, vec![single(flex(1.0))]);
    }

    #[test]
    fn repeat_three_fr() {
        let tracks = grid_template("repeat(3, 1fr)", metrics()).unwrap();
        assert_eq!(
            tracks,
            vec![GridTemplateComponent::Repeat(GridTemplateRepetition {
                count: RepetitionCount::Count(3),
                tracks: vec![flex(1.0)],
                line_names: Vec::new(),
            })]
        );
    }

    #[test]
    fn repeat_auto_fit_minmax() {
        let tracks = grid_template("repeat(auto-fit, minmax(40px, 1fr))", metrics()).unwrap();
        let min = px(40.0);
        let max = flex(1.0);
        let track = prelude::minmax::<TrackSizingFunction>(min.min, max.max);
        assert_eq!(
            tracks,
            vec![GridTemplateComponent::Repeat(GridTemplateRepetition {
                count: RepetitionCount::AutoFit,
                tracks: vec![track],
                line_names: Vec::new(),
            })]
        );
    }

    #[test]
    fn repeat_auto_fill_px() {
        let tracks = grid_template("repeat(auto-fill, 20px)", metrics()).unwrap();
        assert_eq!(
            tracks,
            vec![GridTemplateComponent::Repeat(GridTemplateRepetition {
                count: RepetitionCount::AutoFill,
                tracks: vec![px(20.0)],
                line_names: Vec::new(),
            })]
        );
    }

    #[test]
    fn named_line() {
        let tracks = grid_template("[main] 1fr", metrics()).unwrap();
        assert_eq!(
            tracks,
            vec![GridTemplateComponent::Repeat(GridTemplateRepetition {
                count: RepetitionCount::Count(1),
                tracks: vec![flex(1.0)],
                line_names: vec![vec!["main".to_string()], Vec::new()],
            })]
        );

        let tracks = grid_template("[a b] 40px", metrics()).unwrap();
        assert_eq!(
            tracks,
            vec![GridTemplateComponent::Repeat(GridTemplateRepetition {
                count: RepetitionCount::Count(1),
                tracks: vec![px(40.0)],
                line_names: vec![vec!["a".to_string(), "b".to_string()], Vec::new()],
            })]
        );

        let tracks = grid_template("10px [mid] 1fr", metrics()).unwrap();
        assert_eq!(
            tracks,
            vec![GridTemplateComponent::Repeat(GridTemplateRepetition {
                count: RepetitionCount::Count(1),
                tracks: vec![px(10.0), flex(1.0)],
                line_names: vec![Vec::new(), vec!["mid".to_string()], Vec::new()],
            })]
        );
    }

    #[test]
    fn named_repeat() {
        let tracks = grid_template("repeat(2, [a] 1fr [b])", metrics()).unwrap();
        assert_eq!(
            tracks,
            vec![GridTemplateComponent::Repeat(GridTemplateRepetition {
                count: RepetitionCount::Count(2),
                tracks: vec![flex(1.0)],
                line_names: vec![vec!["a".to_string()], vec!["b".to_string()]],
            })]
        );
    }

    #[test]
    fn neighboring_name_on_integer_repeat() {
        let tracks = grid_template("[side] repeat(3, 1fr)", metrics()).unwrap();
        assert_eq!(
            tracks,
            vec![GridTemplateComponent::Repeat(GridTemplateRepetition {
                count: RepetitionCount::Count(1),
                tracks: vec![flex(1.0), flex(1.0), flex(1.0)],
                line_names: vec![vec!["side".to_string()], Vec::new(), Vec::new(), Vec::new()],
            })]
        );
    }

    #[test]
    fn repeat_count_4096_is_allowed() {
        let tracks = grid_template("repeat(4096, 1fr)", metrics()).unwrap();
        match tracks.as_slice() {
            [GridTemplateComponent::Repeat(rep)] => {
                assert_eq!(rep.count, RepetitionCount::Count(4096));
                assert_eq!(rep.tracks, vec![flex(1.0)]);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn repeat_count_4097() {
        let err = grid_template("repeat(4097, 1fr)", metrics()).unwrap_err();
        assert_eq!(err, "grid repeat exceeds 4096 tracks");
    }

    #[test]
    fn subgrid_copies_definite_parent_tracks() {
        let _parent_scope = super::begin_element(11);
        let parent = grid_template("[main] 32px [side] 32px", metrics()).unwrap();
        super::install_tracks(true, parent.clone());
        let _child_scope = super::begin_element(11);
        let child = super::grid_template_axis("subgrid", metrics(), true).unwrap();
        assert_eq!(child, parent);
        super::install_tracks(true, child.clone());
        let _grandchild_scope = super::begin_element(11);
        let grandchild = super::grid_template_axis("subgrid", metrics(), true).unwrap();
        assert_eq!(grandchild, child);
        match super::resolve_named(true, "side", 1).unwrap() {
            super::NamedPlacement::Line(index) => assert_eq!(index, 2),
            super::NamedPlacement::Named(_, _) => panic!("side resolved as an auto repeat"),
        }
    }

    #[test]
    fn sibling_after_nested_grid_uses_the_parent_lines() {
        let _parent_scope = super::begin_element(21);
        let parent = grid_template("[alpha] 32px [beta] 32px", metrics()).unwrap();
        super::install_tracks(true, parent);
        {
            let _child_scope = super::begin_element(21);
            let child = grid_template("[beta] 16px [gamma] 16px", metrics()).unwrap();
            super::install_tracks(true, child);
        }
        let _sibling_scope = super::begin_element(21);
        match super::resolve_named(true, "beta", 1).unwrap() {
            super::NamedPlacement::Line(index) => assert_eq!(index, 2),
            super::NamedPlacement::Named(_, _) => panic!("beta resolved as an auto repeat"),
        }
    }

    #[test]
    fn subgrid_copies_parent_rows() {
        let _parent_scope = super::begin_element(18);
        let parent = grid_template("[top] 16px [bottom] 16px", metrics()).unwrap();
        super::install_tracks(false, parent.clone());
        let _child_scope = super::begin_element(18);
        let child = super::grid_template_axis("subgrid", metrics(), false).unwrap();
        assert_eq!(child, parent);
    }

    #[test]
    fn subgrid_without_a_parent_errors() {
        let _scope = super::begin_element(12);
        let err = super::grid_template_axis("subgrid", metrics(), true).unwrap_err();
        assert!(err.to_ascii_lowercase().contains("subgrid"), "{err}");
    }

    #[test]
    fn subgrid_on_an_auto_repeat_errors() {
        let _parent_scope = super::begin_element(13);
        let parent = grid_template("repeat(auto-fit, 20px)", metrics()).unwrap();
        super::install_tracks(true, parent);
        let _child_scope = super::begin_element(13);
        let err = super::grid_template_axis("subgrid", metrics(), true).unwrap_err();
        assert!(err.to_ascii_lowercase().contains("subgrid"), "{err}");
    }

    #[test]
    fn subgrid_inside_repeat_errors() {
        let _scope = super::begin_element(14);
        let err = grid_template("repeat(2, subgrid)", metrics()).unwrap_err();
        assert!(err.to_ascii_lowercase().contains("subgrid"), "{err}");
    }

    #[test]
    fn subgrid_adds_line_names() {
        let _parent_scope = super::begin_element(15);
        let parent = grid_template("[main] 32px [side] 32px", metrics()).unwrap();
        super::install_tracks(true, parent);
        let _child_scope = super::begin_element(15);
        let child = super::grid_template_axis("subgrid [a] [b] [c]", metrics(), true).unwrap();
        super::install_tracks(true, child);
        let _grandchild_scope = super::begin_element(15);
        let line = |name: &str| match super::resolve_named(true, name, 1).unwrap() {
            super::NamedPlacement::Line(index) => index,
            super::NamedPlacement::Named(_, _) => panic!("{name} resolved as an auto repeat"),
        };
        assert_eq!(line("a"), 1);
        assert_eq!(line("side"), 2);
        assert_eq!(line("b"), 2);
        assert_eq!(line("c"), 3);
    }

    #[test]
    fn subgrid_name_count_must_match() {
        let _parent_scope = super::begin_element(16);
        let parent = grid_template("32px 32px", metrics()).unwrap();
        super::install_tracks(true, parent);
        let _child_scope = super::begin_element(16);
        let err = super::grid_template_axis("subgrid [only]", metrics(), true).unwrap_err();
        assert!(err.contains("named grid lines"), "{err}");
    }

    #[test]
    fn subgrid_rows_do_not_read_columns() {
        let _parent_scope = super::begin_element(17);
        let parent = grid_template("32px 32px", metrics()).unwrap();
        super::install_tracks(true, parent);
        let _child_scope = super::begin_element(17);
        let err = super::grid_template_axis("subgrid", metrics(), false).unwrap_err();
        assert!(err.to_ascii_lowercase().contains("subgrid"), "{err}");
    }

    #[test]
    fn bare_ident_still_requires_extraction() {
        let err = grid_template("sidebar", metrics()).unwrap_err();
        assert!(err.contains("named grid lines"), "{err}");
    }

    #[test]
    fn named_lines_are_stored() {
        let tracks = grid_template("[sidebar] 32px [end]", metrics()).unwrap();
        assert_eq!(
            tracks,
            vec![GridTemplateComponent::Repeat(GridTemplateRepetition {
                count: RepetitionCount::Count(1),
                tracks: vec![px(32.0)],
                line_names: vec![vec!["sidebar".to_string()], vec!["end".to_string()]],
            })]
        );
        assert_eq!(super::line_index(&tracks, "sidebar", 1).unwrap(), 1);
        assert_eq!(super::line_index(&tracks, "end", 1).unwrap(), 2);
        let missing = grid_template("[main] 32px [side] 32px", metrics()).unwrap();
        assert_eq!(super::line_index(&missing, "side", 1).unwrap(), 2);
        let err = super::line_index(&missing, "missing", 1).unwrap_err();
        assert!(err.contains("named grid lines"), "{err}");
    }

    #[test]
    fn glued_named_lines_are_stored() {
        let tracks = grid_template("[main] 32px[side] 32px", metrics()).unwrap();
        assert_eq!(
            tracks,
            vec![GridTemplateComponent::Repeat(GridTemplateRepetition {
                count: RepetitionCount::Count(1),
                tracks: vec![px(32.0), px(32.0)],
                line_names: vec![
                    vec!["main".to_string()],
                    vec!["side".to_string()],
                    Vec::new()
                ],
            })]
        );
        assert_eq!(super::line_index(&tracks, "main", 1).unwrap(), 1);
        assert_eq!(super::line_index(&tracks, "side", 1).unwrap(), 2);
        let tracks = grid_template("32px[side]", metrics()).unwrap();
        assert_eq!(
            tracks,
            vec![GridTemplateComponent::Repeat(GridTemplateRepetition {
                count: RepetitionCount::Count(1),
                tracks: vec![px(32.0)],
                line_names: vec![Vec::new(), vec!["side".to_string()]],
            })]
        );
    }

    #[test]
    fn keyword_and_length_tracks() {
        let metrics = metrics();
        let tracks = grid_template("auto min-content max-content 12px 50%", metrics).unwrap();
        let px_len = crate::style::length("12px", metrics).unwrap();
        let pct_len = crate::style::length("50%", metrics).unwrap();
        let fixed = if px_len.is_percent() {
            prelude::percent::<f32, TrackSizingFunction>(px_len.value())
        } else {
            prelude::length::<f32, TrackSizingFunction>(px_len.value())
        };
        let percent = if pct_len.is_percent() {
            prelude::percent::<f32, TrackSizingFunction>(pct_len.value())
        } else {
            prelude::length::<f32, TrackSizingFunction>(pct_len.value())
        };
        assert_eq!(
            tracks,
            vec![
                single(prelude::auto::<TrackSizingFunction>()),
                single(prelude::min_content::<TrackSizingFunction>()),
                single(prelude::max_content::<TrackSizingFunction>()),
                single(fixed),
                single(percent),
            ]
        );
    }
}
