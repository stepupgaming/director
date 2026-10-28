//! Beat times place clips, keyframes and the composition's end on the beat
//! grid of `data-bpm` or of the music track, and `data-sfx` fires a sound at
//! a clip's start.

use director_html::{compile, CompileOptions, CompiledHtml};
use director_render::render_frame;
use std::path::{Path, PathBuf};

const GSAP: &str = "<script src='https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/gsap.min.js'></script>";
const GREEN: [u8; 4] = [0, 128, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn options() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 64,
        duration: 4.0,
        base_dir: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assets"),
        gsap_dir: Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/node_modules/gsap/dist")),
        ..Default::default()
    }
}

/// A composition root with `root` attributes holding `body`, and a green box
/// with `boxed` attributes.
fn page(root: &str, boxed: &str, body: &str) -> String {
    format!(
        "<style>body{{margin:0}}#root{{position:relative;width:64px;height:64px;background:white}}#box{{position:absolute;left:0;top:0;width:16px;height:16px;background:rgb(0,128,0)}}</style>\
         <div id='root' data-composition-id='main' data-width='64' data-height='64' {root}>\
         <div id='box' {boxed}></div>{body}</div>"
    )
}

fn timeline(times: &str) -> String {
    format!(
        "<script type='application/director-timeline+json'>{{\"format\":\"director-timeline/1\",\"tracks\":[\
         {{\"target\":\"box\",\"property\":\"opacity\",\"keyframes\":[{times}]}}]}}</script>"
    )
}

fn run(source: &str) -> CompiledHtml {
    compile(source, options()).unwrap_or_else(|e| panic!("{source}: {e}"))
}

fn refused(source: &str) -> String {
    compile(source, options()).err().expect(source).to_string()
}

fn pixel(c: &mut CompiledHtml, time: f64) -> [u8; 4] {
    let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).unwrap();
    render_frame(&mut c.director, time, &mut surface).unwrap();
    let info = skia_safe::ImageInfo::new((64, 64), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let mut pixels = vec![0; 64 * 64 * 4];
    assert!(surface.read_pixels(&info, &mut pixels, 64 * 4, (8, 8)));
    pixels[..4].try_into().unwrap()
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

#[test]
fn clips_start_and_last_on_beats() {
    // 120 BPM from 0.25 s: beat 2 is 1.25 s and one bar later is 3.25 s.
    let root = "data-bpm='120' data-beat-offset='0.25' data-duration='2bar'";
    let mut c = run(&page(root, "data-start='2b' data-duration='1bar'", ""));
    assert_eq!(c.director.timeline_duration(), 4.25);
    assert_eq!(pixel(&mut c, 1.0), WHITE);
    assert_eq!(pixel(&mut c, 2.0), GREEN);
    assert_eq!(pixel(&mut c, 3.5), WHITE);
    let grid = c.beat_grid.as_ref().unwrap();
    assert_eq!((grid.bpm, grid.beats_per_bar), (120.0, 4));

    let mut c = run(&page("data-bpm='120' data-beats-per-bar='3'", "data-start='1bar' data-end='2bar - 1b'", ""));
    assert_eq!(pixel(&mut c, 1.25), WHITE);
    assert_eq!(pixel(&mut c, 1.75), GREEN);
    assert_eq!(pixel(&mut c, 2.75), WHITE);
}

#[test]
fn seconds_stay_seconds_without_a_grid() {
    let mut c = run(&page("data-duration='3'", "data-start='1' data-duration='1'", ""));
    assert!(c.beat_grid.is_none());
    assert_eq!(pixel(&mut c, 1.5), GREEN);
    assert_eq!(pixel(&mut c, 2.5), WHITE);
}

#[test]
fn timeline_keyframes_take_beat_times() {
    let times = r#"{"time":"1b","value":0},{"time":"1bar + 0.5b","value":1},{"time":3.5,"value":0}"#;
    let c = run(&page("data-bpm='120'", "", &timeline(times)));
    let keys: Vec<f64> = c.timeline.as_ref().unwrap().tracks[0].keyframes.iter().map(|k| k.time).collect();
    assert_eq!(keys, [0.5, 2.25, 3.5]);
}

#[test]
fn gsap_reads_the_grid() {
    let script = "if (director.bpm !== 120 || director.beatsPerBar !== 4 || director.beat(2) !== 1.25 \
                  || director.bar(1) !== 2.25 || director.position(2.25) !== 4) throw new Error('grid');\
                  gsap.to('#box', {x: 40, duration: director.beat(1) - director.beat(0), delay: director.bar(1), ease: 'none'});";
    let body = format!("{GSAP}<script>{script}</script>");
    let mut c = run(&page("data-bpm='120' data-beat-offset='0.25'", "", &body));
    assert_eq!(pixel(&mut c, 2.0), GREEN);
    assert_eq!(pixel(&mut c, 3.0), WHITE);

    let body = format!("{GSAP}<script>gsap.set('#box', {{x: director.drop}});</script>");
    assert!(refused(&page("data-bpm='120'", "", &body)).contains("the music has no drop"));
}

/// 16-bit stereo WAV bytes of a drum loop at 120 BPM whose first beat is at
/// 0.25 s: kicks on 1 and 3, snares on 2 and 4 and eighth-note hats, quiet for
/// four bars and loud for four.
fn drum_loop() -> Vec<u8> {
    const RATE: usize = 48000;
    let beat = 0.5;
    let mut mono = vec![0.0f32; (0.25 + 32.0 * beat + 1.0) as usize * RATE];
    let mut seed = 1u32;
    let mut noise = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    };
    for i in 0..32 {
        let start = ((0.25 + i as f64 * beat) * RATE as f64) as usize;
        let loud = if i >= 16 { 1.0 } else { 0.25 };
        if i % 2 == 0 {
            let accent = if i % 4 == 0 { 1.0 } else { 0.6 };
            for n in 0..RATE * 15 / 100 {
                let t = n as f32 / RATE as f32;
                mono[start + n] += accent * loud * (std::f32::consts::TAU * 55.0 * t).sin() * (-t * 25.0).exp();
            }
        } else {
            for n in 0..RATE / 10 {
                mono[start + n] += loud * 0.5 * noise() * (-(n as f32) / RATE as f32 * 40.0).exp();
            }
        }
        for half in 0..2 {
            let hat = start + (half as f64 * beat / 2.0 * RATE as f64) as usize;
            let mut last = 0.0;
            for n in 0..RATE * 3 / 100 {
                let white = noise();
                mono[hat + n] += loud * 0.15 * (white - last) * (-(n as f32) / RATE as f32 * 150.0).exp();
                last = white;
            }
        }
    }
    let data: Vec<u8> = mono
        .iter()
        .flat_map(|s| {
            let s = ((s * 0.5).clamp(-1.0, 1.0) * 32767.0) as i16;
            [s.to_le_bytes(), s.to_le_bytes()].concat()
        })
        .collect();
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    for field in [16u32, 1 | 2 << 16, RATE as u32, RATE as u32 * 4, 4 | 16 << 16] {
        wav.extend_from_slice(&field.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend(data);
    wav
}

#[test]
fn the_music_track_sets_the_grid() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("loop.wav"), drum_loop()).unwrap();
    // The music starts half a second into the composition.
    let music = "<audio src='loop.wav' data-timeline-role='music' data-start='0.5' data-duration='17'></audio>";
    let body = format!("{music}{}", timeline(r#"{"time":"drop - 1b","value":0},{"time":"drop","value":1}"#));
    let source = page("data-duration='8bar'", "", &body);
    let c = compile(&source, CompileOptions { base_dir: temp.path().into(), ..options() }).unwrap();
    let grid = c.beat_grid.as_ref().unwrap();
    assert!(close(grid.bpm, 120.0, 0.5), "{}", grid.bpm);
    assert!(close(grid.time(0.0), 0.75, 0.02), "{}", grid.time(0.0));
    assert_eq!(grid.drop, Some(16.0));
    let keys: Vec<f64> = c.timeline.as_ref().unwrap().tracks[0].keyframes.iter().map(|k| k.time).collect();
    assert!(close(keys[0], 8.25, 0.03) && close(keys[1], 8.75, 0.03), "{keys:?}");
    assert!(close(c.director.timeline_duration(), 16.75, 0.05));
    let tracks: Vec<_> = c.director.audio_mixer.tracks.iter().flatten().collect();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].start_time, 0.5);
}

