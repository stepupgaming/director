// @snapshot: 0.0
let movie = new_director(640, 240, 30);
let scene = movie.add_scene(1.0);

scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#14141e"
});

scene.add_progress_bar(#{
    label: "Loading",
    progress: 75.0,
    bar_color: "#33cc66",
    bg_color: "#333344",
    text_color: "#ffffff",
    width: 400.0,
    height: 20.0
});

movie
