// GEMMY SHOWCASE - Cinematic Asset Demo
// Uses assets from C:\Projects\gemmy\outputs
// 16 seconds, 1920x1080, 30fps

let movie = new_director(1280, 720, 30.0);

// ============================================================
// SCENE 1: COSMIC OPENING (0-4s)
// ============================================================
let s1 = movie.add_scene(4.0);

// Full-bleed cosmic background
let s1_bg = s1.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_211837/cosmic_bg.png", #{
    width: "100%",
    height: "100%",
    object_fit: "cover"
});

// Dark overlay for text readability
let s1_overlay = s1.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#000000",
    position: "absolute",
    top: 0,
    left: 0
});
s1_overlay.animate("opacity", 0.6, 0.3, 4.0, "ease_out");

// Central title with gradient text effect
let s1_title = s1.add_text(#{
    content: "COSMOS",
    size: 140.0,
    weight: "black",
    color: "#ffffff",
    stroke_width: 3.0,
    stroke_color: "#ff00ff",
    text_shadow_color: "#ff00ff",
    text_shadow_blur: 30.0,
    text_shadow_x: 0.0,
    text_shadow_y: 0.0,
    position: "absolute",
    top: 340,
    width: "100%",
    justify_content: "center"
});
s1_title.animate("opacity", 0.0, 1.0, 1.5, "ease_out");
s1_title.animate("y", 80, 0, 1.5, "back_out");

// Subtitle
let s1_sub = s1.add_text(#{
    content: "Beyond the Stars",
    size: 42.0,
    color: "#cccccc",
    position: "absolute",
    top: 520,
    width: "100%",
    justify_content: "center"
});
s1_sub.animate("opacity", 0.0, 1.0, 2.0, "linear");

// Pulsing orb shape behind text
let s1_orb = s1.add_circle(#{
    cx: 960.0,
    cy: 420.0,
    r: 180.0,
    fill: "#ff00ff",
    position: "absolute"
});
s1_orb.animate("opacity", 0.0, 0.15, 2.0, "ease_in_out");
s1_orb.animate("scale", 0.8, 1.2, 2.0, "ease_in_out");

// Small floating accent circles
let s1_dot1 = s1.add_circle(#{
    cx: 200.0, cy: 150.0, r: 8.0,
    fill: "#00ffff",
    position: "absolute"
});
s1_dot1.animate("opacity", 0.0, 0.8, 1.0, "ease_out");
s1_dot1.animate("y", 30, 0, 3.0, "ease_in_out");

let s1_dot2 = s1.add_circle(#{
    cx: 1700.0, cy: 800.0, r: 12.0,
    fill: "#ff00ff",
    position: "absolute"
});
s1_dot2.animate("opacity", 0.0, 0.6, 1.5, "ease_out");
s1_dot2.animate("y", -20, 0, 2.5, "ease_in_out");

// ============================================================
// SCENE 2: PRODUCT GALLERY (4-9s)
// ============================================================
let s2 = movie.add_scene(5.0);

let s2_bg = s2.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a0a12"
});

// Section title
let s2_title = s2_bg.add_text(#{
    content: "INNOVATION",
    size: 64.0,
    weight: "bold",
    color: "#ffffff",
    position: "absolute",
    top: 40,
    width: "100%",
    justify_content: "center"
});
s2_title.animate("opacity", 0.0, 1.0, 0.8, "ease_out");

// 3-column product grid
let s2_grid = s2_bg.add_box(#{
    width: "100%",
    height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    padding: 120.0,
    gap: 40.0
});

// Card 1: Smartwatch
let card1 = s2_grid.add_box(#{
    width: 480.0,
    height: 640.0,
    bg_color: "#111118",
    border_radius: 24.0,
    border_width: 1.0,
    border_color: "#222233",
    overflow: "hidden",
    align_items: "center",
    justify_content: "center"
});
let img1 = card1.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_145721/smartwatch_shot.png", #{
    width: "90%",
    height: "70%",
    object_fit: "contain"
});
img1.animate("scale", 0.9, 1.0, 1.2, "back_out");
card1.add_text(#{
    content: "Smartwatch",
    size: 28.0,
    color: "#cccccc",
    margin_top: 20.0
});

// Card 2: Concept Car
let card2 = s2_grid.add_box(#{
    width: 480.0,
    height: 640.0,
    bg_color: "#111118",
    border_radius: 24.0,
    border_width: 1.0,
    border_color: "#222233",
    overflow: "hidden",
    align_items: "center",
    justify_content: "center"
});
let img2 = card2.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_202655/concept_car.png", #{
    width: "90%",
    height: "70%",
    object_fit: "contain"
});
img2.animate("scale", 0.9, 1.0, 1.4, "back_out");
card2.add_text(#{
    content: "Concept Vehicle",
    size: 28.0,
    color: "#cccccc",
    margin_top: 20.0
});

// Card 3: Owl
let card3 = s2_grid.add_box(#{
    width: 480.0,
    height: 640.0,
    bg_color: "#111118",
    border_radius: 24.0,
    border_width: 1.0,
    border_color: "#222233",
    overflow: "hidden",
    align_items: "center",
    justify_content: "center"
});
let img3 = card3.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_202424/owl-nobg.png", #{
    width: "90%",
    height: "70%",
    object_fit: "contain"
});
img3.animate("scale", 0.9, 1.0, 1.6, "back_out");
card3.add_text(#{
    content: "Wildlife",
    size: 28.0,
    color: "#cccccc",
    margin_top: 20.0
});

