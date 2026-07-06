// Caption proof: Remotion / TikTok / IG style. No pill, scale-up pop on the
// active word, smooth color transitions, black outline for legibility on any bg.
let movie = new_director(800, 600, 30);
let scene = movie.add_scene(10.0);

// Bright video-like background (gray gradient look) to test shadow adapt.
let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#F5F5F5",
});

root.add_caption(#{
    source: "1\n00:00:00,000 --> 00:00:08,000\nWord by word caption\n\n2\n00:00:02,000 --> 00:00:07,000\nOverlapping second line",
    font_size: 40,
    color: "#FFFFFF",
    highlight_color: "#FFD60A",
    spoken_color: "#E5E5E5",
    outline_enabled: true,
    outline_thickness: 2.5,
    text_shadow_offset_y: 2.0,
    caption_position: "bottom",
    bottom_margin: 56,
    line_spacing: 10,
    active_scale: 1.22,
    future_opacity: 0.85,
    spoken_opacity: 0.50,
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
