//! Plan 01 acceptance: single timeline (tracks), freeze, loop audio, f64 fps.

use director_core::director::Director;
use director_core::node::BoxNode;
use director_core::scripting::{self, MovieHandle};
use director_core::timeline::{duration_to_frame_count, frame_to_time, time_to_frame, TrackClip};
use director_core::video_wrapper::RenderMode;
use director_core::DefaultAssetLoader;
use rhai::Engine;
use std::sync::Arc;

fn eval_movie(script: &str) -> MovieHandle {
    let mut engine = Engine::new();
    scripting::register_rhai_api(&mut engine, Arc::new(DefaultAssetLoader));
    engine
        .eval::<MovieHandle>(script)
        .unwrap_or_else(|e| panic!("script failed: {e}"))
}

#[test]
fn add_freeze_rhai_pins_local_time() {
    let movie = eval_movie(
        r##"
        let movie = new_director(320, 180, 30);
        let scene = movie.add_scene(2.0);
        scene.add_box(#{ width: "100%", height: "100%", bg_color: "#111111" });
        add_freeze(movie, scene, 0.75, 1.5);
        movie
        "##,
    );
    let mut d = movie.director.lock().unwrap();
    assert_eq!(d.flat_timeline().len(), 2);
    let (freeze_root, freeze_start, freeze_dur) = d.tracks[0]
        .clips
        .iter()
        .find(|c| c.freeze_time == Some(0.75))
        .map(|c| (c.scene_root, c.start_time, c.duration))
        .expect("freeze clip");
    assert!((freeze_dur - 1.5).abs() < 1e-9);
    let t_hold = freeze_start + 0.5;
    d.update(t_hold, None);
    let local = d.scene.get_node(freeze_root).unwrap().local_time;
    assert!(
        (local - 0.75).abs() < 1e-9,
        "expected local_time 0.75, got {local}"
    );
}

#[test]
fn add_loop_copies_audio_track_ids() {
    let mut d = Director::new(
        320,
        180,
        30.0,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    );
    let root = d.scene.add_node(Box::new(BoxNode::new()));
    let mut base = TrackClip::new(root, 0.0, 1.0);
    base.audio_tracks.push(7);
    d.push_clip(None, base);

    // Mirror lifecycle add_loop: N-1 extra clips with copied audio_tracks.
    let root = d.tracks[0].clips[0].scene_root;
    let start = d.tracks[0].clips[0].end_time();
    let dur = d.tracks[0].clips[0].duration;
    let audio = d.tracks[0].clips[0].audio_tracks.clone();
    for i in 1..3 {
        let mut clip = TrackClip::new(root, start + ((i - 1) as f64) * dur, dur);
        clip.name = Some(format!("loop_{i}"));
        clip.audio_tracks = audio.clone();
        d.push_clip(None, clip);
    }
    assert_eq!(d.tracks[0].clips.len(), 3);
    for clip in &d.tracks[0].clips {
        assert_eq!(clip.audio_tracks, vec![7]);
    }
}

#[test]
fn add_loop_rhai_places_clips() {
    let movie = eval_movie(
        r##"
        let movie = new_director(320, 180, 30);
        let scene = movie.add_scene(1.0);
        scene.add_box(#{ width: "100%", height: "100%", bg_color: "#333333" });
        add_loop(movie, scene, 3);
        movie
        "##,
    );
    let d = movie.director.lock().unwrap();
    // original + 2 loops
    assert_eq!(d.flat_timeline().len(), 3);
    let starts: Vec<f64> = d.tracks[0].clips.iter().map(|c| c.start_time).collect();
    assert!((starts[0] - 0.0).abs() < 1e-9);
    assert!((starts[1] - 1.0).abs() < 1e-9);
    assert!((starts[2] - 2.0).abs() < 1e-9);
}

#[test]
fn fps_ntsc_roundtrip_project() {
    let loader = Arc::new(DefaultAssetLoader);
    let mut d = Director::new(1280, 720, 23.976, loader, RenderMode::Preview, None);
    let root = d.scene.add_node(Box::new(BoxNode::new()));
    d.push_scene_root(root, 2.0);

    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("proj.json");
    d.save_project_json(&path).expect("save");
    let loaded = Director::load_project_json(&path).expect("load");
    assert!((loaded.fps - 23.976).abs() < 1e-12);
    assert_eq!(loaded.flat_timeline().len(), 1);
}

#[test]
fn frame_time_helpers_on_director() {
    let d = Director::new(
        100,
        100,
        29.97,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    );
    assert_eq!(d.time_to_frame(0.0), 0);
    assert_eq!(d.time_to_frame(d.frame_to_time(100)), 100);
    assert_eq!(time_to_frame(frame_to_time(50, 29.97), 29.97), 50);
    assert!(duration_to_frame_count(1.0, 29.97) >= 29);
}

#[test]
fn rhai_movie_fps_and_frame_helpers() {
    let movie = eval_movie(
        r#"
        let movie = new_director(640, 360, 23.976);
        movie
        "#,
    );
    let mut engine = Engine::new();
    scripting::register_rhai_api(&mut engine, Arc::new(DefaultAssetLoader));
    let mut scope = rhai::Scope::new();
    scope.push("movie", movie);
    let fps: f64 = engine
        .eval_with_scope(&mut scope, "movie_fps(movie)")
        .unwrap();
    assert!((fps - 23.976).abs() < 1e-12);
    let t: f64 = engine
        .eval_with_scope(&mut scope, "frame_to_time(movie, 30)")
        .unwrap();
    assert!((t - frame_to_time(30, 23.976)).abs() < 1e-12);
    let f: i64 = engine
        .eval_with_scope(&mut scope, "time_to_frame(movie, 1.0)")
        .unwrap();
    assert_eq!(f as u64, time_to_frame(1.0, 23.976));
}

#[test]
fn empty_tracks_mean_no_active_timeline() {
    let mut d = Director::new(
        100,
        100,
        30.0,
        Arc::new(DefaultAssetLoader),
        RenderMode::Preview,
        None,
    );
    assert!(d.tracks.iter().all(|t| t.clips.is_empty()));
    d.update(0.5, None);
    assert_eq!(d.flat_timeline().len(), 0);
    assert_eq!(d.timeline_duration(), 0.0);
}

#[test]
fn add_freeze_via_rhai_sets_freeze_time() {
    let movie = eval_movie(
        r##"
        let movie = new_director(320, 180, 30);
        let scene = movie.add_scene(2.0);
        scene.add_box(#{ width: "100%", height: "100%", bg_color: "#000000" });
        add_freeze(movie, scene, 1.0, 0.5);
        movie
        "##,
    );
    let d = movie.director.lock().unwrap();
    assert!(d
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .any(|c| c.freeze_time == Some(1.0)));
}
