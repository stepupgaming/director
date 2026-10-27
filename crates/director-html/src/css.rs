use crate::{computed::ComputedStyle, CompileError, Diagnostic};
use lightningcss::{
    declaration::DeclarationBlock,
    media_query::{
        MediaCondition, MediaFeature, MediaFeatureComparison, MediaFeatureId, MediaFeatureName,
        MediaFeatureValue, MediaList, MediaQuery, MediaType, Operator, Qualifier,
    },
    properties::Property,
    rules::{keyframes::KeyframeSelector, style::StyleRule, supports::SupportsCondition, CssRule},
    selector::SelectorList,
    stylesheet::{ParserOptions, PrinterOptions, StyleSheet},
    traits::{IntoOwned, ToCss},
};
use scraper::{ElementRef, Selector};
use std::{borrow::Cow, collections::BTreeMap};

pub type Values = BTreeMap<String, String>;

#[derive(Debug, Clone)]
pub struct Declaration {
    pub name: String,
    pub value: String,
    /// The longhand as parsed. `Property::Unparsed` for values with `var()`.
    pub property: Property<'static>,
    pub important: bool,
}

type Rank = (bool, u32, usize);

#[derive(Debug)]
pub struct Rule {
    selector: Selector,
    specificity: u32,
    declarations: Vec<Declaration>,
}

#[derive(Debug, Clone)]
pub struct Keyframe {
    pub offset: f32,
    pub values: Values,
}

#[derive(Debug, Default)]
pub struct Css {
    rules: Vec<Rule>,
    pub keyframes: BTreeMap<String, Vec<Keyframe>>,
    pub fonts: Vec<(String, String)>,
    width: f32,
    height: f32,
}

fn expanded(property: &Property<'_>, important: bool) -> Result<Vec<Declaration>, CompileError> {
    let id = property.property_id();
    let properties: Vec<_> = id.longhands().map_or_else(
        || vec![property.clone()],
        |ids| ids.iter().filter_map(|id| property.longhand(id)).collect(),
    );
    // Unparsed shorthands containing variables are expanded after substitution.
    let properties = if properties.is_empty() {
        vec![property.clone()]
    } else {
        properties
    };
    properties
        .into_iter()
        .map(|p| {
            Ok(Declaration {
                name: p.property_id().name().to_string(),
                value: p
                    .value_to_css_string(PrinterOptions::default())
                    .map_err(|e| CompileError::Input(e.to_string()))?,
                property: p.into_owned(),
                important,
            })
        })
        .collect()
}

pub fn declarations(block: &DeclarationBlock<'_>) -> Result<Vec<Declaration>, CompileError> {
    let mut result = Vec::new();
    for (properties, important) in [
        (&block.declarations, false),
        (&block.important_declarations, true),
    ] {
        for property in properties {
            result.extend(expanded(property, important)?);
        }
    }
    Ok(result)
}

pub fn parse_inline(css: &str) -> Result<Vec<Declaration>, CompileError> {
    let block = DeclarationBlock::parse_string(css, ParserOptions::default())
        .map_err(|e| CompileError::Input(format!("CSS declaration: {e}")))?;
    declarations(&block)
}

fn selector_sources(selectors: &SelectorList<'_>) -> Result<Vec<String>, CompileError> {
    selectors
        .0
        .iter()
        .map(|selector| {
            selector
                .to_css_string(PrinterOptions::default())
                .map_err(|e| CompileError::Input(e.to_string()))
        })
        .collect()
}

// `&` is the parent selector. Paste a single parent when it stays a valid
// compound; otherwise `:is()` keeps the most specific parent's specificity.
fn expand_nested_selector(parents: &[String], child: &str) -> String {
    let mut out = String::new();
    let mut quote = None;
    let mut saw_nesting = false;
    let mut chars = child.char_indices();
    while let Some((index, c)) = chars.next() {
        if let Some(q) = quote {
            out.push(c);
            if c == '\\' {
                if let Some((_, escaped)) = chars.next() {
                    out.push(escaped);
                }
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                quote = Some(c);
                out.push(c);
            }
            '&' => {
                saw_nesting = true;
                out.push_str(&nesting_parent(parents, is_compound_start(child, index)));
            }
            _ => out.push(c),
        }
    }
    if saw_nesting {
        out
    } else {
        format!("{} {child}", nesting_parent(parents, true))
    }
}

