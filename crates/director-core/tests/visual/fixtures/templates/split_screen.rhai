// @skip
let movie = new_director(640, 360, 30);
let scene = movie.add_scene(1.0);

scene.add_split_screen(#{
    left_color: "#2a2a4e",
    right_color: "#4e2a2a"
});

movie
