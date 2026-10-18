//! Track lists for `grid-template-columns` and `grid-template-rows`.
//!
//! taffy 0.9.2 names the repetition enum `RepetitionCount`
//! (`AutoFit`, `AutoFill`, `Count`). It has no `GridTrackRepetition` type.
//! Line names live on `GridTemplateRepetition::line_names`: one set per grid line,
//! `line_names[i]` before track `i` and the last set after the final track.

use crate::style::{self, LengthParts, Metrics};
use taffy::style::{GridTemplateComponent, GridTemplateRepetition, RepetitionCount, TrackSizingFunction};

pub fn grid_template(
    source: &str,
    metrics: Metrics,
) -> Result<Vec<GridTemplateComponent<String>>, String> {
    let source = source.trim();
    if source.is_empty() || source == "none" {
        return Ok(Vec::new());
    }
    components_from_parts(parse_parts(source, metrics)?)
}

fn grid_track(source: &str, metrics: Metrics) -> Result<TrackSizingFunction, String> {
    match source {
        "auto" => Ok(taffy::prelude::auto()),
        "min-content" => Ok(taffy::prelude::min_content()),
        "max-content" => Ok(taffy::prelude::max_content()),
        _ if source.ends_with("fr") => {
            Ok(taffy::prelude::fr(style::number(source.trim_end_matches("fr"))?))
        }
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

fn parse_parts(source: &str, metrics: Metrics) -> Result<Vec<Part>, String> {
    let mut parts = Vec::new();
    for token in crate::css::split(source, false) {
        if token.eq_ignore_ascii_case("subgrid") {
            return Err("subgrid requires extraction".into());
        } else if token.starts_with('[') {
            parts.push(Part::Names(line_names(&token)?));
        } else if token.starts_with("repeat(") {
            parts.push(Part::Repeat(parse_repeat(&token, metrics)?));
        } else if bare_line_name(&token) {
            return Err("named grid lines require extraction".into());
        } else {
            parts.push(Part::Track(grid_track(&token, metrics)?));
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

fn is_custom_ident(name: &str) -> bool {
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
        if matches!(self.state, LineState::Named(_)) {
            return Err("named grid lines require extraction".into());
        }
        self.state = LineState::Named(names);
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
            return Ok(tracks.into_iter().map(GridTemplateComponent::Single).collect());
        }
        Ok(vec![GridTemplateComponent::Repeat(GridTemplateRepetition {
            count: RepetitionCount::Count(1),
            tracks,
            line_names,
        })])
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

fn tracks_from_parts(parts: Vec<Part>) -> Result<(Vec<TrackSizingFunction>, Vec<Vec<String>>), String> {
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
    let index = if at_start { 0 } else { rep.line_names.len() - 1 };
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
    fn subgrid_is_rejected() {
        let err = grid_template("subgrid", metrics()).unwrap_err();
        assert!(err.to_ascii_lowercase().contains("subgrid"), "{err}");
    }

    #[test]
    fn bare_name_requires_extraction() {
        let err = grid_template("sidebar", metrics()).unwrap_err();
        assert_eq!(err, "named grid lines require extraction");
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