fn nesting_parent(parents: &[String], compound_start: bool) -> String {
    if let [parent] = parents {
        if compound_start || is_simple_without_type(parent) {
            return parent.clone();
        }
    }
    format!(":is({})", parents.join(", "))
}

fn is_compound_start(selector: &str, index: usize) -> bool {
    let before = &selector[..index];
    if before.trim().is_empty() || before.ends_with(char::is_whitespace) {
        return true;
    }
    matches!(before.chars().last(), Some('>' | '+' | '~' | ',' | '('))
}

fn is_simple_without_type(selector: &str) -> bool {
    let selector = selector.trim();
    if !matches!(selector.chars().next(), Some('.' | '#' | '[' | ':')) {
        return false;
    }
    let mut depth = 0i32;
    let mut quote = None;
    let mut chars = selector.chars();
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == '\\' {
                chars.next();
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '>' | '+' | '~' | ',' if depth == 0 => return false,
            c if c.is_whitespace() && depth == 0 => return false,
            _ => {}
        }
    }
    true
}

impl Css {
    pub fn has_stylesheet_property(&self, el: ElementRef<'_>, name: &str) -> bool {
        self.rules.iter().any(|rule| {
            rule.selector.matches(&el) && rule.declarations.iter().any(|d| d.name == name)
        })
    }
    pub fn has_local_property(&self, el: ElementRef<'_>, name: &str) -> bool {
        self.has_stylesheet_property(el, name)
            || el
                .value()
                .attr("style")
                .and_then(|s| parse_inline(s).ok())
                .is_some_and(|ds| ds.iter().any(|d| d.name == name))
    }
    pub fn append(&mut self, source: &str, width: i32, height: i32) -> Result<(), CompileError> {
        self.width = width as f32;
        self.height = height as f32;
        let sheet = StyleSheet::parse(source, ParserOptions::default())
            .map_err(|e| CompileError::Input(format!("CSS stylesheet: {e}")))?;
        for rule in sheet.rules.0 {
            match rule {
                CssRule::Style(rule) => self.append_style(&[], &rule)?,
                CssRule::Keyframes(rule) => {
                    let name = rule
                        .name
                        .to_css_string(PrinterOptions::default())
                        .map_err(|e| CompileError::Input(e.to_string()))?;
                    let mut frames = Vec::new();
                    for frame in rule.keyframes {
                        let values: Values = declarations(&frame.declarations)?
                            .into_iter()
                            .filter(|d| !d.important)
                            .map(|d| (d.name, d.value))
                            .collect();
                        for selector in frame.selectors {
                            let offset = match selector {
                                KeyframeSelector::From => 0.0,
                                KeyframeSelector::To => 1.0,
                                KeyframeSelector::Percentage(p) => p.0,
                                _ => {
                                    return Err(CompileError::Input(
                                        "scroll-driven keyframes are unsupported".into(),
                                    ))
                                }
                            };
                            frames.push(Keyframe {
                                offset,
                                values: values.clone(),
                            });
                        }
                    }
                    frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
                    self.keyframes.insert(name, frames);
                }
                CssRule::FontFace(rule) => {
                    let mut family = None;
                    let mut src = None;
                    for p in rule.properties {
                        let s = p
                            .to_css_string(PrinterOptions::default())
                            .map_err(|e| CompileError::Input(e.to_string()))?;
                        if let Some((name, value)) = s.trim_end_matches(';').split_once(':') {
                            match name.trim() {
                                "font-family" => {
                                    family =
                                        Some(value.trim().trim_matches(['\'', '"']).to_string())
                                }
                                "src" => src = Some(value.trim().to_string()),
                                "font-weight" | "font-style" | "font-display" => {}
                                _ => {
                                    return Err(CompileError::Input(format!(
                                        "unsupported font descriptor: {name}"
                                    )))
                                }
                            }
                        }
                    }
                    if let (Some(family), Some(src)) = (family, src) {
                        self.fonts.push((family, src));
                    }
                }
                _ => {
                    return Err(CompileError::Input(
                        "unsupported CSS at-rule; use compile-time extraction".into(),
                    ))
                }
            }
        }
        Ok(())
    }

