// God-tier caption export demo — 9:16 vertical, pills, safe area, phrase break.
// Render: cargo run --release -p director-cli --bin director-engine -- examples/features/caption_god_tier_proof.rhai output/caption_god_tier_proof.mp4
let movie = new_director(1080, 1920, 30.0);
let scene = movie.add_scene(10.0);

let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#F0F0F0",
});

root.add_caption(#{
    source: "1\n00:00:00,300 --> 00:00:03,000\nthis is how great captions should look on any background\n\n2\n00:00:02,500 --> 00:00:06,000\nhigh quality kinetic text with pills and safe areas\n\n3\n00:00:05,500 --> 00:00:08,500\nopus clip tier readability without the subscription\n\n4\n00:00:08,200 --> 00:00:09,800\ndirector god tier captions",
    font_size: 58,
    font_family: "Poppins",
    font_weight: 700,
    uppercase: true,
    color: "#FFFFFF",
    highlight_color: "#FFD60A",
    spoken_color: "#D8D8D8",
    safe_area: "tiktok",
    pill_enabled: true,
    pill_adaptive: true,
    stroke_enabled: true,
    stroke_width: 2.5,
    glow_enabled: true,
    auto_phrase_break: true,
    max_words_per_line: 4,
    caption_position: "bottom",
    line_spacing: 18,
    color_transition_duration: 0.10,
    active_pop_scale: 1.04,
    position: "absolute",
    bottom: 0,
    left: 0,
    width: "100%",
});

movie