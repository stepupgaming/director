// @snapshot: 2.6, 5.2
// Stress: overlapping SRT windows — newest cue replaces older in the same position.
let movie = new_director(1080, 1920, 30);
let scene = movie.add_scene(8.0);

let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#2D2D44",
});

root.add_caption(#{
    source: "1\n00:00:00,500 --> 00:00:04,000\nfirst cue stays readable\n\n2\n00:00:02,500 --> 00:00:06,500\nsecond cue replaces first\n\n3\n00:00:05,000 --> 00:00:07,500\nthird replaces second",
    font_size: 48,
    font_family: "Poppins",
    font_weight: 700,
    safe_area: "youtube_shorts",
    pill_enabled: true,
    pill_adaptive: true,
    highlight_color: "#FFD60A",
    caption_position: "bottom",
    position: "absolute",
    bottom: 0,
    left: 0,
    width: "100%",
});

movie