    fn append_style(
        &mut self,
        parents: &[String],
        rule: &StyleRule<'_>,
    ) -> Result<(), CompileError> {
        let mut child_parents = selector_sources(&rule.selectors)?;
        let decls = declarations(&rule.declarations)?;
        if parents.is_empty() {
            for selector in rule.selectors.0.iter() {
                self.push_lightning_selector(selector, &decls)?;
            }
        } else {
            for source in &mut child_parents {
                *source = expand_nested_selector(parents, source);
            }
            for source in &child_parents {
                self.push_expanded(source, &decls)?;
            }
        }
        for nested in &rule.rules.0 {
            self.append_nested(&child_parents, nested)?;
        }
        Ok(())
    }

    fn append_nested(
        &mut self,
        parents: &[String],
        rule: &CssRule<'_>,
    ) -> Result<(), CompileError> {
        match rule {
            CssRule::Style(rule) => self.append_style(parents, rule),
            CssRule::Nesting(rule) => self.append_style(parents, &rule.style),
            CssRule::NestedDeclarations(rule) => {
                let decls = declarations(&rule.declarations)?;
                for source in parents {
                    self.push_expanded(source, &decls)?;
                }
                Ok(())
            }
            // A true condition is inlined. A false one is dropped. Unknown stays an error.
            CssRule::Media(rule) => {
                if media_matches(&rule.query, self.width, self.height)? {
                    for nested in &rule.rules.0 {
                        self.append_nested(parents, nested)?;
                    }
                }
                Ok(())
            }
            CssRule::Supports(rule) => {
                if supports_matches(&rule.condition)? {
                    for nested in &rule.rules.0 {
                        self.append_nested(parents, nested)?;
                    }
                }
                Ok(())
            }
            _ => Err(CompileError::Input(
                "CSS nesting requires compile-time extraction".into(),
            )),
        }
    }

    fn push_expanded(
        &mut self,
        source: &str,
        declarations: &[Declaration],
    ) -> Result<(), CompileError> {
        if declarations.is_empty() {
            return Ok(());
        }
        let css = format!("{source} {{}}");
        let rule = CssRule::parse_string(&css, ParserOptions::default()).map_err(|e| {
            CompileError::Input(format!("unsupported selector '{source}': {e}"))
        })?;
        let CssRule::Style(rule) = rule else {
            return Err(CompileError::Input(format!(
                "unsupported selector '{source}'"
            )));
        };
        if !rule.rules.0.is_empty() {
            return Err(CompileError::Input(
                "CSS nesting requires compile-time extraction".into(),
            ));
        }
        for selector in rule.selectors.0.iter() {
            self.push_lightning_selector(selector, declarations)?;
        }
        Ok(())
    }

    fn push_lightning_selector(
        &mut self,
        selector: &lightningcss::selector::Selector<'_>,
        declarations: &[Declaration],
    ) -> Result<(), CompileError> {
        let source = selector
            .to_css_string(PrinterOptions::default())
            .map_err(|e| CompileError::Input(e.to_string()))?;
        let parsed = Selector::parse(&source).map_err(|e| {
            CompileError::Input(format!("unsupported selector '{source}': {e:?}"))
        })?;
        self.rules.push(Rule {
            selector: parsed,
            specificity: selector.specificity(),
            declarations: declarations.to_vec(),
        });
        Ok(())
    }

