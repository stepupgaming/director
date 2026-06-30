// @snapshot: 0.0, 1.5, 3.0
let movie = new_director(300, 300, 30);
let scene = movie.add_scene(5.0);

// Flex container filling the scene and centering its child
let bg = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e",
    align_items: "center",
    justify_content: "center"
});

// Centered red box that will fade out
let box = bg.add_box(#{
    width: 150,
    height: 150,
    bg_color: "#ff3333",
    border_radius: 16.0
});

// Fade from fully opaque to fully transparent over 3 seconds
box.animate("opacity", 1.0, 0.0, 3.0, "linear");

movie
