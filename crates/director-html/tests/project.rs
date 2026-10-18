use director_core::systems::renderer::render_frame;
use director_html::{CompileOptions, ElementEdit, NativeProject};
use std::{collections::BTreeMap, path::Path, process::Command};

fn pixels(compiled: &mut director_html::CompiledHtml, time: f64) -> Vec<u8> {
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    render_frame(&mut compiled.director, time, surface.canvas()).unwrap();
    let info = skia_safe::ImageInfo::new(
        (64, 64),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut bytes = vec![0; 64 * 64 * 4];
    assert!(surface.read_pixels(&info, &mut bytes, 64 * 4, (0, 0)));
    bytes
}

fn source(input: &Path) -> std::path::PathBuf {
    std::fs::write(input.join("style.css"), "body{margin:0}#box{width:64px;height:64px;background:red;animation:fade 2s linear both}@keyframes fade{from{opacity:.2}to{opacity:1}}").unwrap();
    let path = input.join("scene.html");
    std::fs::write(&path, "<link rel='stylesheet' href='style.css'><main id='stage' data-composition-id='main' data-width='64' data-height='64' data-fps='30' data-duration='2'><div id='box'></div></main>").unwrap();
    path
}

#[test]
fn portable_project_retains_assets_motion_and_seek_order_without_originals() {
    let original = tempfile::tempdir().unwrap();
    let path = source(original.path());
    let mut native = director_html::compile_file(&path, Default::default()).unwrap();
    let expected: Vec<_> = [0.0, 0.5, 1.5, 0.5]
        .into_iter()
        .map(|t| pixels(&mut native, t))
        .collect();
    drop(native);
    let project = NativeProject::from_file(&path, Default::default()).unwrap();
    assert!(project.assets.contains_key("style.css"));
    assert!(project
        .assets
        .contains_key("director:fonts/Roboto-Regular.ttf"));
    original.close().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let path = destination.path().join("scene.dirproj");
    project.save(&path).unwrap();
    let loaded = NativeProject::load(&path).unwrap();
    let mut compiled = loaded.compile().unwrap();
    assert!(compiled.dependency_paths().is_empty());
    assert!(compiled.element_ids.contains_key("box"));
    assert_eq!(compiled.director.timeline_duration(), 2.0);
    for (index, time) in [0.0, 0.5, 1.5, 0.5].into_iter().enumerate() {
        assert_eq!(pixels(&mut compiled, time), expected[index]);
    }
    let json = serde_json::to_value(loaded).unwrap();
    assert!(json["source"]["options"].get("base_dir").is_none());
}

#[test]
fn validated_edits_change_paint_and_timing_and_reject_invalid_candidates() {
    let original = tempfile::tempdir().unwrap();
    let path = source(original.path());
    let mut project = NativeProject::from_file(&path, Default::default()).unwrap();
    let edit = ElementEdit {
        styles: BTreeMap::from([
            ("background-color".into(), "blue".into()),
            ("animation".into(), "none".into()),
        ]),
        ..Default::default()
    };
    project
        .apply_edits(BTreeMap::from([("box".into(), edit)]))
        .unwrap();
    project
        .apply_edits(BTreeMap::from([(
            "stage".into(),
            ElementEdit {
                attributes: BTreeMap::from([("data-duration".into(), "3".into())]),
                ..Default::default()
            },
        )]))
        .unwrap();
    let mut compiled = project.compile().unwrap();
    assert_eq!(compiled.director.timeline_duration(), 3.0);
    assert_eq!(
        &pixels(&mut compiled, 0.5)[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 4],
        &[0, 0, 255, 255]
    );
    let accepted = serde_json::to_value(&project).unwrap();
    for edits in [
        BTreeMap::from([(
            "missing".into(),
            ElementEdit {
                text: Some("oops".into()),
                ..Default::default()
            },
        )]),
        BTreeMap::from([(
            "box".into(),
            ElementEdit {
                styles: BTreeMap::from([("transform-origin".into(), "50% 50% 10%".into())]),
                ..Default::default()
            },
        )]),
        BTreeMap::from([(
            "box".into(),
            ElementEdit {
                styles: BTreeMap::from([("color".into(), "red;display:none".into())]),
                ..Default::default()
            },
        )]),
    ] {
        assert!(project.apply_edits(edits).is_err());
        assert_eq!(serde_json::to_value(&project).unwrap(), accepted);
    }
    let output = original.path().join("edited.dirproj");
    project.save(&output).unwrap();
    assert_eq!(
        pixels(
            &mut NativeProject::load(&output).unwrap().compile().unwrap(),
            0.5
        ),
        pixels(&mut compiled, 0.5)
    );
}

#[test]
fn explicit_fonts_images_and_literal_text_edits_survive_portable_loading() {
    let original = tempfile::tempdir().unwrap();
    let mut image = skia_safe::surfaces::raster_n32_premul((8, 8)).unwrap();
    image.canvas().clear(skia_safe::Color::RED);
    let png = image
        .image_snapshot()
        .encode(None, skia_safe::EncodedImageFormat::PNG, 100)
        .unwrap();
    for name in ["a.png", "b.png"] {
        std::fs::write(original.path().join(name), png.as_bytes()).unwrap();
    }
    let font = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../director-core/assets/fonts/Roboto-Regular.ttf");
    std::fs::copy(font, original.path().join("custom.ttf")).unwrap();
    let html = original.path().join("scene.html");
    std::fs::write(&html,"<style>@font-face{font-family:Custom;src:url('custom.ttf')}body{margin:0}#title{font-family:Custom;font-size:12px;color:white}img{width:8px;height:8px}</style><main data-composition-id='main' data-width='64' data-height='64' data-duration='1'><img src='a.png'><img src='b.png'><p id='title'>before</p></main>").unwrap();
    let mut project = NativeProject::from_file(&html, Default::default()).unwrap();
    assert_eq!(project.assets["a.png"], project.assets["b.png"]);
    assert_eq!(
        project.assets["custom.ttf"],
        project.assets["director:fonts/Roboto-Regular.ttf"]
    );
    let before = pixels(&mut project.compile().unwrap(), 0.0);
    project
        .apply_edits(BTreeMap::from([(
            "title".into(),
            ElementEdit {
                text: Some("<svg><foreignObject>literal</foreignObject></svg>".into()),
                ..Default::default()
            },
        )]))
        .unwrap();
    let expected = pixels(&mut project.compile().unwrap(), 0.0);
    assert!(
        before != expected,
        "text edit did not change visible pixels"
    );
    original.close().unwrap();
    assert_eq!(expected, pixels(&mut project.compile().unwrap(), 0.0));
}

#[test]
fn project_integrity_and_format_errors_are_explicit() {
    let original = tempfile::tempdir().unwrap();
    let mut project =
        NativeProject::from_file(&source(original.path()), Default::default()).unwrap();
    let digest = project.assets["style.css"].clone();
    let blob = project.blobs.remove(&digest).unwrap();
    assert!(project
        .compile()
        .err()
        .unwrap()
        .to_string()
        .contains("missing"));
    project.blobs.insert(digest.clone(), "ZmFrZQ==".into());
    assert!(project
        .compile()
        .err()
        .unwrap()
        .to_string()
        .contains("SHA-256"));
    project.blobs.insert(digest, blob);
    project.format = "director-native-project/999".into();
    assert!(project
        .compile()
        .err()
        .unwrap()
        .to_string()
        .contains("unsupported project format"));
    let script = original.path().join("scene.rhai");
    std::fs::write(&script, "new_director(64,64,30)").unwrap();
    assert!(NativeProject::from_file(&script, Default::default()).is_err());
}

#[test]
fn sampled_capture_remains_editable_and_inspectable_after_round_trip() {
    let original = tempfile::tempdir().unwrap();
    let path = original.path().join("capture.json");
    let capture = serde_json::json!({"format":"director-html-capture/1","width":64,"height":64,"fps":30,"duration":1,"sample_rate":2,"frames":[
        {"time":0,"root":{"tag":"p","id":"title","rect":[0,0,64,64],"styles":{"font-size":"12px","background-color":"red"},"text":"before"}},
        {"time":0.5,"root":{"tag":"p","id":"title","rect":[0,0,64,64],"styles":{"font-size":"12px","background-color":"blue"},"text":"before"}}
    ]});
    std::fs::write(&path, serde_json::to_vec(&capture).unwrap()).unwrap();
    let mut project = NativeProject::from_file(&path, Default::default()).unwrap();
    project
        .apply_edits(BTreeMap::from([(
            "title".into(),
            ElementEdit {
                text: Some("after <&>".into()),
                ..Default::default()
            },
        )]))
        .unwrap();
    original.close().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let path = destination.path().join("capture.dirproj");
    project.save(&path).unwrap();
    let mut compiled = NativeProject::load(&path).unwrap().compile().unwrap();
    let source = compiled.source_inspection().unwrap();
    assert_eq!(source.at(0.5)[0].text.as_deref(), Some("after <&>"));
    assert_eq!(source.at(0.0)[0].bounds, [0.0, 0.0, 64.0, 64.0]);
    let late = pixels(&mut compiled, 0.5);
    pixels(&mut compiled, 0.0);
    assert_eq!(late, pixels(&mut compiled, 0.5));
    assert!(project
        .apply_edits(BTreeMap::from([(
            "title".into(),
            ElementEdit {
                styles: BTreeMap::from([("width".into(), "100px".into())]),
                ..Default::default()
            }
        )]))
        .is_err());
}

#[test]
fn anonymous_captured_elements_have_distinct_editable_paths() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("capture.json");
    let capture = serde_json::json!({"format":"director-html-capture/1","width":64,"height":64,"fps":30,"duration":1,"sample_rate":1,"frames":[{"time":0,"root":{"tag":"main","id":"stage","rect":[0,0,64,64],"styles":{},"children":[
        {"tag":"p","id":"div","rect":[0,0,64,32],"styles":{"font-size":"12px"},"text":"first"},
        {"tag":"p","id":"div","rect":[0,32,64,32],"styles":{"font-size":"12px"},"text":"second"}
    ]}}]});
    std::fs::write(&path, serde_json::to_vec(&capture).unwrap()).unwrap();
    let mut project = NativeProject::from_file(&path, Default::default()).unwrap();
    let source = project.compile().unwrap().source_inspection().unwrap();
    let first = source.at(0.0)[1].id.clone();
    let second = source.at(0.0)[2].id.clone();
    assert_ne!(first, second);
    assert_eq!(source.at(0.0)[0].id, "stage");
    project
        .apply_edits(BTreeMap::from([(
            first.clone(),
            ElementEdit {
                text: Some("edited".into()),
                ..Default::default()
            },
        )]))
        .unwrap();
    let compiled = project.compile().unwrap();
    let edited = compiled.source_inspection().unwrap();
    assert_eq!(edited.at(0.0)[1].id, first);
    assert_eq!(edited.at(0.0)[1].text.as_deref(), Some("edited"));
    assert_eq!(edited.at(0.0)[2].text.as_deref(), Some("second"));
}

