// L2: Smart lower third — content-reactive width (name + title).

let movie = new_director(1280, 720, 30);
let scene = movie.add_scene(4.0);

scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1e293b"
});

let lt = scene.add_smart_lower_third(#{
    name: "Alex Rivera",
    title: "Director of Product",
    accent_color: "#38bdf8",
    bg_color: "#0f172ae6",
    margin: 40.0
});
lt.fly_in_left(0.45, 0.1, 80.0);

// Longer name should produce a wider plate than a short one would.
let lt2 = scene.add_smart_lower_third(#{
    name: "Dr. Priya Chandrasekaran-Wells",
    title: "Principal Research Scientist · Edge ML",
    accent_color: "#a78bfa",
    margin: 40.0,
    // push up so it doesn't stack on the first
});
// Reposition second via translate for the demo.
lt2.animate("translate_y", 0.0, -110.0, 0.01, "linear");
lt2.fly_in_right(0.5, 0.35, 90.0);

movie
