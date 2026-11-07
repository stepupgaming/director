use director_breakdown::{analyze, ShotOptions, Transition};

use crate::ffmpeg;

/// Four sources joined by a hard cut at 2 s, a 0.8 s cross-fade from 4.2 s, a fade out,
/// half a second of black and a fade in, 30 fps, 11.5 s long. The last faded frame cuts
/// to pure black, so the black hold reads as its own shot and the fade in closes it.
#[test]
fn finds_a_cut_a_dissolve_and_a_fade() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("edit.mp4");
    let graph = concat!(
        "[0][1]concat=n=2:v=1:a=0,settb=1/30[ab];",
        "[2]fade=t=out:st=3.3:d=0.5[c];[3]fade=t=in:st=0:d=0.5[d];",
        "[ab][c]xfade=transition=fade:duration=0.8:offset=4.2,settb=1/30[abc];",
        "[abc][4][d]concat=n=3:v=1:a=0[v]",
    );
    ffmpeg(
        &[
            "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=2",
            "-f", "lavfi", "-i", "smptebars=size=320x180:rate=30:duration=3",
            "-f", "lavfi", "-i", "mandelbrot=size=320x180:rate=30,trim=duration=3.8",
            "-f", "lavfi", "-i", "rgbtestsrc=size=320x180:rate=30:duration=3",
            "-f", "lavfi", "-i", "color=c=black:size=320x180:rate=30:duration=0.5",
            "-filter_complex", graph, "-map", "[v]", "-c:v", "libx264", "-crf", "16", "-pix_fmt", "yuv420p",
        ],
        &out,
    );
    let (doc, _) = analyze(&out, &ShotOptions::default()).unwrap();
    let kinds: Vec<(Transition, f64)> = doc.shots.iter().map(|s| (s.transition_in, s.start)).collect();
    assert_eq!(doc.shots.len(), 5, "{kinds:?}");
    assert_eq!(kinds[0], (Transition::Start, 0.0));
    assert_eq!(kinds[1].0, Transition::Cut, "{kinds:?}");
    assert!((kinds[1].1 - 2.0).abs() < 0.05, "{kinds:?}");
    assert_eq!(kinds[2].0, Transition::Dissolve, "{kinds:?}");
    assert!((kinds[2].1 - 4.6).abs() < 0.25, "{kinds:?}");
    assert_eq!(kinds[3].0, Transition::Cut, "{kinds:?}");
    assert!((kinds[3].1 - 7.77).abs() < 0.1, "{kinds:?}");
    assert_eq!(kinds[4].0, Transition::Fade, "{kinds:?}");
    assert!((kinds[4].1 - 8.63).abs() < 0.1, "{kinds:?}");
    assert_eq!(doc.source.frames, 345);
    assert!(!doc.source.variable_frame_rate);
    let last = doc.shots.last().unwrap();
    assert_eq!(last.end_frame, 345);
    assert!((last.end - 11.5).abs() < 1e-9);
}

/// A slow pan over one picture changes every pixel without being a dissolve.
#[test]
fn a_pan_stays_one_shot() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("pan.mp4");
    ffmpeg(
        &[
            "-f", "lavfi", "-i", "testsrc2=size=960x540:rate=1:duration=1",
            "-vf", "loop=loop=179:size=1:start=0,fps=30,crop=640:360:x='t*60':y='t*20',setpts=N/30/TB",
            "-t", "5", "-c:v", "libx264", "-crf", "16", "-pix_fmt", "yuv420p",
        ],
        &out,
    );
    let (doc, _) = analyze(&out, &ShotOptions::default()).unwrap();
    let kinds: Vec<(Transition, f64)> = doc.shots.iter().map(|s| (s.transition_in, s.start)).collect();
    assert_eq!(doc.shots.len(), 1, "{kinds:?}");
}

/// Held slides that swap in one frame read as cuts or beats, never as one long static shot.
#[test]
fn held_slides_split() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("slides.mp4");
    let graph = "[0][1][2]concat=n=3:v=1:a=0[v]";
    ffmpeg(
        &[
            "-f", "lavfi", "-i", "color=c=0x203040:size=320x180:rate=30:duration=1.5,drawbox=x=20:y=20:w=120:h=60:color=white:t=fill",
            "-f", "lavfi", "-i", "color=c=0x203040:size=320x180:rate=30:duration=1.5,drawbox=x=160:y=90:w=140:h=70:color=yellow:t=fill",
            "-f", "lavfi", "-i", "color=c=0x203040:size=320x180:rate=30:duration=1.5,drawbox=x=40:y=100:w=240:h=60:color=red:t=fill",
            "-filter_complex", graph, "-map", "[v]", "-c:v", "libx264", "-crf", "16", "-pix_fmt", "yuv420p",
        ],
        &out,
    );
    let (doc, _) = analyze(&out, &ShotOptions::default()).unwrap();
    let splits = doc.shots.len() - 1 + doc.shots.iter().map(|s| s.beats.len()).sum::<usize>();
    assert_eq!(splits, 2, "{:?}", doc.shots);
}
