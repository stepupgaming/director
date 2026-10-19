// @skip
let movie = new_director(400, 300, 30);
let scene = movie.add_scene(1.0);

scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e"
});

scene.add_watermark(#{
    text: "WATERMARK",
    color: "#ffffff",
    text_size: 18.0,
    corner: "bottom_right",
    margin: 20.0,
    opacity: 0.6
});

movie