    /// Cascade `el` over its parent's computed style. Declarations keep the
    /// typed value parsed with the stylesheet; only values with `var()` are
    /// substituted and parsed again.
    pub fn cascade(
        &self,
        el: ElementRef<'_>,
        parent: &ComputedStyle,
    ) -> Result<ComputedStyle, CompileError> {
        let mut values = ComputedStyle::default();
        for name in parent.keys().filter(|k| inherited(k)) {
            values.copy_from(parent, name);
        }
        let inline = match el.value().attr("style") {
            Some(style) => parse_inline(style)?,
            None => Vec::new(),
        };
        let mut matched: Vec<(&Declaration, Rank)> = Vec::new();
        for (order, rule) in self.rules.iter().enumerate() {
            if rule.selector.matches(&el) {
                for d in &rule.declarations {
                    matched.push((d, (d.important, rule.specificity, order)));
                }
            }
        }
        for d in &inline {
            matched.push((d, (d.important, u32::MAX, self.rules.len())));
        }
        let mut winners: BTreeMap<String, (Rank, Cow<'_, Declaration>)> = BTreeMap::new();
        for &(d, rank) in &matched {
            if d.name.starts_with("--") {
                insert_winner(&mut winners, Cow::Borrowed(d), rank);
            }
        }
        let mut variables = Values::clone(&values);
        for (key, (_, d)) in &winners {
            variables.insert(key.clone(), d.value.clone());
        }
        // Expand variable-dependent shorthands before ranking their longhands.
        for (d, rank) in matched {
            if d.name.starts_with("--") {
                continue;
            }
            if has_var(&d.value) {
                let value = resolve_vars(&d.value, &variables, 0)?;
                for expanded in parse_inline(&format!("{}: {}", d.name, value))? {
                    insert_winner(&mut winners, Cow::Owned(expanded), rank);
                }
            } else {
                insert_winner(&mut winners, Cow::Borrowed(d), rank);
            }
        }
        for (key, (_, d)) in winners {
            match d.value.as_str() {
                "inherit" => values.copy_from(parent, &key),
                "unset" if inherited(&key) => {}
                "initial" | "unset" => values.remove(&key),
                "revert" | "revert-layer" => {
                    return Err(CompileError::Input(format!(
                        "{key}: {} is unsupported",
                        d.value
                    )))
                }
                _ => {
                    let d = d.into_owned();
                    values.set(key, d.value, Some(d.property));
                }
            }
        }
        Ok(values)
    }
}

fn media_matches(list: &MediaList<'_>, width: f32, height: f32) -> Result<bool, CompileError> {
    if list.media_queries.is_empty() {
        return Ok(true);
    }
    let mut matched = false;
    for query in &list.media_queries {
        if media_query_matches(query, width, height)? {
            matched = true;
        }
    }
    Ok(matched)
}

fn media_query_matches(
    query: &MediaQuery<'_>,
    width: f32,
    height: f32,
) -> Result<bool, CompileError> {
    let type_matches = match &query.media_type {
        MediaType::All | MediaType::Screen => true,
        MediaType::Print => false,
        MediaType::Custom(name) => {
            return Err(CompileError::Input(format!(
                "unsupported CSS media type: {name}"
            )));
        }
    };
    let condition_matches = match &query.condition {
        Some(condition) => media_condition_matches(condition, width, height)?,
        None => true,
    };
    let matches = type_matches && condition_matches;
    Ok(match query.qualifier {
        Some(Qualifier::Not) => !matches,
        Some(Qualifier::Only) | None => matches,
    })
}

fn media_condition_matches(
    condition: &MediaCondition<'_>,
    width: f32,
    height: f32,
) -> Result<bool, CompileError> {
    match condition {
        MediaCondition::Feature(feature) => media_feature_matches(feature, width, height),
        MediaCondition::Not(inner) => Ok(!media_condition_matches(inner, width, height)?),
        MediaCondition::Operation { operator, conditions } => {
            let mut matched = !matches!(operator, Operator::Or);
            for condition in conditions {
                let part = media_condition_matches(condition, width, height)?;
                matched = match operator {
                    Operator::And => matched && part,
                    Operator::Or => matched || part,
                };
            }
            Ok(matched)
        }
        MediaCondition::Unknown(_) => Err(CompileError::Input(
            "unsupported CSS media condition".into(),
        )),
    }
}

fn media_feature_matches(
    feature: &MediaFeature<'_>,
    width: f32,
    height: f32,
) -> Result<bool, CompileError> {
    let axis = |name: &MediaFeatureName<'_, MediaFeatureId>| match name {
        MediaFeatureName::Standard(MediaFeatureId::Width) => Ok(width),
        MediaFeatureName::Standard(MediaFeatureId::Height) => Ok(height),
        _ => Err(CompileError::Input(format!(
            "unsupported CSS media feature {}",
            css_text(feature)
        ))),
    };
    match feature {
        MediaFeature::Boolean { name } => Ok(axis(name)? > 0.0),
        MediaFeature::Plain { name, value } => match name {
            MediaFeatureName::Standard(MediaFeatureId::Orientation) => {
                orientation_matches(value, width, height)
            }
            MediaFeatureName::Standard(MediaFeatureId::AspectRatio) => Ok(compare_px(
                MediaFeatureComparison::Equal,
                canvas_aspect(width, height)?,
                media_ratio(value)?,
            )),
            _ => Ok(compare_px(
                MediaFeatureComparison::Equal,
                axis(name)?,
                media_px(value)?,
            )),
        },
        MediaFeature::Range {
            name,
            operator,
            value,
        } => match name {
            MediaFeatureName::Standard(MediaFeatureId::AspectRatio) => Ok(compare_px(
                *operator,
                canvas_aspect(width, height)?,
                media_ratio(value)?,
            )),
            _ => Ok(compare_px(*operator, axis(name)?, media_px(value)?)),
        },
        MediaFeature::Interval {
            name,
            start,
            start_operator,
            end,
            end_operator,
        } => {
            let (actual, start, end) = match name {
                MediaFeatureName::Standard(MediaFeatureId::AspectRatio) => (
                    canvas_aspect(width, height)?,
                    media_ratio(start)?,
                    media_ratio(end)?,
                ),
                _ => (axis(name)?, media_px(start)?, media_px(end)?),
            };
            Ok(compare_px(*start_operator, start, actual)
                && compare_px(*end_operator, actual, end))
        }
    }
}

