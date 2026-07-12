// Minimal test for path_animate functionality
let movie = new_director(1920, 1080, 30);
let scene = movie.add_scene(2.0);

let bg = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#111111"
});

// A red circle that follows a figure-8 path
let circle = bg.add_circle(#{
    cx: 0, cy: 0, r: 40,
    fill: "#FF3366",
    position: "absolute",
    left: 0, top: 0
});

// Figure-8 path centered on screen
circle.path_animate("M 660,540 C 660,240 1260,240 1260,540 C 1260,840 660,840 660,540 C 660,240 1260,240 1260,540", 2.0, "ease_in_out");

movie
