// Shaders Batch 3 — Beauty / Portrait, Atmospheric / Weather,
// Cinematic Grades, and Artistic Stylize.
//
// Each shader gets its own full-screen scene so the rendered video shows
// exactly one effect at a time.
// Rendered via: director-engine examples/features/shaders_batch3.rhai out.mp4

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
        font_size: 56.0,
        color: "#ffffff",
        font_weight: "bold"
    });
    return root;
}

// Beauty / Portrait
scene(movie, "Skin Smooth", "#ffd1dc", "skin_smooth", #{ intensity: 0.7, radius: 4.0 });
scene(movie, "Beauty Glow", "#fff0c2", "beauty_glow", #{ intensity: 0.6, threshold: 0.55 });
scene(movie, "Soft Focus", "#e6d4ff", "soft_focus", #{ amount: 0.5 });
scene(movie, "Vibrance", "#c2f0c2", "vibrance", #{ amount: 0.6 });

// Atmospheric / Weather
scene(movie, "Fog", "#c8d6e5", "fog", #{ intensity: 0.6, color: "#dce6f0" });
scene(movie, "Mist", "#dfe9f3", "mist", #{ intensity: 0.5, speed: 1.0 });
scene(movie, "Rain", "#9fb3c8", "rain", #{ intensity: 0.7, speed: 1.2 });
scene(movie, "Snow", "#eef3f8", "snow", #{ intensity: 0.7, speed: 1.0, size: 2.0 });
scene(movie, "Dust", "#e8dcc0", "dust", #{ intensity: 0.6, speed: 1.0 });
scene(movie, "Bokeh Dots", "#3a2f4a", "bokeh_dots", #{ intensity: 0.6, size: 14.0, color: "#ffd9a0" });

// Cinematic Grades
scene(movie, "Film Look", "#d8c9a8", "film_look", #{ amount: 0.65, grain: 1.0 });
scene(movie, "Orange/Teal", "#ff9e7a", "orange_teal", #{ amount: 0.6 });
scene(movie, "Cross Process", "#a8d8b0", "cross_process", #{ amount: 0.75 });
scene(movie, "Polaroid", "#f5e0c0", "polaroid", #{ amount: 0.7 });
scene(movie, "Faded Film", "#e8e4dc", "faded_film", #{ amount: 0.6 });
scene(movie, "Midnight", "#1a2238", "midnight", #{ amount: 0.65 });

// Artistic Stylize
scene(movie, "Watercolor", "#f7f3e8", "watercolor", #{ intensity: 0.75, edge: 0.9 });
scene(movie, "Crosshatch", "#fafafa", "crosshatch", #{ intensity: 0.85, density: 7.0 });
scene(movie, "Pointillism", "#f2efe6", "pointillism", #{ dot_size: 7.0, intensity: 0.9 });
scene(movie, "Mosaic", "#dcdcdc", "mosaic", #{ tile_size: 22.0 });
scene(movie, "Ink", "#ffffff", "ink", #{ threshold: 0.1, intensity: 0.9 });
scene(movie, "Pencil Color", "#f7f3ec", "pencil_color", #{ intensity: 0.8 });

movie