fn orientation_matches(
    value: &MediaFeatureValue<'_>,
    width: f32,
    height: f32,
) -> Result<bool, CompileError> {
    match value {
        MediaFeatureValue::Ident(ident) if ident.eq_ignore_ascii_case("landscape") => {
            Ok(width >= height)
        }
        MediaFeatureValue::Ident(ident) if ident.eq_ignore_ascii_case("portrait") => {
            Ok(height > width)
        }
        _ => Err(CompileError::Input(
            "unsupported CSS media feature value".into(),
        )),
    }
}

fn canvas_aspect(width: f32, height: f32) -> Result<f32, CompileError> {
    let ratio = width / height;
    if ratio.is_finite() {
        Ok(ratio)
    } else {
        Err(CompileError::Input(
            "unsupported CSS media feature value".into(),
        ))
    }
}

fn media_ratio(value: &MediaFeatureValue<'_>) -> Result<f32, CompileError> {
    match value {
        MediaFeatureValue::Ratio(ratio) => {
            let ratio = ratio.0 / ratio.1;
            if ratio.is_finite() {
                Ok(ratio)
            } else {
                Err(CompileError::Input(
                    "unsupported CSS media feature value".into(),
                ))
            }
        }
        _ => Err(CompileError::Input(
            "unsupported CSS media feature value".into(),
        )),
    }
}

fn media_px(value: &MediaFeatureValue<'_>) -> Result<f32, CompileError> {
    match value {
        MediaFeatureValue::Length(length) => length
            .to_px()
            .ok_or_else(|| CompileError::Input("unsupported CSS media length".into())),
        _ => Err(CompileError::Input(
            "unsupported CSS media feature value".into(),
        )),
    }
}

fn compare_px(operator: MediaFeatureComparison, left: f32, right: f32) -> bool {
    match operator {
        MediaFeatureComparison::Equal => (left - right).abs() <= 0.01,
        MediaFeatureComparison::GreaterThan => left > right,
        MediaFeatureComparison::GreaterThanEqual => left >= right,
        MediaFeatureComparison::LessThan => left < right,
        MediaFeatureComparison::LessThanEqual => left <= right,
    }
}

