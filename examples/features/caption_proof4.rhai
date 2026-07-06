// Caption proof: word-by-word highlighting + multi-line support
let movie = new_director(800, 600, 30);
let scene = movie.add_scene(10.0);

// White background box as root
let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#FFFFFF",
});

// Two overlapping captions to test multi-line support
root.add_caption(#{
    source: "1\n00:00:00,000 --> 00:00:08,000\nWord by word caption\n\n2\n00:00:02,000 --> 00:00:07,000\nOverlapping second line",
    font_size: 32,
    color: "#333333",
    highlight_color: "#FF6600",
    spoken_color: "#999999",
    background_color: "#00000099",
    caption_position: "bottom",
    bottom_margin: 40,
    background_padding: 14,
    word_transition_duration: 0.2,
    fade_in_duration: 0.3,
    fade_out_duration: 0.3,
    position: "absolute",
    bottom: 0,
    left: 0,
    width: "100%",
});

movie
