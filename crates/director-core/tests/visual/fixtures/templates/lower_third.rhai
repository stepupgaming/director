// @snapshot: 0.0
let movie = new_director(640, 180, 30);
let scene = movie.add_scene(1.0);

scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e"
});

scene.add_lower_third(#{
    text: "Subscribe for more content like this!",
    bg_color: "#ff3366",
    text_color: "#ffffff",
    text_size: 28.0,
    height: 20.0,
    accent_color: "#ffffff"
});

movie