fn supports_matches(condition: &SupportsCondition<'_>) -> Result<bool, CompileError> {
    match condition {
        SupportsCondition::Declaration { property_id, value } => {
            let name = property_id.name();
            let value = value.trim();
            if supports_declaration(name, value) {
                Ok(true)
            } else {
                Err(CompileError::Input(format!(
                    "unsupported CSS supports condition ({name}: {value})"
                )))
            }
        }
        SupportsCondition::Not(inner) => Ok(!supports_matches(inner)?),
        SupportsCondition::And(parts) => {
            let mut matched = true;
            for part in parts {
                matched &= supports_matches(part)?;
            }
            Ok(matched)
        }
        SupportsCondition::Or(parts) => {
            let mut matched = false;
            for part in parts {
                matched |= supports_matches(part)?;
            }
            Ok(matched)
        }
        SupportsCondition::Selector(_) | SupportsCondition::Unknown(_) => Err(CompileError::Input(
            "unsupported CSS supports condition".into(),
        )),
    }
}

fn supports_declaration(name: &str, value: &str) -> bool {
    let keyword = |items: &[&str]| items.iter().any(|item| value.eq_ignore_ascii_case(item));
    if name.eq_ignore_ascii_case("display") {
        keyword(&["block", "flex", "grid", "inline", "inline-block", "none"])
    } else if name.eq_ignore_ascii_case("background-clip") {
        keyword(&["text", "border-box", "padding-box", "content-box"])
    } else if name.eq_ignore_ascii_case("text-overflow") {
        keyword(&["ellipsis", "clip"])
    } else if name.eq_ignore_ascii_case("object-fit") {
        keyword(&["fill", "cover", "contain", "none", "scale-down"])
    } else if name.eq_ignore_ascii_case("position") {
        keyword(&["static", "relative", "absolute", "fixed", "sticky"])
    } else if name.eq_ignore_ascii_case("overflow-x")
        || name.eq_ignore_ascii_case("overflow-y")
        || name.eq_ignore_ascii_case("overflow")
    {
        keyword(&["visible", "hidden", "clip"])
    } else {
        false
    }
}

fn css_text(value: &impl ToCss) -> String {
    value
        .to_css_string(PrinterOptions::default())
        .unwrap_or_else(|_| "condition".into())
}

fn insert_winner<'a>(
    winners: &mut BTreeMap<String, (Rank, Cow<'a, Declaration>)>,
    d: Cow<'a, Declaration>,
    rank: Rank,
) {
    if winners.get(&d.name).is_none_or(|(old, _)| rank >= *old) {
        winners.insert(d.name.clone(), (rank, d));
    }
}

fn has_var(value: &str) -> bool {
    value.to_ascii_lowercase().contains("var(")
}

pub fn inherited(name: &str) -> bool {
    crate::property::is_inherited(name)
}

fn resolve_vars(value: &str, variables: &Values, depth: usize) -> Result<String, CompileError> {
    if depth > 32 {
        return Err(CompileError::Input("cyclic CSS custom property".into()));
    }
    let mut input = cssparser::ParserInput::new(value);
    let mut parser = cssparser::Parser::new(&mut input);
    let mut output = String::new();
    while !parser.is_exhausted() {
        let start = parser.position();
        let token = parser
            .next_including_whitespace_and_comments()
            .map_err(|e| CompileError::Input(format!("CSS token: {e:?}")))?
            .clone();
        if let cssparser::Token::Function(name) = token {
            let content: String = parser
                .parse_nested_block(|p| {
                    let start = p.position();
                    while p.next_including_whitespace_and_comments().is_ok() {}
                    Ok::<_, cssparser::ParseError<'_, ()>>(p.slice_from(start).to_string())
                })
                .map_err(|e| CompileError::Input(format!("CSS function: {e:?}")))?;
            if name.eq_ignore_ascii_case("var") {
                let (key, fallback) = content
                    .split_once(',')
                    .map_or((content.as_str(), None), |(k, v)| (k, Some(v)));
                let replacement = variables
                    .get(key.trim())
                    .map(String::as_str)
                    .or(fallback)
                    .ok_or_else(|| {
                        CompileError::Input(format!("undefined CSS variable {}", key.trim()))
                    })?;
                output.push_str(&resolve_vars(replacement, variables, depth + 1)?);
            } else {
                output.push_str(&name);
                output.push('(');
                output.push_str(&resolve_vars(&content, variables, depth + 1)?);
                output.push(')');
            }
        } else {
            output.push_str(parser.slice_from(start));
        }
    }
    Ok(output)
}

