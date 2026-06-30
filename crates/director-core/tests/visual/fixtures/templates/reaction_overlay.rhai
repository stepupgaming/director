// @snapshot: 0.0
let movie = new_director(640, 360, 30);
let scene = movie.add_scene(1.0);

scene.add_reaction_overlay(#{
    corner: "bottom_right",
    facecam_size: 25.0,
    border_radius: 12.0,
    border_width: 3.0,
    border_color: "#ffffff"
});

movie
