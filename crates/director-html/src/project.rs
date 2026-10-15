use crate::{
    assets::AssetStore,
    capture::compile_capture_with_assets,
    compiler::compile_with_assets,
    css::{parse_inline, Values},
    Capture, CompileError, CompileOptions, CompiledHtml, CAPTURE_FORMAT,
};
use html5ever::{ns, LocalName, QualName};
use scraper::{Html, Node, Selector};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Write, path::Path, sync::Arc};

pub const PROJECT_FORMAT: &str = "director-native-project/1";

/// Portable, editable authoring input. Native scene adapters are rebuilt on load;
/// they never pass through the core's closed set of serialized node kinds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProject {
    pub format: String,
    pub source: ProjectSource,
    pub assets: BTreeMap<String, String>,
    pub blobs: BTreeMap<String, Arc<str>>,
    #[serde(default)]
    pub edits: BTreeMap<String, ElementEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectSource {
    Html {
        html: Arc<str>,
        options: CompileOptions,
    },
    Capture {
        capture: Arc<Capture>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElementEdit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub styles: Values,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: Values,
}

impl NativeProject {
    pub fn from_file(path: &Path, mut options: CompileOptions) -> Result<Self, CompileError> {
        if !path
            .extension()
            .is_some_and(|e| e == "html" || e == "htm" || e == "json" || e == "dirproj")
        {
            return Err(CompileError::Input("native packing accepts .html, .htm, capture JSON or .dirproj; Rhai retains its core project contract".into()));
        }
        let bytes = std::fs::read(path)
            .map_err(|e| CompileError::Input(format!("input {}: {e}", path.display())))?;
        options.base_dir = path.parent().unwrap_or(Path::new(".")).into();
        let source = if path
            .extension()
            .is_some_and(|e| e == "json" || e == "dirproj")
        {
            if options.composition.is_some() {
                return Err(CompileError::Input("captures/projects retain their selected composition; select it when packing authored HTML".into()));
            }
            let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(json_error)?;
            match value.get("format").and_then(|v| v.as_str()) {
                Some(PROJECT_FORMAT) => {
                    let project: Self = serde_json::from_value(value).map_err(json_error)?;
                    project.compile()?;
                    return Ok(project);
                }
                Some(CAPTURE_FORMAT) => {
                    let mut capture: Capture = serde_json::from_value(value).map_err(json_error)?;
                    crate::capture::normalize_ids(&mut capture);
                    ProjectSource::Capture {
                        capture: Arc::new(capture),
                    }
                }
                format => {
                    return Err(CompileError::Input(format!(
                        "unsupported native input format {format:?}"
                    )))
                }
            }
        } else {
            ProjectSource::Html {
                html: Arc::from(
                    String::from_utf8(bytes)
                        .map_err(|e| CompileError::Input(format!("HTML: {e}")))?,
                ),
                options: options.clone(),
            }
        };
        let assets = Arc::new(AssetStore::local(&options.base_dir, true));
        let compiled = compile_source(&source, &BTreeMap::new(), assets.clone())?;
        compiled
            .director
            .ensure_required_fonts()
            .map_err(|e| CompileError::Input(e.to_string()))?;
        let snapshot = assets.snapshot()?;
        Ok(Self {
            format: PROJECT_FORMAT.into(),
            source,
            assets: snapshot.references,
            blobs: snapshot.blobs,
            edits: BTreeMap::new(),
        })
    }

    pub fn load(path: &Path) -> Result<Self, CompileError> {
        let bytes = std::fs::read(path)
            .map_err(|e| CompileError::Input(format!("project {}: {e}", path.display())))?;
        let project: Self = serde_json::from_slice(&bytes).map_err(json_error)?;
        project.validate_format()?;
        AssetStore::frozen(&project.assets, &project.blobs)?;
        Ok(project)
    }

    pub fn compile(&self) -> Result<CompiledHtml, CompileError> {
        self.validate_format()?;
        let assets = Arc::new(AssetStore::frozen(&self.assets, &self.blobs)?);
        let compiled = compile_source(&self.source, &self.edits, assets)?;
        compiled
            .director
            .ensure_required_fonts()
            .map_err(|e| CompileError::Input(e.to_string()))?;
        Ok(compiled)
    }

    /// Validate the complete candidate before replacing any accepted edits.
    pub fn apply_edits(
        &mut self,
        edits: BTreeMap<String, ElementEdit>,
    ) -> Result<(), CompileError> {
        let mut candidate = self.clone();
        for (id, edit) in edits {
            let previous = candidate.edits.entry(id).or_default();
            if edit.text.is_some() {
                previous.text = edit.text;
            }
            previous.styles.extend(edit.styles);
            previous.attributes.extend(edit.attributes);
        }
        candidate.compile()?;
        *self = candidate;
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<(), CompileError> {
        self.compile()?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
        serde_json::to_writer_pretty(&mut file, self).map_err(json_error)?;
        file.flush().map_err(io_error)?;
        file.as_file().sync_all().map_err(io_error)?;
        file.persist(path).map_err(|e| io_error(e.error))?;
        Ok(())
    }

    fn validate_format(&self) -> Result<(), CompileError> {
        if self.format != PROJECT_FORMAT {
            return Err(CompileError::Input(format!(
                "unsupported project format '{}'",
                self.format
            )));
        }
        Ok(())
    }
}

fn compile_source(
    source: &ProjectSource,
    edits: &BTreeMap<String, ElementEdit>,
    assets: Arc<AssetStore>,
) -> Result<CompiledHtml, CompileError> {
    match source {
        ProjectSource::Html { html, options } => {
            let html = edited_html(html, edits)?;
            compile_with_assets(&html, options.clone(), assets)
        }
        ProjectSource::Capture { capture } => {
            let mut capture = capture.as_ref().clone();
            let mut found = BTreeMap::new();
            for frame in &mut capture.frames {
                let mut frame_ids = BTreeMap::new();
                edit_captured_node(&mut frame.root, edits, &mut frame_ids)?;
                for (id, count) in frame_ids {
                    if count > 1 {
                        return Err(CompileError::Input(format!(
                            "edit target '{id}' is ambiguous in a capture frame"
                        )));
                    }
                    found.insert(id, true);
                }
            }
            for id in edits.keys() {
                if !found.contains_key(id) {
                    return Err(CompileError::Input(format!("edit target '{id}' not found")));
                }
            }
            compile_capture_with_assets(capture, assets)
        }
    }
}

fn edited_html(html: &str, edits: &BTreeMap<String, ElementEdit>) -> Result<String, CompileError> {
    if edits.is_empty() {
        return Ok(html.into());
    }
    let mut dom = Html::parse_document(html);
    for (id, edit) in edits {
        let ids: Vec<_> = dom
            .select(&Selector::parse("[id]").unwrap())
            .filter(|n| n.value().attr("id") == Some(id.as_str()))
            .map(|n| n.id())
            .collect();
        if ids.len() != 1 {
            return Err(CompileError::Input(format!(
                "edit target '{id}' must identify exactly one element, found {}",
                ids.len()
            )));
        }
        let node_id = ids[0];
        let declarations = style_edit(&edit.styles)?;
        let mut node = dom.tree.get_mut(node_id).unwrap();
        if let Node::Element(element) = node.value() {
            if !declarations.is_empty() {
                let previous = element.attr("style").unwrap_or("");
                set_attribute(element, "style", &format!("{previous};{declarations}"));
            }
            for (name, value) in &edit.attributes {
                if !matches!(
                    name.as_str(),
                    "data-start"
                        | "data-end"
                        | "data-duration"
                        | "data-fps"
                        | "data-width"
                        | "data-height"
                        | "data-media-start"
                        | "data-volume"
                        | "data-hidden"
                        | "src"
                ) {
                    return Err(CompileError::Input(format!(
                        "attribute edit '{name}' is not supported"
                    )));
                }
                set_attribute(element, name, value);
            }
        }
        if let Some(text) = &edit.text {
            let children: Vec<_> = dom
                .tree
                .get(node_id)
                .unwrap()
                .children()
                .map(|n| n.id())
                .collect();
            for child in children {
                dom.tree.get_mut(child).unwrap().detach();
            }
            dom.tree
                .get_mut(node_id)
                .unwrap()
                .append(Node::Text(scraper::node::Text {
                    text: text.as_str().into(),
                }));
        }
    }
    Ok(dom.html())
}

fn set_attribute(element: &mut scraper::node::Element, name: &str, value: &str) {
    let name = QualName::new(None, ns!(), LocalName::from(name));
    match element.attrs.binary_search_by(|(key, _)| key.cmp(&name)) {
        Ok(index) => element.attrs[index].1 = value.into(),
        Err(index) => element.attrs.insert(index, (name, value.into())),
    }
}

fn style_edit(styles: &Values) -> Result<String, CompileError> {
    let mut css = String::new();
    for (name, value) in styles {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(CompileError::Input(format!(
                "invalid edit property '{name}'"
            )));
        }
        let declaration = format!("{name}:{value}");
        let block = lightningcss::declaration::DeclarationBlock::parse_string(
            &declaration,
            Default::default(),
        )
        .map_err(|e| CompileError::Input(format!("style edit: {e}")))?;
        if block.declarations.len() != 1 || !block.important_declarations.is_empty() {
            return Err(CompileError::Input(format!(
                "edit '{name}' must contain one CSS value"
            )));
        }
        css.push_str(&format!("{declaration}!important;"));
    }
    Ok(css)
}

fn edit_captured_node(
    node: &mut crate::CapturedNode,
    edits: &BTreeMap<String, ElementEdit>,
    found: &mut BTreeMap<String, usize>,
) -> Result<(), CompileError> {
    if let Some(edit) = edits.get(&node.id) {
        *found.entry(node.id.clone()).or_default() += 1;
        if !edit.attributes.is_empty() {
            return Err(CompileError::Input(
                "captured geometry/timing is frozen; attribute edits require authored HTML".into(),
            ));
        }
        if edit.text.is_some() && node.text.is_none() {
            return Err(CompileError::Input(format!(
                "captured '{}' has no editable text",
                node.id
            )));
        }
        if node.svg.is_some()
            && edit
                .styles
                .keys()
                .any(|n| n == "color" || n == "fill" || n == "stroke")
        {
            return Err(CompileError::Input(
                "captured SVG paint is frozen in its vector markup".into(),
            ));
        }
        for name in edit.styles.keys() {
            if !matches!(
                name.as_str(),
                "color"
                    | "background-color"
                    | "opacity"
                    | "filter"
                    | "backdrop-filter"
                    | "mix-blend-mode"
                    | "border-radius"
                    | "box-shadow"
                    | "font-size"
                    | "font-weight"
                    | "font-family"
                    | "font-style"
                    | "letter-spacing"
                    | "text-align"
                    | "visibility"
                    | "clip-path"
                    | "mask-image"
            ) {
                return Err(CompileError::Input(format!(
                    "captured style edit '{name}' requires layout or animation translation"
                )));
            }
        }
        let css = style_edit(&edit.styles)?;
        for declaration in parse_inline(&css)? {
            node.styles.insert(declaration.name, declaration.value);
        }
        if let Some(text) = &edit.text {
            node.text = Some(text.clone());
        }
    }
    for child in &mut node.children {
        edit_captured_node(child, edits, found)?;
    }
    Ok(())
}

fn json_error(error: serde_json::Error) -> CompileError {
    CompileError::Input(format!("project JSON: {error}"))
}
fn io_error(error: std::io::Error) -> CompileError {
    CompileError::Input(format!("project output: {error}"))
}
