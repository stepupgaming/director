// @snapshot: 0.0
let movie = new_director(640, 320, 30);
let scene = movie.add_scene(1.0);

scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#10101a"
});

scene.add_countdown_timer(#{
    header: "Event Starts In",
    days: 2,
    hours: 14,
    minutes: 35,
    seconds: 9,
    segment_bg: "#1e1e2e",
    text_color: "#ffffff",
    label_color: "#9999aa"
});

movie
