//! `add_caption` reads one strict set of keys: numbers take ints or floats,
//! every value is type checked, unknown and removed keys fail with the valid
//! list or their replacement, and a caption without cues fails. Smart caption
//! entity refs and the entity tracks survive scene export and import.

use director_core::node::caption::{balanced_breaks, is_weak_word, CaptionLine, CaptionNode, CaptionPosition, CaptionStyle};
use director_core::node::SmartCaptionNode;
use director_core::scene_export::export_scene;
use director_core::scene_import::{import_scene, ImportRegistry};
use director_core::scripting::{register_rhai_api, MovieHandle};
use director_core::video_wrapper::RenderMode;
use director_core::{DefaultAssetLoader, Director};
use rhai::Engine;
use std::sync::{Arc, Mutex};

const CUE: &str = r#"source: "1\n00:00:00,000 --> 00:00:04,000\none two three four five six seven eight""#;

fn run(props: &str) -> Result<Arc<Mutex<Director>>, String> {
    let mut engine = Engine::new();
    register_rhai_api(&mut engine, Arc::new(DefaultAssetLoader));
    let script = format!(
        r##"
        let movie = new_director(1080, 1920, 30);
        let scene = movie.add_scene(4.0);
        scene.add_caption(#{{ {props} }});
        movie
        "##
    );
    engine.eval::<MovieHandle>(&script).map(|m| m.director).map_err(|e| e.to_string())
}

fn fails(props: &str) -> String {
    match run(props) {
        Ok(_) => panic!("expected {props:?} to fail"),
        Err(e) => e,
    }
}

fn caption(d: &Director) -> (Vec<CaptionLine>, CaptionStyle) {
    d.scene
        .nodes
        .iter()
        .flatten()
        .find_map(|sn| sn.element.as_any().downcast_ref::<CaptionNode>().map(|c| (c.lines.clone(), c.style.clone())))
        .expect("a caption node")
}

#[test]
fn integer_numbers_set_the_style() {
    let d = run(&format!("{CUE}, font_size: 52, stroke_width: 3, line_spacing: 18, max_words_per_line: 3")).unwrap();
    let (_, style) = caption(&d.lock().unwrap());
    assert_eq!(style.font_size, 52.0);
    assert_eq!(style.stroke_width, 3.0);
    assert_eq!(style.line_spacing, 18.0);
    assert_eq!(style.max_words_per_line, 3);
}

#[test]
fn an_unknown_key_fails_with_the_valid_keys() {
    let err = fails(&format!("{CUE}, font_szie: 52"));
    assert!(err.contains("font_szie"), "{err}");
    assert!(err.contains("font_size") && err.contains("highlight_color"), "{err}");
}

#[test]
fn removed_and_css_keys_name_their_replacement() {
    for (key, value, hint) in [
        ("pill_adaptive", "true", "pill_color"),
        ("auto_adapt_shadow", "true", "text_shadow_color"),
        ("scale_with_canvas", "true", "frame pixels"),
        ("align", "\"center\"", "caption_position"),
        ("position", "\"absolute\"", "caption_position"),
        ("left", "0", "left_margin"),
        ("bottom", "0", "bottom_margin"),
        ("width", "\"100%\"", "layout_width_fraction"),
        ("background_color", "\"#000000AA\"", "pill_color"),
    ] {
        let err = fails(&format!("{CUE}, {key}: {value}"));
        assert!(err.contains(key) && err.contains(hint), "{key}: {err}");
    }
}

#[test]
fn a_wrong_type_or_name_fails() {
    for (props, needle) in [
        ("color: 5", "color must be a string"),
        ("color: \"white\"", "hex colour"),
        ("font_size: \"52\"", "font_size must be a number"),
        ("stroke_enabled: 1", "stroke_enabled must be true or false"),
        ("caption_position: \"middle\"", "caption_position must be"),
        ("phrase_line_mode: \"stacked\"", "phrase_line_mode must be"),
        ("safe_area: \"snapchat_story\"", "tiktok"),
        ("active_pop_scale: 1.5", "1.1"),
        ("preset: \"neon\"", "bold-outline"),
        ("size: 0.05, font_size: 40", "size or font_size"),
        ("max_lines: 0", "max_lines must be a whole number"),
        ("positions: [#{ caption_position: \"top\" }]", "needs at"),
        ("positions: [#{ at: 1.0, font_size: 40 }]", "not a placement key"),
    ] {
        let err = fails(&format!("{CUE}, {props}"));
        assert!(err.contains(needle), "{props}: {err}");
    }
}

#[test]
fn a_caption_without_cues_fails() {
    let err = fails("font_size: 52");
    assert!(err.contains("media, words, file, source or content"), "{err}");
}

#[test]
fn hex_alpha_reaches_the_pill() {
    let d = run(&format!("{CUE}, pill_color: \"#00000080\"")).unwrap();
    let (_, style) = caption(&d.lock().unwrap());
    assert!((style.pill_color.a - 128.0 / 255.0).abs() < 1e-3, "{:?}", style.pill_color);
}

#[test]
fn phrase_break_off_keeps_one_display_line() {
    let d = run(&format!("{CUE}, auto_phrase_break: false")).unwrap();
    let (lines, _) = caption(&d.lock().unwrap());
    assert_eq!(lines[0].words.len(), 8);
    assert!(lines[0].words.iter().all(|w| w.display_line_idx == 0));
    // Estimated words split the whole cue evenly again.
    let first = &lines[0].words[0];
    assert!((first.end_sec - first.start_sec - 0.5).abs() < 0.05, "{first:?}");
}

#[test]
fn smart_caption_entity_refs_survive_scene_import() {
    let mut engine = Engine::new();
    register_rhai_api(&mut engine, Arc::new(DefaultAssetLoader));
    let movie = engine
        .eval::<MovieHandle>(
            r#"
            let movie = new_director(1080, 1920, 30);
            let face = create_entity(movie, "face", "mara");
            add_entity_sample(movie, face, 0.0, 400.0, 1500.0, 200.0, 200.0, 1.0);
            let scene = movie.add_scene(2.0);
            scene.add_smart_caption(#{
                content: [#{ text: "hello there", start: 0, end: 2 }],
                entity_refs: [face.to_string()],
                sample_rate: 10,
            });
            movie
            "#,
        )
        .unwrap();
    let document = export_scene(&movie.director.lock().unwrap()).unwrap();
    let imported =
        import_scene(&document, Arc::new(DefaultAssetLoader), &ImportRegistry::core(), RenderMode::Export).unwrap();
    assert_eq!(imported.entities.len(), 1);
    let node = imported
        .scene
        .nodes
        .iter()
        .flatten()
        .find(|sn| sn.element.as_any().is::<SmartCaptionNode>())
        .expect("a smart caption");
    assert_eq!(node.entity_refs.len(), 1);
    assert!(imported.entities.contains(node.entity_refs[0]));
    assert_eq!(export_scene(&imported).unwrap(), document);
}

fn caption_node(d: &Director) -> &CaptionNode {
    d.scene
        .nodes
        .iter()
        .flatten()
        .find_map(|sn| sn.element.as_any().downcast_ref::<CaptionNode>())
        .expect("a caption node")
}

/// Display lines of the first cue, as word lists.
fn display_lines(line: &CaptionLine) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    for w in &line.words {
        if out.len() <= w.display_line_idx {
            out.resize(w.display_line_idx + 1, Vec::new());
        }
        out[w.display_line_idx].push(w.text.clone());
    }
    out
}

