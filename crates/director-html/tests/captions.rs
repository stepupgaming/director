//! `<captions>` compiles a transcript into one core caption node: kebab-case
//! attributes are the caption key table, presets apply first, `<position>`
//! children are the placement track, word times follow the composition clock,
//! and anything unknown fails with the valid names.

use base64::Engine as _;
use director_core::node::caption::{CaptionNode, CaptionPosition};
use director_core::scene_export::export_scene;
use director_html::{compile, CompileOptions, CompiledHtml};
use director_scene::{NodeBody, SceneDocument};

const SRT: &str = "1\n00:00:00,000 --> 00:00:02,000\nsix hundred billion dollars\n\n2\n00:00:02,000 --> 00:00:04,000\nin one quarter\n";
const WORDS: &str = r#"[{"word":"Hello","start":0.0,"end":0.4},{"word":"world.","start":0.4,"end":0.9},{"word":"Again","start":1.5,"end":2.0}]"#;

fn data(text: &str) -> String {
    format!("data:text/plain;base64,{}", base64::engine::general_purpose::STANDARD.encode(text))
}

fn build(html: &str) -> Result<CompiledHtml, String> {
    compile(html, CompileOptions { width: 1080, height: 1920, duration: 6.0, ..Default::default() }).map_err(|e| e.to_string())
}

fn captions(compiled: &CompiledHtml) -> Vec<&CaptionNode> {
    compiled
        .director
        .scene
        .nodes
        .iter()
        .flatten()
        .filter_map(|n| n.element.as_any().downcast_ref::<CaptionNode>())
        .collect()
}

fn one(html: &str) -> CompiledHtml {
    let compiled = build(html).unwrap();
    assert_eq!(captions(&compiled).len(), 1, "one caption node");
    compiled
}

fn node(compiled: &CompiledHtml) -> &CaptionNode {
    captions(compiled)[0]
}

fn fails(html: &str) -> String {
    match build(html) {
        Ok(_) => panic!("expected {html:?} to fail"),
        Err(e) => e,
    }
}

#[test]
fn attributes_are_the_caption_keys_in_kebab_case() {
    let node_html = one(&format!(
        "<captions src='{}' font-size='72px' highlight-color='#FF0000' max-lines='2' emphasis-words='billion, quarter' uppercase></captions>",
        data(SRT)
    ));
    let node = node(&node_html);
    assert_eq!(node.lines.len(), 2);
    assert_eq!(node.style.font_size, 72.0);
    assert_eq!(node.style.max_lines, 2);
    assert!(node.style.uppercase);
    assert_eq!(node.style.emphasis_words, ["billion", "quarter"]);
    assert_eq!(node.style.highlight_color, director_core::types::Color::new(1.0, 0.0, 0.0, 1.0));
}

#[test]
fn a_preset_applies_first_and_keys_override_it() {
    let preset_html = one(&format!("<captions src='{}' preset='pop'></captions>", data(SRT)));
    let preset = node(&preset_html);
    let both_html = one(&format!("<captions src='{}' preset='pop' font-size='50'></captions>", data(SRT)));
    let both = node(&both_html);
    assert_ne!(preset.style.font_size, 50.0);
    assert_eq!(both.style.font_size, 50.0);
    assert_eq!(both.style.stroke_width, preset.style.stroke_width);
}

#[test]
fn size_is_a_fraction_of_the_frame_height() {
    let node_html = one(&format!("<captions src='{}' size='0.05'></captions>", data(SRT)));
    let node = node(&node_html);
    assert!((node.style.font_size - 96.0).abs() < 0.01, "{}", node.style.font_size);
}

#[test]
fn a_word_transcript_groups_into_cues_by_the_cue_rules() {
    let default_html = one(&format!("<captions src='{}'></captions>", data(WORDS)));
    let default = node(&default_html);
    assert_eq!(default.lines.len(), 2, "the sentence end and the gap split the cue");
    let joined_html = one(&format!(
        "<captions src='{}' break-on-sentence='false' max-cue-gap='1'></captions>",
        data(WORDS)
    ));
    let joined = node(&joined_html);
    assert_eq!(joined.lines.len(), 1);
    assert_eq!(joined.lines[0].words.len(), 3);
}

