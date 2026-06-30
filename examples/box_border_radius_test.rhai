// Test BoxNode border_radius animation
let movie = new_director(1920, 1080, 30);
let scene = movie.add_scene(2.0);

let bg = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e",
    justify_content: "center",
    align_items: "center"
});

let box = bg.add_box(#{
    width: 400.0,
    height: 400.0,
    bg_color: "#ff6b6b",
    border_radius: 0.0
});

// Animate border_radius from 0 to 200 (becoming a circle)
box.animate("border_radius", 0.0, 200.0, 2.0, "ease_in_out");

movie
