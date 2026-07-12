// Shaders & Effects Gallery
//
// Demonstrates the extended effects/shader library:
//   Distortion:    warp, swirl, ripple, glass, shake
//   Color/Grade:   duotone, posterize, threshold, temperature
//   Stylize:       scanlines, glitch, edge_detect
//   Light/Atmos:   zoom_blur, kaleidoscope
//
// Each scene applies one or more effects to a labeled colored card so the
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
        font_size: 22.0,
        color: "#ffffff",
        font_weight: "bold"
    });
    return card;
}

// ---- Scene 1: Distortion effects ----
let s1 = movie.add_scene(2.0);
let r1 = s1.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#0b1020"
});
let warp_card = make_card(r1, "Warp", "#ff6b6b");
warp_card.apply_effect("warp", #{ amplitude: 12.0, frequency: 0.03, direction: 0.0, speed: 3.0 });
let swirl_card = make_card(r1, "Swirl", "#4ecdc4");
swirl_card.apply_effect("swirl", #{ radius: 120.0, angle: 220.0 });
let ripple_card = make_card(r1, "Ripple", "#ffe66d");
ripple_card.apply_effect("ripple", #{ amplitude: 6.0, frequency: 0.08, speed: 4.0 });

// ---- Scene 2: Glass + Shake ----
let s2 = movie.add_scene(2.0);
let r2 = s2.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#121212"
});
let glass_card = make_card(r2, "Glass", "#a06cd5");
glass_card.apply_effect("glass", #{ scale: 0.02, intensity: 6.0 });
let shake_card = make_card(r2, "Shake", "#6bcb77");
shake_card.apply_effect("shake", #{ intensity: 6.0, speed: 20.0 });

// ---- Scene 3: Color / Grade ----
let s3 = movie.add_scene(2.0);
let r3 = s3.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#1a1a2e"
});
let duo_card = make_card(r3, "Duotone", "#e94560");
duo_card.apply_effect("duotone", #{ shadow: "#0d1b2a", highlight: "#f5d76e" });
let post_card = make_card(r3, "Posterize", "#00b8d4");
post_card.apply_effect("posterize", #{ levels: 3.0 });
let thr_card = make_card(r3, "Threshold", "#9b5de5");
thr_card.apply_effect("threshold", #{ level: 0.45 });
let temp_card = make_card(r3, "Temperature", "#f15bb5");
temp_card.apply_effect("temperature", 0.4);

// ---- Scene 4: Stylize ----
let s4 = movie.add_scene(2.0);
let r4 = s4.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#0d1117"
});
let scan_card = make_card(r4, "Scanlines", "#58a6ff");
scan_card.apply_effect("scanlines", #{ intensity: 0.5, count: 240.0 });
let glitch_card = make_card(r4, "Glitch", "#f778ba");
glitch_card.apply_effect("glitch", #{ intensity: 1.0, speed: 12.0 });
let edge_card = make_card(r4, "Edge Detect", "#7ee787");
edge_card.apply_effect("edge_detect", #{ intensity: 1.5 });

// ---- Scene 5: Light / Atmosphere ----
let s5 = movie.add_scene(2.0);
let r5 = s5.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#161b22"
});
let zoom_card = make_card(r5, "Zoom Blur", "#ffa657");
zoom_card.apply_effect("zoom_blur", #{ strength: 0.35 });
let kaleo_card = make_card(r5, "Kaleidoscope", "#79c0ff");
kaleo_card.apply_effect("kaleidoscope", #{ segments: 8.0 });

movie