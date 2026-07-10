let movie = new_director(1080, 1920, 30.0);
let scene = movie.add_scene(2.0);

scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0A0A12"
});

scene.add_watermark(#{
    text: "@DirectorApp",
    corner: "bottom_right",
    color: "#FFFFFF",
    text_size: 18.0,
    opacity: 0.6,
    margin: 100.0
});

movie