pub fn diagnostic(context: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        context: context.into(),
        message: message.into(),
    }
}

/// Split at the top-level separator using the CSS tokenizer, preserving functions and strings.
pub fn split(value: &str, comma: bool) -> Vec<String> {
    let mut input = cssparser::ParserInput::new(value);
    let mut p = cssparser::Parser::new(&mut input);
    let mut start = p.position();
    let mut parts = Vec::new();
    while !p.is_exhausted() {
        let before = p.position();
        let Ok(token) = p.next_including_whitespace_and_comments().cloned() else {
            break;
        };
        let separator = if comma {
            matches!(token, cssparser::Token::Comma)
        } else {
            matches!(token, cssparser::Token::WhiteSpace(_))
        };
        if separator {
            let piece = p.slice(start..before).trim();
            if !piece.is_empty() {
                parts.push(piece.into());
            }
            start = p.position();
        } else if matches!(
            token,
            cssparser::Token::Function(_)
                | cssparser::Token::ParenthesisBlock
                | cssparser::Token::SquareBracketBlock
        ) {
            let _ = p.parse_nested_block(|p| {
                while p.next_including_whitespace_and_comments().is_ok() {}
                Ok::<_, cssparser::ParseError<'_, ()>>(())
            });
        }
    }
    let piece = p.slice_from(start).trim();
    if !piece.is_empty() {
        parts.push(piece.into());
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightningcss::properties::size::Size;
    use scraper::Html;

    fn cascade(css: &str, html: &str, parent: &ComputedStyle) -> ComputedStyle {
        let mut sheet = Css::default();
        sheet.append(css, 400, 300).unwrap();
        let document = Html::parse_fragment(html);
        let div = document.select(&Selector::parse("div").unwrap()).next().unwrap();
        sheet.cascade(div, parent).unwrap()
    }

    #[test]
    fn declarations_keep_their_parsed_values() {
        let values = cascade("div { width: 12px; --gap: 4px }", "<div></div>", &Default::default());
        let width = values.computed("width").unwrap();
        assert_eq!(width.text, "12px");
        assert!(matches!(width.value, Property::Width(Size::LengthPercentage(_))));
        assert_eq!(values.get("--gap").map(String::as_str), Some("4px"));
        assert!(values.computed("--gap").is_none());
    }

    #[test]
    fn var_values_substitute_then_parse_as_longhands() {
        let values = cascade(
            "div { --w: 20px; --pad: 1px 2px; height: var(--w); padding: var(--pad) }",
            "<div style=\"--w: 30px\"></div>",
            &Default::default(),
        );
        assert_eq!(values.get("height").map(String::as_str), Some("30px"));
        assert!(matches!(
            values.computed("height").unwrap().value,
            Property::Height(Size::LengthPercentage(_))
        ));
        assert_eq!(values.get("padding-left").map(String::as_str), Some("2px"));
        assert!(matches!(
            values.computed("padding-left").unwrap().value,
            Property::PaddingLeft(_)
        ));
    }

    #[test]
    fn inherit_copies_the_parent_typed_value() {
        let parent = ComputedStyle::from_text(&[("width".to_string(), "40px".to_string())].into());
        let values = cascade("div { width: inherit }", "<div></div>", &parent);
        let width = values.computed("width").unwrap();
        assert_eq!(width.text, "40px");
        assert!(matches!(width.value, Property::Width(_)));
    }
}
