// One Shader Per Scene — Batch 2
//
// Each of the 18 new shaders gets its own full-screen scene so the
// rendered video shows exactly one effect at a time.
// Rendered via: director-engine examples/features/shaders_one_per_scene.rhai out.mp4

let movie = new_director(1280, 720, 30);

fn scene(m, label, color, name, params) {
    let s = m.add_scene(0.6);
    let root = s.add_box(#{
        width: "100%", height: "100%",
        flex_direction: "column",
        justify_content: "center",
        align_items: "center",
        bg_color: color
    });
    root.apply_effect(name, params);
    root.add_text(#{
        content: label,
        font_size: 64.0,
        color: "#ffffff",
        font_weight: "bold"
    });
    return root;
}

// Lens / Distortion
scene(movie, "Fisheye", "#ff6b6b", "fisheye", #{ strength: 0.6 });
scene(movie, "Bulge", "#4ecdc4", "bulge", #{ strength: 0.5, radius: 320.0 });
scene(movie, "Barrel", "#ffe66d", "barrel", #{ curvature: 0.4 });
scene(movie, "Lens Flare", "#a06cd5", "lens_flare", #{ intensity: 0.7, color: "#ffd9a0" });

// Color-Grade
scene(movie, "Noir", "#e0e0e0", "noir", #{ contrast: 1.5 });
scene(movie, "Cyberpunk", "#6bcbff", "cyberpunk", #{ intensity: 0.7 });
scene(movie, "Vintage", "#ffb86b", "vintage", #{ amount: 0.7 });
scene(movie, "Gameboy", "#9bbc0f", "gameboy", #{});
scene(movie, "Bleach Bypass", "#cfcfcf", "bleach_bypass", #{ amount: 0.8 });
scene(movie, "Teal/Orange", "#ff9e7a", "teal_orange", #{ amount: 0.7 });

// Stylized
scene(movie, "Plasma", "#58a6ff", "plasma", #{ scale: 12.0, speed: 1.5 });
scene(movie, "Hologram", "#39d0ff", "hologram", #{ intensity: 0.8 });
scene(movie, "Emboss", "#c0c0c0", "emboss", #{ angle: 45.0, strength: 2.5 });
scene(movie, "Toon", "#7ee787", "toon", #{ levels: 5.0, edge: 0.1 });
scene(movie, "Oil Paint", "#ffa657", "oil_paint", #{ block_size: 9.0, levels: 6.0 });

// Atmosphere / More
scene(movie, "Light Leak", "#ff8c42", "light_leak", #{ intensity: 0.6, color: "#ffcf8f" });
scene(movie, "Aurora", "#79f7c0", "aurora", #{ intensity: 0.6, speed: 1.2 });
scene(movie, "Sun Rays", "#ffd166", "sunrays", #{ intensity: 0.6, angle: 0.0, speed: 1.0 });

movie
