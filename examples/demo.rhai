// DIRECTOR CAPABILITIES DEMO
// 8 seconds, 1920x1080, 30fps, motion blur enabled
// Coherent narrative: cosmic opening -> product showcase -> urban energy -> grand finale

let movie = new_director(1920, 1080, 30.0, #{ mode: "export" });
movie.configure_encoder("auto");
movie.configure_motion_blur(4, 180.0);

// Global music bed
let music = movie.add_audio("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_212629/music.wav");
music.animate_volume(0.0, 0.7, 2.0, "ease_out");

// ============================================================
// SCENE 1: COSMIC OPENING (0-2s)
// Full-screen nebula video with kinetic title
// ============================================================
let s1 = movie.add_scene(2.0);

let nebula = s1.add_video("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_205741/final_nebula_scene.mp4", #{
    width: "100%", height: "100%", object_fit: "cover"
});
nebula.apply_effect("grain", #{ intensity: 0.05, size: 1.0 });

// Dark vignette overlay
let vignette = s1.add_box(#{
    width: "100%", height: "100%",
    bg_gradient: ["#000000ff", "#00000000", "#000000ff"],
    position: "absolute", top: 0, left: 0
});
vignette.animate("opacity", 0.0, 0.6, 1.5, "ease_out");

// Title with gradient fill, stroke, and drop shadow
let title = s1.add_text(#{
    content: [
        #{ text: "DIRECTOR", color: "#ffffff", size: 120.0, weight: "black" }
    ],
    position: "absolute",
    top: 380, width: "100%",
    justify_content: "center",
    fill_gradient: ["#ff00ff", "#00ffff", "#ffcc00"],
    stroke_width: 3.0,
    stroke_color: "#000000",
    text_shadow_color: "#ff00ff",
    text_shadow_blur: 40.0,
    text_shadow_x: 0.0,
    text_shadow_y: 8.0
});
title.animate("opacity", 0.0, 1.0, 1.0, "ease_out");
title.animate("y", 60, 0, 1.2, "back_out");
title.animate("scale", 0.85, 1.0, 1.2, "back_out");

// Subtitle
let subtitle = s1.add_text(#{
    content: "Programmatic Video Engine",
    size: 36.0,
    color: "#cccccc",
    weight: "light",
    position: "absolute",
    top: 540, width: "100%",
    justify_content: "center"
});
subtitle.animate("opacity", 0.0, 1.0, 1.5, "ease_out");
subtitle.animate("y", 30, 0, 1.5, "ease_out");

// Floating orb accent
let orb = s1.add_circle(#{
    cx: 1600.0, cy: 200.0, r: 80.0,
    fill_gradient: ["#ff00ff", "#00ffff"],
    position: "absolute"
});
orb.animate("opacity", 0.0, 0.5, 2.0, "ease_in_out");
orb.animate("scale", 0.5, 1.0, 2.0, "ease_in_out");

// ============================================================
// SCENE 2: PRODUCT SHOWCASE (2-4s)
// Split screen with images, rich text, shape accents
// ============================================================
let s2 = movie.add_scene(2.0);

let split = s2.add_split_screen(#{
    left: "C:/Projects/gemmy/outputs/agent_runs/agent_20260628_142245/hero.png",
    right: "C:/Projects/gemmy/outputs/agent_runs/agent_20260628_202655/concept_car.png",
    ratio: 0.5,
    gap: 4.0
});
split.animate("opacity", 0.0, 1.0, 0.8, "ease_out");

// Overlay text with spans
let product_text = s2.add_text(#{
    content: [
        #{ text: "Design ", color: "#ffffff", size: 64.0 },
        #{ text: "Without ", color: "#ffcc00", size: 64.0, weight: "bold" },
        #{ text: "Limits", color: "#00ffff", size: 64.0, weight: "black" }
    ],
    position: "absolute",
    top: 80, width: "100%",
    justify_content: "center",
    stroke_width: 2.0,
    stroke_color: "#000000",
    text_shadow_color: "#000000",
    text_shadow_blur: 20.0,
    text_shadow_x: 0.0,
    text_shadow_y: 4.0
});
product_text.animate("opacity", 0.0, 1.0, 1.0, "ease_out");
product_text.animate("y", -40, 0, 1.0, "back_out");

// Accent line drawn with path
let accent_line = s2.add_path(#{
    commands: [
        #{ type: "move", x: 560.0, y: 680.0 },
        #{ type: "line", x: 1360.0, y: 680.0 }
    ],
    stroke: "#ff00ff",
    stroke_width: 4.0,
    position: "absolute"
});
accent_line.animate("opacity", 0.0, 1.0, 1.2, "ease_out");

// Small orb accents
let orb_left = s2.add_circle(#{
    cx: 200.0, cy: 900.0, r: 30.0,
    fill: "#00ffff",
    position: "absolute"
});
orb_left.animate("opacity", 0.0, 0.7, 1.5, "ease_in_out");