#[test]
fn word_times_follow_the_composition_clock() {
    let node_html = one(&format!(
        "<div data-composition-id='root' data-duration='6'><div data-composition-id='late' data-start='1.5' data-duration='4'><captions src='{}'></captions></div></div>",
        data(SRT)
    ));
    let node = node(&node_html);
    assert_eq!(node.lines[0].start_sec, 1.5);
    assert_eq!(node.lines[1].end_sec, 5.5);
    assert!(node.lines[0].words.iter().all(|w| w.start_sec >= 1.5));
}

#[test]
fn position_children_are_the_placement_track() {
    let node_html = one(&format!(
        "<captions src='{}' bottom-margin='200'>\n  <position at='2s' caption-position='top' top-margin='300'></position>\n</captions>",
        data(SRT)
    ));
    let node = node(&node_html);
    let track = node.positions();
    assert_eq!(track.len(), 1);
    assert_eq!(track[0].at, 2.0);
    assert_eq!(track[0].position, CaptionPosition::Top);
    assert_eq!(track[0].top_margin, 300.0);
    assert_eq!(track[0].bottom_margin, 200.0, "unset placement keys keep the caption's value");
}

#[test]
fn unknown_attributes_fail_with_the_valid_names() {
    let e = fails(&format!("<captions src='{}' pill-adaptive='true'></captions>", data(SRT)));
    assert!(e.contains("'pill-adaptive' is not a captions attribute") && e.contains("highlight-color"), "{e}");
    let e = fails(&format!("<captions src='{}' preset='neon'></captions>", data(SRT)));
    assert!(e.contains("karaoke"), "{e}");
    let e = fails(&format!("<captions src='{}' font-size='big'></captions>", data(SRT)));
    assert!(e.contains("font_size must be a number"), "{e}");
    let e = fails(&format!("<captions src='{}'><position at='1' color='#fff'></position></captions>", data(SRT)));
    assert!(e.contains("<position> takes at, caption-position"), "{e}");
    let e = fails(&format!("<captions src='{}'><span>hi</span></captions>", data(SRT)));
    assert!(e.contains("only <position"), "{e}");
    let e = fails("<captions></captions>");
    assert!(e.contains("captions need src"), "{e}");
    let e = fails(&format!("<captions src='{}'></captions>", data("   ")));
    assert!(e.contains("no caption cues"), "{e}");
}

#[test]
fn css_on_captions_points_to_the_attributes() {
    let e = fails(&format!("<style>captions{{color:red}}</style><captions src='{}'></captions>", data(SRT)));
    assert!(e.contains("CSS color does not style captions") && e.contains("docs/reference/captions.md"), "{e}");
    let e = fails(&format!("<captions id='cc' style='bottom:40px' src='{}'></captions>", data(SRT)));
    assert!(e.starts_with("#cc: CSS bottom"), "{e}");
}

#[test]
fn captions_export_as_a_caption_body() {
    let compiled = build(&format!(
        "<div style='width:100px;height:100px;background:blue'></div><captions id='cc' src='{}' preset='karaoke'></captions>",
        data(SRT)
    ))
    .unwrap();
    let document = export_scene(&compiled.director).unwrap();
    let parsed = SceneDocument::from_json(&document.to_json().unwrap()).unwrap();
    assert_eq!(parsed.nodes.iter().filter(|n| matches!(n.body, NodeBody::Caption(_))).count(), 1);
}

fn caption_rect(html: &str, time: f64) -> skia_safe::Rect {
    let mut compiled = build(html).unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((1080, 1920)).unwrap();
    director_render::render_frame(&mut compiled.director, time, &mut surface).unwrap();
    compiled
        .director
        .scene
        .nodes
        .iter()
        .flatten()
        .find(|n| n.element.as_any().is::<CaptionNode>())
        .map(|n| n.layout_rect)
        .unwrap()
}

#[test]
fn captions_sit_on_the_frame_at_their_anchor() {
    let page = |extra: &str| {
        format!(
            "<style>body{{margin:0}}#stage{{position:relative;width:100%;height:100%}}</style><div id='stage' data-composition-id='r' data-width='1080' data-height='1920'><div style='height:200px'></div><captions src='{}' {extra}><position at='3' caption-position='center'></position></captions></div>",
            data(SRT)
        )
    };
    let bottom = caption_rect(&page(""), 1.0);
    assert!((bottom.bottom - (1920.0 - 110.0)).abs() < 1.0, "{bottom:?}");
    let center = caption_rect(&page(""), 3.5);
    assert!((center.center_y() - 960.0).abs() < 2.0, "{center:?}");
    assert!(center.height() < 200.0, "the box holds the block, {center:?}");
}
