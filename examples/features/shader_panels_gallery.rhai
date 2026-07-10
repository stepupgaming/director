// Shader Panels Gallery
//
// Demonstrates the 10 new shader effects ported from shader-color-panels:
//   New:          thermal, liquid, acid, halftone, ascii, sketch, mirror
//   Enhanced:     tritone (3-color duotone), vhs (CRT glitch), neon_edge
//
// Each scene shows one or more effects applied to labeled colored cards.

let movie = new_director(1280, 720, 30.0);

// Helper: a labeled card with a gradient background and rounded corners.
fn make_card(parent, label, color) {
    let card = parent.add_box(#{
        width: 280.0,
        height: 180.0,
        flex_direction: "column",
        justify_content: "center",
        align_items: "center",
        gap: 10.0,
        bg_color: color,
        border_radius: 18.0
    });
    card.add_text(#{
        content: label,
        font_size: 22.0,
        color: "#ffffff",
        font_weight: "bold"
    });
    return card;
}

// ---- Scene 1: Thermal + Liquid + Acid ----
let s1 = movie.add_scene(3.0);
let r1 = s1.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#0b1020"
});
let thermal_card = make_card(r1, "Thermal", "#ff6b6b");
thermal_card.apply_effect("thermal");
let liquid_card = make_card(r1, "Liquid", "#4ecdc4");
liquid_card.apply_effect("liquid", #{ frequency: 12.0, amplitude: 0.015 });
let acid_card = make_card(r1, "Acid", "#ffe66d");
acid_card.apply_effect("acid", #{ color: "#10b981", speed: 1.8 });

// ---- Scene 2: Halftone + ASCII + Sketch ----
let s2 = movie.add_scene(3.0);
let r2 = s2.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#121212"
});
let halftone_card = make_card(r2, "Halftone", "#a06cd5");
halftone_card.apply_effect("halftone", #{ dot_size: 55.0 });
let ascii_card = make_card(r2, "ASCII", "#6bcb77");
ascii_card.apply_effect("ascii", #{ scale: 90.0, color: "#00ff41" });
let sketch_card = make_card(r2, "Sketch", "#ff9f43");
sketch_card.apply_effect("sketch", #{ contrast: 1.2 });

// ---- Scene 3: Mirror + TriTone ----
let s3 = movie.add_scene(3.0);
let r3 = s3.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#1a1a2e"
});
let mirror_card = make_card(r3, "Mirror", "#e94560");
mirror_card.apply_effect("mirror");
let tritone_card = make_card(r3, "TriTone", "#00b8d4");
tritone_card.apply_effect("tritone", #{
    shadow: "#08080a",
    mid: "#3b82f6",
    highlight: "#60a5fa",
    midpoint: 0.45
});

// ---- Scene 4: VHS + Neon Edge ----
let s4 = movie.add_scene(3.0);
let r4 = s4.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#0d1117"
});
let vhs_card = make_card(r4, "VHS", "#58a6ff");
vhs_card.apply_effect("vhs", #{ distortion: 0.35, scanline: 0.5 });
let neon_card = make_card(r4, "Neon Edge", "#7ee787");
neon_card.apply_effect("neon_edge", #{ glow_color: "#00ffcc", threshold: 0.12 });

// ---- Scene 5: Combined showcase ----
let s5 = movie.add_scene(3.0);
let r5 = s5.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "column",
    justify_content: "center",
    align_items: "center",
    gap: 30.0,
    bg_color: "#161b22"
});
r5.add_text(#{
    content: "Shader Panels Gallery",
    font_size: 48.0,
    color: "#ffffff",
    font_weight: "bold"
});

let showcase = r5.add_box(#{
    width: 600.0,
    height: 300.0,
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_gradient: #{
        colors: ["#1e1b4b", "#4c1d95", "#b91c1c"],
        type: "radial",
        center: [0.5, 0.5],
        radius: 0.8
    },
    border_radius: 20.0
});
let show_card = make_card(showcase, "EFFECTS", "#ff6b6b");
show_card.apply_effect("thermal");
show_card.apply_effect("vignette", #{ intensity: 0.4, radius: 0.4, feather: 0.2 });

r5.add_text(#{
    content: "10 new shaders + radial gradients + backdrop blur",
    font_size: 20.0,
    color: "#8b949e"
});

movie