#[test]
fn balanced_breaks_take_the_fewest_even_lines() {
    let widths = [100.0; 6];
    let weak = [false; 6];
    // Three words fit 320 px of a 350 px column, so two lines of three.
    assert_eq!(balanced_breaks(&widths, 10.0, 350.0, 4, 1, &weak), vec![0, 0, 0, 1, 1, 1]);
    // A weak third word moves to the second line.
    let weak = [false, false, true, false, false, false];
    let lines = balanced_breaks(&widths, 10.0, 450.0, 4, 1, &weak);
    assert_eq!(lines.iter().filter(|&&l| l == 0).count(), 2, "{lines:?}");
    // One word wider than the column takes a line of its own.
    assert_eq!(balanced_breaks(&[500.0, 50.0], 10.0, 300.0, 4, 1, &[false; 2]), vec![0, 1]);
    assert!(is_weak_word("the") && !is_weak_word("the,") && !is_weak_word("billion"));
}

#[test]
fn width_breaks_fit_the_measured_column() {
    let d = run(&format!("{CUE}, font_size: 140, max_words_per_line: 8")).unwrap();
    let d = d.lock().unwrap();
    let node = caption_node(&d);
    assert!(node.box_width().is_some());
    let lines = display_lines(&node.lines[0]);
    assert!(lines.len() >= 3, "140 px words cannot fit eight to a 1080 px frame: {lines:?}");
    // The same keys with fixed word chunks keep eight words on a line.
    drop(d);
    let d = run(&format!("{CUE}, font_size: 140, max_words_per_line: 8, line_break: \"words\"")).unwrap();
    let lines = display_lines(&caption_node(&d.lock().unwrap()).lines[0]);
    assert_eq!(lines.len(), 1, "{lines:?}");
}

