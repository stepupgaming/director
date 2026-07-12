// Shaders & Effects Gallery — Batch 2
//
// Demonstrates the second shader wave:
//   Lens/Distortion: fisheye, bulg, barrel, lens_flare
//   Color-Grade:     noir, cyberpunk, vintage, gameboy, bleach_bypass, teal_orange
//   Stylized:        plasma, hologram, emboss, toon, oil_paint
//   Atmosphere:       light_leak, aurora, sunrays
//
// Each scene applies one effect to a labeled colored card so the
// result is easy to compare against the unfiltered original.

let movie = new_director(1280, 720, 30);

// Helper: a labeled card with a solid color and rounded corners.
fn make_card(parent, label, color) {
    let card = parent.add_box(#{
        width: 240.0,
        height: 150.0,
        flex_direction: "column",
        justify_content: "center",
        align_items: "center",
        gap: 10.0,
        bg_color: color,
        border_radius: 18.0
    });
    card.add_text(#{
        content: label,
        font_size: 20.0,
        color: "#ffffff",
        font_weight: "bold"
    });
    return card;
}

// ---- Scene 1: Lens / Distortion ----
let s1 = movie.add_scene(2.5);
let r1 = s1.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#0b1020"
});
let fish_card = make_card(r1, "Fisheye", "#ff6b6b");
fish_card.apply_effect("fisheye", #{ strength: 0.6 });
let bulge_card = make_card(r1, "Bulge", "#4ecdc4");
bulge_card.apply_effect("bulge", #{ strength: 0.5, radius: 220.0 });
let barrel_card = make_card(r1, "Barrel", "#ffe66d");
barrel_card.apply_effect("barrel", #{ curvature: 0.4 });
let flare_card = make_card(r1, "Lens Flare", "#a06cd5");
flare_card.apply_effect("lens_flare", #{ intensity: 0.7, color: "#ffd9a0" });

// ---- Scene 2: Color Grade ----
let s2 = movie.add_scene(2.5);
let r2 = s2.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#121212"
});
let noir_card = make_card(r2, "Noir", "#e0e0e0");
noir_card.apply_effect("noir", #{ contrast: 1.5 });
let cyber_card = make_card(r2, "Cyberpunk", "#6bcbff");
cyber_card.apply_effect("cyberpunk", #{ intensity: 0.7 });
let vintage_card = make_card(r2, "Vintage", "#ffb86b");
vintage_card.apply_effect("vintage", #{ amount: 0.7 });
let gameboy_card = make_card(r2, "Gameboy", "#9bbc0f");
gameboy_card.apply_effect("gameboy");
let bleach_card = make_card(r2, "Bleach Bypass", "#cfcfcf");
bleach_card.apply_effect("bleach_bypass", #{ amount: 0.8 });
let teal_card = make_card(r2, "Teal/Orange", "#ff9e7a");
teal_card.apply_effect("teal_orange", #{ amount: 0.7 });

// ---- Scene 3: Stylized ----
let s3 = movie.add_scene(2.5);
let r3 = s3.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#0d1117"
});
let plasma_card = make_card(r3, "Plasma", "#58a6ff");
plasma_card.apply_effect("plasma", #{ scale: 12.0, speed: 1.5 });
let holo_card = make_card(r3, "Hologram", "#39d0ff");
holo_card.apply_effect("hologram", #{ intensity: 0.8 });
let emboss_card = make_card(r3, "Emboss", "#c0c0c0");
emboss_card.apply_effect("emboss", #{ angle: 45.0, strength: 2.5 });
let toon_card = make_card(r3, "Toon", "#7ee787");
toon_card.apply_effect("toon", #{ levels: 5.0, edge: 0.1 });
let oil_card = make_card(r3, "Oil Paint", "#ffa657");
oil_card.apply_effect("oil_paint", #{ block_size: 9.0, levels: 6.0 });

// ---- Scene 4: Atmosphere ----
let s4 = movie.add_scene(2.5);
let r4 = s4.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#161b22"
});
let leak_card = make_card(r4, "Light Leak", "#ff8c42");
leak_card.apply_effect("light_leak", #{ intensity: 0.6, color: "#ffcf8f" });
let aurora_card = make_card(r4, "Aurora", "#79f7c0");
aurora_card.apply_effect("aurora", #{ intensity: 0.6, speed: 1.2 });
let sun_card = make_card(r4, "Sun Rays", "#ffd166");
sun_card.apply_effect("sunrays", #{ intensity: 0.6, angle: 0.0, speed: 1.0 });

movie
