// Shader Contact Sheet — Batch 2
//
// One frame, 6x3 grid, every new shader applied to a labeled card.
// Rendered via: preview_frame -t 0 -o shaders_contact.png

let movie = new_director(1280, 720, 30);
let scene = movie.add_scene(0.5);
let root = scene.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "column",
    justify_content: "center",
    align_items: "center",
    bg_color: "#05060a"
});

fn card(parent, label, color) {
    let c = parent.add_box(#{
        width: 188.0, height: 200.0,
        flex_direction: "column",
        justify_content: "center",
        align_items: "center",
        gap: 8.0,
        bg_color: color,
        border_radius: 12.0
    });
    c.add_text(#{
        content: label,
        font_size: 16.0,
        color: "#ffffff",
        font_weight: "bold"
    });
    return c;
}

// Row 1: Lens / Distortion
let r1 = root.add_box(#{
    width: "100%", height: "33%",
    flex_direction: "row", justify_content: "space_evenly", align_items: "center"
});
let c1 = card(r1, "Fisheye", "#ff6b6b"); c1.apply_effect("fisheye", #{ strength: 0.6 });
let c2 = card(r1, "Bulge", "#4ecdc4"); c2.apply_effect("bulge", #{ strength: 0.5, radius: 90.0 });
let c3 = card(r1, "Barrel", "#ffe66d"); c3.apply_effect("barrel", #{ curvature: 0.4 });
let c4 = card(r1, "Lens Flare", "#a06cd5"); c4.apply_effect("lens_flare", #{ intensity: 0.7, color: "#ffd9a0" });
let c5 = card(r1, "Noir", "#e0e0e0"); c5.apply_effect("noir", #{ contrast: 1.5 });
let c6 = card(r1, "Cyberpunk", "#6bcbff"); c6.apply_effect("cyberpunk", #{ intensity: 0.7 });

// Row 2: Color-grade + stylized
let r2 = root.add_box(#{
    width: "100%", height: "33%",
    flex_direction: "row", justify_content: "space_evenly", align_items: "center"
});
let c7 = card(r2, "Vintage", "#ffb86b"); c7.apply_effect("vintage", #{ amount: 0.7 });
let c8 = card(r2, "Gameboy", "#9bbc0f"); c8.apply_effect("gameboy", #{});
let c9 = card(r2, "Bleach", "#cfcfcf"); c9.apply_effect("bleach_bypass", #{ amount: 0.8 });
let c10 = card(r2, "Teal/Orange", "#ff9e7a"); c10.apply_effect("teal_orange", #{ amount: 0.7 });
let c11 = card(r2, "Plasma", "#58a6ff"); c11.apply_effect("plasma", #{ scale: 12.0, speed: 1.5 });
let c12 = card(r2, "Hologram", "#39d0ff"); c12.apply_effect("hologram", #{ intensity: 0.8 });

// Row 3: Stylized + Atmosphere
let r3 = root.add_box(#{
    width: "100%", height: "33%",
    flex_direction: "row", justify_content: "space_evenly", align_items: "center"
});
let c13 = card(r3, "Emboss", "#c0c0c0"); c13.apply_effect("emboss", #{ angle: 45.0, strength: 2.5 });
let c14 = card(r3, "Toon", "#7ee787"); c14.apply_effect("toon", #{ levels: 5.0, edge: 0.1 });
let c15 = card(r3, "Oil Paint", "#ffa657"); c15.apply_effect("oil_paint", #{ block_size: 9.0, levels: 6.0 });
let c16 = card(r3, "Light Leak", "#ff8c42"); c16.apply_effect("light_leak", #{ intensity: 0.6, color: "#ffcf8f" });
let c17 = card(r3, "Aurora", "#79f7c0"); c17.apply_effect("aurora", #{ intensity: 0.6, speed: 1.2 });
let c18 = card(r3, "Sun Rays", "#ffd166"); c18.apply_effect("sunrays", #{ intensity: 0.6, angle: 0.0, speed: 1.0 });

movie
