//! HyperFrames sub-compositions: `data-composition-src` mounts another HTML
//! file into a host clip before anything else reads the document.
//!
//! The file's `<template>` contents mount, as the HyperFrames runtime clones
//! them; a file without a template mounts its head stylesheets and its body,
//! so a standalone composition also mounts. Relative URLs in the mounted
//! markup and styles are rebased onto the host's directory, and a script or
//! stylesheet the document already loads is dropped. The host keeps its
//! `data-composition-src`, so page scripts seek the file's timeline on the
//! clip's clock. A document whose only
//! composition root sits inside a `<template>` compiles that template.

use crate::assets::AssetStore;
use crate::CompileError;
use ego_tree::NodeId;
use scraper::node::Text;
use scraper::{ElementRef, Html, Node, Selector};
use std::collections::HashSet;

/// Host-level attributes a mounted root drops: the host document's root sets
/// size, frame rate and motion blur for the whole film.
const ROOT_ONLY: [&str; 5] = ["data-width", "data-height", "data-fps", "data-motion-blur", "data-shutter-angle"];

/// Mounts every `data-composition-src` of `dom`, nested ones included, and
/// unwraps a template-only document.
pub(crate) fn mount(dom: &mut Html, assets: &AssetStore) -> Result<(), CompileError> {
    unwrap_template(dom);
    let mut mounted = HashSet::new();
    let mut depth = 0;
    loop {
        let hosts: Vec<(NodeId, String)> = dom
            .select(&Selector::parse("[data-composition-src]").unwrap())
            .filter(|el| !mounted.contains(&el.id()))
            .map(|el| (el.id(), el.value().attr("data-composition-src").unwrap_or("").trim().to_string()))
            .collect();
        if hosts.is_empty() {
            return Ok(());
        }
        depth += 1;
        if depth > 8 {
            return Err(CompileError::Input("data-composition-src nests more than 8 levels deep".into()));
        }
        for (host, src) in hosts {
            mount_one(dom, host, &src, assets)?;
            mounted.insert(host);
        }
    }
}

fn mount_one(dom: &mut Html, host: NodeId, src: &str, assets: &AssetStore) -> Result<(), CompileError> {
    let el = ElementRef::wrap(dom.tree.get(host).unwrap()).unwrap();
    let id = el.value().attr("data-composition-id").map(str::to_string);
    let label = format!("data-composition-src '{src}'");
    let fail = |message: String| CompileError::Input(format!("{label}: {message}"));
    let Some(id) = id else {
        return Err(fail("the host clip needs the data-composition-id of the file's root".into()));
    };
    if src.is_empty() {
        return Err(fail("names no file".into()));
    }
    if el.children().any(|c| c.value().is_element()) {
        return Err(fail("the host clip must be empty; the file supplies its content".into()));
    }
    let bytes = assets.read(src)?;
    let source = String::from_utf8(bytes).map_err(|_| fail("is not UTF-8".into()))?;
    let sub = Html::parse_document(&source);
    let dir = src.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let template = sub.select(&Selector::parse("template").unwrap()).next();
    let content: Vec<NodeId> = match template {
        Some(template) => contents(&sub, template.id()),
        None => {
            let head = Selector::parse("head > style, head > link[rel=stylesheet]").unwrap();
            let mut nodes: Vec<NodeId> = sub.select(&head).map(|e| e.id()).collect();
            if let Some(body) = sub.select(&Selector::parse("body").unwrap()).next() {
                nodes.extend(body.children().map(|c| c.id()));
            }
            nodes
        }
    };
    let roots: Vec<NodeId> = content
        .iter()
        .filter_map(|n| ElementRef::wrap(sub.tree.get(*n).unwrap()))
        .flat_map(|e| e.descendants().filter_map(ElementRef::wrap))
        .filter(|e| e.value().attr("data-composition-id") == Some(id.as_str()))
        .map(|e| e.id())
        .collect();
    if roots.len() != 1 {
        return Err(fail(format!("the file needs exactly one root with data-composition-id '{id}' (found {})", roots.len())));
    }
    let loaded = loaded(dom);
    for node in content {
        if let Some(el) = ElementRef::wrap(sub.tree.get(node).unwrap()) {
            let key = match el.value().name() {
                "script" => el.value().attr("src").map(|s| rebase(dir, s)),
                "link" => el.value().attr("href").map(|s| rebase(dir, s)),
                _ => None,
            };
            if key.is_some_and(|key| loaded.contains(&key)) {
                continue;
            }
        }
        let copy = copy(&sub, node, dom, dir, roots[0]);
        dom.tree.get_mut(host).unwrap().append_id(copy);
    }
    Ok(())
}