#[test]
fn sound_effects_fire_at_clip_starts() {
    let quiet = "<div data-start='1bar' data-sfx='tone.wav' data-sfx-volume='0.5'></div>";
    let hidden = "<div data-hidden><div data-start='2b' data-sfx='tone.wav'></div></div>";
    let c = run(&page("data-bpm='120'", "data-start='1b' data-sfx='tone.wav'", &format!("{quiet}{hidden}")));
    let tracks: Vec<_> = c.director.audio_mixer.tracks.iter().flatten().collect();
    let starts: Vec<(f64, f32)> = tracks.iter().map(|t| (t.start_time, t.volume.value_at(0.0))).collect();
    assert_eq!(starts, [(0.5, 1.0), (2.0, 0.5), (1.0, 0.0)]);
    assert!(tracks.iter().all(|t| t.source_path.as_deref() == Some("tone.wav")));
}

#[test]
fn beat_times_refuse_what_they_cannot_place() {
    let cases = [
        (page("", "data-start='2b'", ""), "beat times need data-bpm"),
        (page("data-bpm='120'", "data-start='1b' data-duration='drop'", ""), "drop names a moment"),
        (page("data-bpm='120'", "data-start='2b*3'", ""), "write beat times as terms"),
        (page("data-bpm='120'", "data-bpm='90'", ""), "data-bpm belongs on the composition root"),
        (page("data-beat-offset='0.1'", "", ""), "data-beat-offset needs data-bpm"),
        (page("data-bpm='120' data-beats-per-bar='2.5'", "", ""), "data-beats-per-bar must be a whole number"),
        (page("data-bpm='120'", "", &timeline(r#"{"time":"soon","value":0}"#)), "must be seconds or a beat time"),
    ];
    for (source, message) in cases {
        let error = refused(&source);
        assert!(error.contains(message), "{message}: {error}");
    }
    let film = "<div id='film'><div data-start='1' data-sfx='tone.wav'></div></div>\
                <script type='application/director-timeline+json'>{\"format\":\"director-timeline/1\",\"tracks\":[\
                {\"target\":\"film\",\"property\":\"time\",\"keyframes\":[{\"time\":0,\"value\":0},{\"time\":2,\"value\":1}]}]}</script>";
    let error = refused(&page("", "", film));
    assert!(error.contains("a sound effect inside a time track"), "{error}");
}