#[test]
fn bundled_video_and_audio_outlive_the_original_files_and_project_value() {
    let original = tempfile::tempdir().unwrap();
    for args in [
        vec![
            "-f",
            "lavfi",
            "-i",
            "color=blue:s=64x64:r=30:d=0.6",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "video.mp4",
        ],
        vec![
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=0.6",
            "audio.wav",
        ],
    ] {
        let result = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .current_dir(original.path())
            .output()
            .expect("FFmpeg required");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let html = original.path().join("media.html");
    std::fs::write(&html,"<style>body{margin:0}video{width:64px;height:64px}</style><main data-composition-id='media' data-width='64' data-height='64' data-duration='.6'><video src='video.mp4' muted></video><audio src='audio.wav'></audio></main>").unwrap();
    let project = NativeProject::from_file(&html, CompileOptions::default()).unwrap();
    original.close().unwrap();
    let mut compiled = project.compile().unwrap();
    drop(project);
    assert_eq!(
        &pixels(&mut compiled, 0.2)[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 4],
        &[0, 0, 254, 255]
    );
    let samples = compiled.director.mix_audio(4800, 0.1);
    assert_eq!(samples.len(), 9600);
    assert!(samples.iter().any(|s| s.abs() > 0.01));
    let mut director = compiled.director;
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    render_frame(&mut director, 0.1, surface.canvas()).unwrap();
}