/// The script sources and stylesheet links `dom` already loads.
fn loaded(dom: &Html) -> Vec<String> {
    dom.select(&Selector::parse("script[src], link[rel=stylesheet][href]").unwrap())
        .filter_map(|e| e.value().attr("src").or_else(|| e.value().attr("href")).map(str::to_string))
        .collect()
}

/// Copies the subtree at `node` of `from` into `to` as an orphan, rebasing
/// relative URLs from `dir` and dropping host-only attributes from `root`.
fn copy(from: &Html, node: NodeId, to: &mut Html, dir: &str, root: NodeId) -> NodeId {
    let source = from.tree.get(node).unwrap();
    let mut value = source.value().clone();
    if let Node::Element(el) = &mut value {
        for (name, attr) in el.attrs.iter_mut() {
            if matches!(&*name.local, "src" | "href" | "data-composition-src") {
                *attr = rebase(dir, attr).into();
            }
        }
        if node == root {
            el.attrs.retain(|(name, _)| !ROOT_ONLY.contains(&&*name.local));
        }
    }
    if let Node::Text(text) = &mut value {
        let in_style = source.parent().and_then(ElementRef::wrap).is_some_and(|p| p.value().name() == "style");
        if in_style {
            *text = Text { text: rebase_css(dir, text).into() };
        }
    }
    let id = to.tree.orphan(value).id();
    let children: Vec<NodeId> = source.children().map(|c| c.id()).collect();
    for child in children {
        let child = copy(from, child, to, dir, root);
        to.tree.get_mut(id).unwrap().append_id(child);
    }
    if let Some(el) = ElementRef::wrap(source) {
        if let Some(style) = el.value().attr("style") {
            let rebased = rebase_css(dir, style);
            if let Node::Element(el) = to.tree.get_mut(id).unwrap().value() {
                for (name, attr) in el.attrs.iter_mut() {
                    if &*name.local == "style" {
                        *attr = rebased.clone().into();
                    }
                }
            }
        }
    }
    id
}

/// `url` relative to the host when it is relative to `dir`.
fn rebase(dir: &str, url: &str) -> String {
    let url = url.trim();
    let absolute = url.is_empty()
        || url.contains("://")
        || url.starts_with("//")
        || url.starts_with('/')
        || url.starts_with('#')
        || url.starts_with("data:")
        || url.as_bytes().get(1) == Some(&b':');
    if absolute || dir.is_empty() {
        return url.to_string();
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    let mut ups = 0;
    for part in url.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                if parts.pop().is_none() {
                    ups += 1;
                }
            }
            part => parts.push(part),
        }
    }
    let mut out = "../".repeat(ups);
    out.push_str(&parts.join("/"));
    out
}

/// Rebases every relative `url(...)` in CSS text.
fn rebase_css(dir: &str, css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(at) = rest.find("url(") {
        out.push_str(&rest[..at + 4]);
        rest = &rest[at + 4..];
        let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'');
        let body = &rest[quote.map_or(0, |_| 1)..];
        let end = match quote {
            Some(q) => body.find(q),
            None => body.find(')'),
        };
        let Some(end) = end else { break };
        if let Some(q) = quote {
            out.push(q);
        }
        out.push_str(&rebase(dir, &body[..end]));
        rest = &body[end..];
    }
    out.push_str(rest);
    out
}

/// A document whose composition roots all sit inside `<template>` elements
/// compiles the first such template: its contents move into `<body>`.
fn unwrap_template(dom: &mut Html) {
    let roots = Selector::parse("[data-composition-id]").unwrap();
    let in_template = |e: &ElementRef| e.ancestors().filter_map(ElementRef::wrap).any(|a| a.value().name() == "template");
    if dom.select(&roots).any(|e| !in_template(&e)) {
        return;
    }
    let Some(template) = dom.select(&Selector::parse("template").unwrap()).find(|t| t.select(&roots).next().is_some()) else {
        return;
    };
    let template = template.id();
    let body = dom.select(&Selector::parse("body").unwrap()).next().map(|b| b.id());
    let Some(body) = body else { return };
    for child in contents(dom, template) {
        dom.tree.get_mut(child).unwrap().detach();
        dom.tree.get_mut(body).unwrap().append_id(child);
    }
    dom.tree.get_mut(template).unwrap().detach();
}

/// The nodes of a template: html5ever parses them into a fragment child.
fn contents(dom: &Html, template: NodeId) -> Vec<NodeId> {
    let node = dom.tree.get(template).unwrap();
    match node.first_child() {
        Some(fragment) if matches!(fragment.value(), Node::Fragment) => fragment.children().map(|c| c.id()).collect(),
        _ => node.children().map(|c| c.id()).collect(),
    }
}
