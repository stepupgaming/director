let movie = new_director(1080, 1920, 30);
let scene = movie.add_scene(2.0);

// Add a main background so the facecam overlay has something to sit on
scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#2A2A4E"
});

// Reaction overlay with no images - should still show the facecam container
scene.add_reaction_overlay(#{
    corner: "bottom_right",
    facecam_size: 30.0,
    facecam_margin: 20.0,
    border_radius: 16.0,
    border_width: 3.0,
    border_color: "#FFFFFF"
});

movie
