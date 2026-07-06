// @snapshot: 1.0, 3.0, 5.0, 7.0
// Single cue with multi-line text. Stable slot allocation, one active word,
// color/glow pulse on the active word. Surrounding words never reflow.
let movie = new_director(800, 600, 30);
let scene = movie.add_scene(10.0);

let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#FFFFFF",
});

root.add_caption(#{
    source: "1\n00:00:00,000 --> 00:00:08,000\nWord by word caption\nOverlapping second line",
    font_size: 36,
    color: "#222222",
    highlight_color: "#FFD60A",
    spoken_color: "#999999",
    caption_position: "bottom",
    bottom_margin: 80,
    line_spacing: 12,
    glow_enabled: true,
    glow_blur: 3.5,
    glow_pulse_amplitude: 1.2,
    glow_pulse_hz: 2.0,
    entry_glow_ramp: 0.18,
    auto_adapt_shadow: true,
    position: "absolute",
    bottom: 0,
    left: 0,
    width: "100%",
});

movie
