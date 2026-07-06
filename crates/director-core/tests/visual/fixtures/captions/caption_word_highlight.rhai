// @snapshot: 1.0, 3.0, 5.0, 7.0
let movie = new_director(800, 600, 30);
let scene = movie.add_scene(10.0);

let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#FFFFFF",
});

// Remotion / TikTok style: no pill, scale-up pop on active word, smooth
// color transitions, lightweight drop shadow + outline for legibility.
root.add_caption(#{
    source: "1\n00:00:00,000 --> 00:00:08,000\nWord by word caption\n\n2\n00:00:02,000 --> 00:00:07,000\nOverlapping second line",
    font_size: 36,
    color: "#222222",
    highlight_color: "#FFD60A",
    spoken_color: "#999999",
    outline_enabled: true,
    outline_thickness: 2.0,
    text_shadow_offset_y: 2.0,
    caption_position: "bottom",
    bottom_margin: 50,
    line_spacing: 10,
    active_scale: 1.20,
    future_opacity: 0.85,
    spoken_opacity: 0.55,
    active_opacity: 1.0,
    word_transition_duration: 0.18,
    scale_transition_duration: 0.14,
    fade_in_duration: 0.20,
    fade_out_duration: 0.30,
    auto_adapt_shadow: true,
    position: "absolute",
    bottom: 0,
    left: 0,
    width: "100%",
});

movie
