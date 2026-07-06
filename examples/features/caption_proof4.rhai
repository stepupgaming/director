// Caption proof: stable text block; pre-laid slots; one active word.
// Single cue with multi-line text keeps slot allocation trivial and
// the block geometry stable across the entire 8s window.
let movie = new_director(800, 600, 30);
let scene = movie.add_scene(10.0);

// Bright video-like background to exercise the auto-adapt shadow.
let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#F2F2F2",
});

root.add_caption(#{
    source: "1\n00:00:00,000 --> 00:00:08,000\nWord by word caption\nOverlapping second line",
    font_size: 38,
    color: "#FFFFFF",
    highlight_color: "#FFD60A",
    spoken_color: "#D0D0D0",
    caption_position: "bottom",
    bottom_margin: 96,
    line_spacing: 14,
    glow_enabled: true,
    glow_blur: 4.0,
    glow_pulse_amplitude: 1.6,
    glow_pulse_hz: 2.2,
    entry_glow_ramp: 0.18,
    auto_adapt_shadow: true,
    position: "absolute",
    bottom: 0,
    left: 0,
    width: "100%",
});

movie