// ============================================================
// SCENE 3: URBAN ENERGY (9-13s)
// ============================================================
let s3 = movie.add_scene(4.0);

// City video background
let s3_bg = s3.add_video("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_203324/city_pan.mp4", #{
    width: "100%",
    height: "100%",
    object_fit: "cover"
});

// Gradient overlay
let s3_overlay = s3.add_box(#{
    width: "100%",
    height: "100%",
    bg_gradient: #{
        colors: ["#00000000", "#000000cc"],
        start: [0.0, 0.0],
        end: [0.0, 1.0]
    },
    position: "absolute",
    top: 0,
    left: 0
});

// Large kinetic text
let s3_text = s3.add_text(#{
    content: "URBAN",
    size: 180.0,
    weight: "black",
    color: "#ffffff",
    fill_gradient: ["#ff6b00", "#ffcc00"],
    stroke_width: 4.0,
    stroke_color: "#ffffff",
    text_shadow_color: "#ff6b00",
    text_shadow_blur: 40.0,
    position: "absolute",
    top: 280,
    width: "100%",
    justify_content: "center"
});
s3_text.animate("opacity", 0.0, 1.0, 0.8, "ease_out");
s3_text.animate("x", -100, 0, 1.0, "back_out");

// Accent line shape
let s3_line = s3.add_path(#{
    commands: [
        #{ type: "move", x: 200.0, y: 600.0 },
        #{ type: "line", x: 1720.0, y: 600.0 }
    ],
    stroke: "#ff6b00",
    stroke_width: 4.0,
    position: "absolute"
});
s3_line.animate("opacity", 0.0, 1.0, 1.2, "linear");

// Bottom info strip
let s3_strip = s3.add_box(#{
    width: "100%",
    height: 80.0,
    bg_color: "#00000099",
    position: "absolute",
    bottom: 0,
    align_items: "center",
    justify_content: "center"
});
s3_strip.animate("opacity", 0.0, 1.0, 1.5, "linear");
s3_strip.add_text(#{
    content: "Cinematic Motion  //  4K HDR  //  60fps",
    size: 24.0,
    color: "#888888"
});

// ============================================================
// SCENE 4: THE FINALE - COMPOSITION MASHUP (13-16s)
// ============================================================
let s4 = movie.add_scene(3.0);

let s4_bg = s4.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#050508"
});

// Central hero image with circular clip
let s4_hero_container = s4_bg.add_box(#{
    width: 800.0,
    height: 800.0,
    position: "absolute",
    top: 140,
    left: 560,
    border_radius: 400.0,
    overflow: "hidden",
    justify_content: "center",
    align_items: "center"
});

let s4_hero = s4_hero_container.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_142245/hero.png", #{
    width: "100%",
    height: "100%",
    object_fit: "cover"
});
s4_hero.animate("scale", 1.1, 1.0, 3.0, "ease_out");

// Orbital ring shapes
let s4_ring1 = s4_bg.add_circle(#{
    cx: 960.0, cy: 540.0, r: 420.0,
    stroke: "#ff00ff",
    stroke_width: 2.0,
    position: "absolute"
});
s4_ring1.animate("opacity", 0.0, 0.4, 1.5, "ease_out");
s4_ring1.animate("scale", 0.9, 1.0, 2.0, "ease_in_out");

let s4_ring2 = s4_bg.add_circle(#{
    cx: 960.0, cy: 540.0, r: 460.0,
    stroke: "#00ffff",
    stroke_width: 1.0,
    position: "absolute"
});
s4_ring2.animate("opacity", 0.0, 0.25, 2.0, "ease_out");
s4_ring2.animate("scale", 0.85, 1.0, 2.5, "ease_in_out");

// Corner accent images (small, with effects)
let corner1 = s4_bg.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_212629/scene1.png", #{
    width: 200.0,
    height: 120.0,
    position: "absolute",
    top: 40,
    left: 40,
    border_radius: 12.0
});
corner1.apply_effect("blur", 2.0);
corner1.animate("opacity", 0.0, 0.7, 1.0, "ease_out");

let corner2 = s4_bg.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_212629/scene2.png", #{
    width: 200.0,
    height: 120.0,
    position: "absolute",
    top: 40,
    right: 40,
    border_radius: 12.0
});
corner2.apply_effect("sepia");
corner2.animate("opacity", 0.0, 0.7, 1.2, "ease_out");

let corner3 = s4_bg.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_212629/scene3.png", #{
    width: 200.0,
    height: 120.0,
    position: "absolute",
    bottom: 40,
    left: 40,
    border_radius: 12.0
});
corner3.apply_effect("grayscale");
corner3.animate("opacity", 0.0, 0.7, 1.4, "ease_out");

// Final title
let s4_title = s4_bg.add_text(#{
    content: "DIRECTOR",
    size: 72.0,
    weight: "bold",
    color: "#ffffff",
    text_shadow_color: "#ff00ff",
    text_shadow_blur: 20.0,
    position: "absolute",
    bottom: 180,
    width: "100%",
    justify_content: "center"
});
s4_title.animate("opacity", 0.0, 1.0, 1.0, "ease_out");
s4_title.animate("y", 30, 0, 1.0, "back_out");

// ============================================================
// TRANSITIONS
// ============================================================
movie.add_transition(s1, s2, "fade", 0.8, "ease_in_out");
movie.add_transition(s2, s3, "slide_left", 0.6, "ease_out");
movie.add_transition(s3, s4, "circle_open", 1.0, "ease_in_out");

// ============================================================
// GLOBAL AUDIO
// ============================================================
let music = movie.add_audio("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_212629/music.wav");

movie
