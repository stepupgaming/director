let movie = new_director(1080, 1920, 30.0);
let scene = movie.add_scene(2.0);

scene.add_lower_third(#{
    text: "Breaking News: Director Templates",
    bg_color: "#FF3366",
    text_color: "#FFFFFF",
    text_size: 36.0,
    height: 20.0,
    accent_color: "#FFFFFF",
    margin: 60.0
});

movie
