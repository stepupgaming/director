// @snapshot: 1.0, 3.5, 6.0, 8.5
// TikTok/Reels style kinetic captions: chunked words, 2-line reserve,
// one global active word (yellow), subtle 1.04 scale pop, no jitter.
let movie = new_director(800, 600, 30);
let scene = movie.add_scene(10.0);

let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#FFFFFF",
});

root.add_caption(#{
    source: "1\n00:00:00,300 --> 00:00:02,800\nthis is how\n\n2\n00:00:02,500 --> 00:00:05,500\nhigh quality\ncaptions\n\n3\n00:00:05,200 --> 00:00:07,800\nshould feel\nlike this\n\n4\n00:00:07,500 --> 00:00:09,400\ntry it today",
    font_size: 40,
    color: "#FFFFFF",
    highlight_color: "#FFD60A",
    spoken_color: "#C7C7C7",
    caption_position: "bottom",
    bottom_margin: 96,
    line_spacing: 14,
    color_transition_duration: 0.10,
    active_pop_scale: 1.04,
    active_pop_duration: 0.08,
    line_reveal_offset_1: 0.45,
    line_reveal_duration_1: 0.20,
    glow_enabled: true,
    glow_blur: 4.0,
    auto_adapt_shadow: true,
    position: "absolute",
    bottom: 0,
    left: 0,
    width: "100%",
});

movie