let orb_right = s2.add_ellipse(#{
    cx: 1720.0, cy: 900.0, rx: 40.0, ry: 20.0,
    fill: "#ff00ff",
    position: "absolute"
});
orb_right.animate("opacity", 0.0, 0.7, 1.5, "ease_in_out");

// ============================================================
// SCENE 3: URBAN ENERGY (4-6s)
// Video background with kinetic typography, shapes, effects
// ============================================================
let s3 = movie.add_scene(2.0);

let city = s3.add_video("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_203324/city_pan.mp4", #{
    width: "100%", height: "100%", object_fit: "cover"
});
city.apply_effect("directional_blur", #{ strength: 6.0, angle: 0.0, samples: 12 });

// Gradient overlay for text readability
let city_overlay = s3.add_box(#{
    width: "100%", height: "100%",
    bg_gradient: ["#000000cc", "#00000000"],
    position: "absolute", top: 0, left: 0
});

// Kinetic text
let kinetic = s3.add_text(#{
    content: [
        #{ text: "MOTION ", color: "#ffffff", size: 96.0, weight: "black" },
        #{ text: "DRIVEN", color: "#ff00ff", size: 96.0, weight: "black" }
    ],
    position: "absolute",
    top: 350, width: "100%",
    justify_content: "center",
    fill_gradient: ["#ff00ff", "#00ffff"],
    stroke_width: 2.0,
    stroke_color: "#000000"
});
kinetic.animate("opacity", 0.0, 1.0, 0.8, "ease_out");
kinetic.animate("x", -200, 0, 1.0, "elastic_out");

// Per-glyph wave
kinetic.add_animator(0, 6, "y", 30.0, 0.0, 1.0, "bounce_out", 0.04);
kinetic.add_animator(7, 13, "y", 30.0, 0.0, 1.0, "bounce_out", 0.04);

// Audio-reactive orb
let bass_orb = s3.add_circle(#{
    cx: 960.0, cy: 200.0, r: 50.0,
    fill: "#ffff00",
    position: "absolute"
});
bass_orb.bind_audio(music.id(), "bass", "scale");
bass_orb.animate("opacity", 0.0, 0.8, 1.0, "ease_out");

// Orbiting ring of small circles
let ring1 = s3.add_circle(#{
    cx: 0.0, cy: 0.0, r: 12.0,
    fill: "#00ffff",
    position: "absolute"
});
ring1.path_animate("M 960 540 A 300 300 0 1 1 960 541", 2.0, "linear");

// ============================================================
// SCENE 4: GRAND FINALE (6-8s)
// Everything together: shapes, effects, templates, transitions
// ============================================================
let s4 = movie.add_scene(2.0);

let s4_bg = s4.add_box(#{
    width: "100%", height: "100%",
    bg_gradient: ["#0a0a12", "#1a0a2e"]
});

// Hero image with sepia effect
let hero = s4_bg.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_142245/hero.png", #{
    width: 500.0, height: 500.0,
    position: "absolute", top: 100, left: 100,
    object_fit: "cover",
    border_radius: 250.0
});
hero.apply_effect("sepia");
hero.animate("opacity", 0.0, 1.0, 0.8, "ease_out");
hero.animate("scale", 0.8, 1.0, 1.0, "back_out");

// Car image with grayscale
let car = s4_bg.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_202655/concept_car.png", #{
    width: 500.0, height: 500.0,
    position: "absolute", top: 100, right: 100,
    object_fit: "cover",
    border_radius: 250.0
});
car.apply_effect("grayscale");
car.animate("opacity", 0.0, 1.0, 1.0, "ease_out");
car.animate("scale", 0.8, 1.0, 1.0, "back_out");

// Center quote card
let quote = s4.add_quote_card(#{
    quote: "The future of video is code.",
    author: "Director Engine",
    bg_color: "#00000088",
    text_color: "#ffffff",
    accent_color: "#ff00ff",
    border_radius: 20.0
});
quote.animate("opacity", 0.0, 1.0, 1.2, "ease_out");
quote.animate("scale", 0.9, 1.0, 1.2, "back_out");

// Bottom progress bar
let pb = s4.add_progress_bar(#{
    progress: 100.0,
    label: "Features Loaded",
    bar_color: "#00ff88",
    bg_color: "#1a1a2e",
    text_color: "#ffffff",
    width: 800.0,
    height: 32.0
});
pb.animate("opacity", 0.0, 1.0, 1.5, "ease_out");

// Subtle grain over everything
let grain = s4.add_box(#{
    width: "100%", height: "100%",
    position: "absolute", top: 0, left: 0
});
grain.apply_effect("grain", #{ intensity: 0.06, size: 1.0 });
grain.animate("opacity", 0.0, 0.3, 2.0, "ease_in_out");

// ============================================================
// TRANSITIONS
// ============================================================
movie.add_transition(s1, s2, "fade", 0.4, "ease_in_out");
movie.add_transition(s2, s3, "slide_left", 0.4, "ease_out");
movie.add_transition(s3, s4, "circle_open", 0.6, "ease_in_out");

movie
