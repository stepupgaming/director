let movie = new_director(1080, 1920, 30.0);
let scene = movie.add_scene(2.0);

scene.add_countdown_timer(#{
    days: 1,
    hours: 12,
    minutes: 30,
    seconds: 45,
    header: "LAUNCHING IN",
    segment_bg: "#1A1A2E",
    text_color: "#FFFFFF",
    label_color: "#A0A0B0"
});

movie