#[test]
fn no_line_ends_on_a_weak_word() {
    let source = r#"source: "1\n00:00:00,000 --> 00:00:04,000\nThe company is now worth six hundred billion dollars to the whole market""#;
    let d = run(&format!("{source}, preset: \"bold-outline\"")).unwrap();
    let lines = display_lines(&caption_node(&d.lock().unwrap()).lines[0]);
    for line in &lines[..lines.len() - 1] {
        assert!(!is_weak_word(line.last().unwrap()), "{lines:?}");
    }
}

#[test]
fn presets_scale_and_keys_override_them() {
    let d = run(&format!("{CUE}, preset: \"bold-outline\", highlight_color: \"#00FFFF\"")).unwrap();
    let (_, style) = caption(&d.lock().unwrap());
    assert!(style.uppercase && !style.pill_enabled);
    assert_eq!(style.font_size, 84.0);
    assert_eq!(style.highlight_color.g, 1.0);
    assert_eq!(style.highlight_color.r, 0.0);
    let d = run(&format!("{CUE}, size: 0.05")).unwrap();
    assert!((caption(&d.lock().unwrap()).1.font_size - 96.0).abs() < 1e-3);
}

#[test]
fn emphasis_words_take_a_list() {
    let d = run(&format!("{CUE}, emphasis_words: [\"three\", \"SEVEN!\"], emphasis_color: \"#FF0000\"")).unwrap();
    let (_, style) = caption(&d.lock().unwrap());
    assert_eq!(style.emphasis_words, vec!["three", "SEVEN!"]);
    assert!(director_core::node::caption::is_emphasis_word(&style, "Seven"));
    assert!(!director_core::node::caption::is_emphasis_word(&style, "four"));
}

#[test]
fn max_lines_pages_two_lines_together() {
    let d = run(&format!("{CUE}, max_words_per_line: 2, max_lines: 2, line_break: \"words\"")).unwrap();
    let mut d = d.lock().unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((1080, 1920)).unwrap();
    director_render::render_frame(&mut d, 0.1, &mut surface).unwrap();
    let one_page = caption_rect(&d).height();
    drop(d);
    let d = run(&format!("{CUE}, max_words_per_line: 2, line_break: \"words\"")).unwrap();
    let mut d = d.lock().unwrap();
    director_render::render_frame(&mut d, 0.1, &mut surface).unwrap();
    let one_line = caption_rect(&d).height();
    assert!(one_page > one_line * 1.5, "{one_page} {one_line}");
}

#[test]
fn the_placement_track_moves_the_caption_and_survives_export() {
    let d = run(&format!(
        "{CUE}, positions: [#{{ at: 2.0, caption_position: \"top\", top_margin: 200 }}]"
    ))
    .unwrap();
    let mut d = d.lock().unwrap();
    let mut surface = skia_safe::surfaces::raster_n32_premul((1080, 1920)).unwrap();
    director_render::render_frame(&mut d, 0.5, &mut surface).unwrap();
    assert_eq!(caption_node(&d).style.position, CaptionPosition::Bottom);
    director_render::render_frame(&mut d, 2.5, &mut surface).unwrap();
    assert_eq!(caption_node(&d).style.position, CaptionPosition::Top);
    assert_eq!(caption_node(&d).style.top_margin, 200.0);
    assert_eq!(caption_node(&d).base_style().position, CaptionPosition::Bottom);

    let document = export_scene(&d).unwrap();
    let imported =
        import_scene(&document, Arc::new(DefaultAssetLoader), &ImportRegistry::core(), RenderMode::Export).unwrap();
    let node = caption_node(&imported);
    assert_eq!(node.positions().len(), 1);
    assert_eq!(node.style.position, CaptionPosition::Bottom);
    assert_eq!(export_scene(&imported).unwrap(), document);
}

fn caption_rect(d: &Director) -> skia_safe::Rect {
    d.scene
        .nodes
        .iter()
        .flatten()
        .find(|sn| sn.element.as_any().is::<CaptionNode>())
        .expect("a caption node")
        .layout_rect
}
