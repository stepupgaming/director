let movie = new_director(1080, 1920, 30.0);
let scene = movie.add_scene(2.0);

scene.add_progress_bar(#{
    progress: 75.0,
    label: "Rendering",
    bar_color: "#00CCFF",
    bg_color: "#333333",
    text_color: "#FFFFFF",
    width: 800.0,
    height: 24.0
});

movie
