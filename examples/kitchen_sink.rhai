// KITCHEN SINK — Every Director Feature Demo
// Goal: exercise every Rhai API to find broken/lacking/low-quality features
// 12 seconds, 1920x1080, 30fps - kitchen sink feature test

let movie = new_director(1920, 1080, 30.0, #{ mode: "export" });

// ============================================================
// GLOBAL CONFIG
// ============================================================
movie.configure_encoder("auto");
let enc_info = movie.get_encoder_info();
// Motion blur disabled for fast rendering during feature audit

// Global audio track
let global_music = movie.add_audio("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_212629/music.wav");
global_music.animate_volume(0.0, 0.8, 2.0, "ease_out");

// Add a reverb effect to global audio
let reverb_idx = global_music.add_effect("reverb", #{ decay: 2.5, wet: 0.3, dry: 0.7 });
global_music.set_effect_param(reverb_idx, "decay", 3.0);
global_music.animate_effect_param(reverb_idx, "wet", 0.3, 0.5, 5.0, "ease_in_out");

// ============================================================
// SCENE 1: SETUP + SHAPES + SPRING + TEMPLATES (0-4s)
// ============================================================
let s1 = movie.add_scene(2.0);

// Title card template
let tc = s1.add_title_card(#{
    title: "KITCHEN SINK",
    subtitle: "Every Feature Test",
    bg_color: "#0a0a12",
    title_color: "#ffffff",
    subtitle_color: "#888888",
    title_size: 96.0,
    subtitle_size: 48.0,
    shadow_color: "#ff00ff",
    shadow_blur: 20.0,
    shadow_x: 0.0,
    shadow_y: 4.0
});
tc.animate("opacity", 0.0, 1.0, 1.0, "ease_out");
tc.animate("scale", 0.9, 1.0, 1.0, "back_out");

// Floating circles with spring physics (rand_float usage)
let orb1 = s1.add_circle(#{
    cx: rand_float(100.0, 300.0), cy: rand_float(100.0, 300.0), r: 40.0,
    fill: "#ff00ff",
    position: "absolute"
});
orb1.animate("opacity", 0.0, 0.6, 2.0, "ease_in_out");
orb1.spring("scale", 0.5, 1.2, #{ stiffness: 100.0, damping: 10.0 });

let orb2 = s1.add_ellipse(#{
    cx: 1600.0, cy: 200.0, rx: 60.0, ry: 30.0,
    fill_gradient: ["#00ffff", "#ff00ff"],
    position: "absolute"
});
orb2.animate("rotation", 0.0, 360.0, 3.0, "linear");

// Path shape — star burst
let star = s1.add_path(#{
    commands: [
        #{ type: "move", x: 960.0, y: 100.0 },
        #{ type: "line", x: 990.0, y: 190.0 },
        #{ type: "line", x: 1080.0, y: 190.0 },
        #{ type: "line", x: 1010.0, y: 240.0 },
        #{ type: "line", x: 1030.0, y: 330.0 },
        #{ type: "line", x: 960.0, y: 280.0 },
        #{ type: "line", x: 890.0, y: 330.0 },
        #{ type: "line", x: 910.0, y: 240.0 },
        #{ type: "line", x: 840.0, y: 190.0 },
        #{ type: "line", x: 930.0, y: 190.0 },
        #{ type: "close" }
    ],
    fill: "#ffcc00",
    stroke: "#ffffff",
    stroke_width: 3.0,
    position: "absolute"
});
star.animate("scale", 0.0, 1.0, 1.5, "bounce_out");
star.animate_color("#ffcc00", "#ff6600", 2.0, "ease_in_out");

// Watermark template
let wm = s1.add_watermark(#{
    text: "DIRECTOR v0.1",
    corner: "bottom_right",
    color: "#ffffff",
    opacity: 0.4,
    text_size: 16.0,
    margin: 40.0
});

// ============================================================
// SCENE 2: MEDIA + MASKS + BLEND MODES + EFFECTS (4-8s)
// ============================================================
let s2 = movie.add_scene(2.0);

let s2_bg = s2.add_box(#{
    width: "100%", height: "100%", bg_color: "#050508"
});

// Image with mask (circle mask)
let mask_circle = s2_bg.add_circle(#{
    cx: 400.0, cy: 400.0, r: 300.0,
    fill: "#ffffff",
    position: "absolute"
});
let masked_img = s2_bg.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_142245/hero.png", #{
    width: 800.0, height: 800.0,
    position: "absolute", top: 40, left: 80,
    object_fit: "cover"
});
masked_img.set_mask(mask_circle);
masked_img.animate("opacity", 0.0, 1.0, 1.0, "ease_out");
masked_img.animate("scale", 1.2, 1.0, 2.0, "ease_out");

// Blend mode test — multiply overlay
let blend_overlay = s2_bg.add_box(#{
    width: 400.0, height: 400.0,
    bg_gradient: ["#ff00ff", "#00ffff"],
    position: "absolute", top: 200, left: 1000,
    border_radius: 200.0
});
blend_overlay.set_blend_mode("multiply");
blend_overlay.animate("opacity", 0.0, 0.6, 2.0, "ease_in_out");
blend_overlay.animate("rotation", 0.0, 45.0, 3.0, "ease_in_out");

// Effects test grid
let fx_grid = s2_bg.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    padding: 100.0
});

let fx1 = fx_grid.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_145721/smartwatch_shot.png", #{
    width: 300.0, height: 300.0, object_fit: "contain"
});
fx1.apply_effect("blur", 5.0);
fx1.animate("opacity", 0.0, 1.0, 0.8, "ease_out");

let fx2 = fx_grid.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_202655/concept_car.png", #{
    width: 300.0, height: 300.0, object_fit: "contain"
});
fx2.apply_effect("sepia");
fx2.animate("opacity", 0.0, 1.0, 1.0, "ease_out");

let fx3 = fx_grid.add_image("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_202424/owl-nobg.png", #{
    width: 300.0, height: 300.0, object_fit: "contain"
});
fx3.apply_effect("grayscale");
fx3.animate("opacity", 0.0, 1.0, 1.2, "ease_out");

let fx4 = fx_grid.add_box(#{
    width: 300.0, height: 300.0,
    bg_color: "#00ff00",
    align_items: "center",
    justify_content: "center"
});
fx4.add_text(#{ content: "INVERT", size: 48.0, color: "#ffffff" });
fx4.apply_effect("invert");
fx4.animate("opacity", 0.0, 1.0, 1.4, "ease_out");

// Effect node container
let fx_container = s2_bg.add_effect_node(#{
    width: 200.0, height: 200.0,
    position: "absolute", top: 40, right: 40
});
let fx_child = fx_container.add_box(#{
    width: "100%", height: "100%",
    bg_color: "#ff6600"
});
fx_child.apply_effect("brightness", 0.5);
fx_child.apply_effect("blur", 2.0);

// ============================================================
// SCENE 3: TEXT + PATH ANIMATE + PATH MORPH + COLOR (8-12s)
// ============================================================
let s3 = movie.add_scene(2.0);

let s3_bg = s3.add_box(#{
    width: "100%", height: "100%", bg_color: "#0f0f1a"
});

// Rich text with spans
let rich_text = s3_bg.add_text(#{
    content: [
        #{ text: "Hello ", color: "#ffffff", size: 64.0 },
        #{ text: "World", color: "#ff00ff", size: 64.0, weight: "bold" },
        #{ text: "!", color: "#00ffff", size: 64.0, weight: "black" }
    ],
    position: "absolute", top: 60, width: "100%",
    justify_content: "center",
    stroke_width: 2.0,
    stroke_color: "#000000",
    background_color: "#222233",
    background_padding: 8.0,
    fill_gradient: ["#ff00ff", "#00ffff"]
});
rich_text.animate("opacity", 0.0, 1.0, 0.8, "ease_out");
rich_text.animate("y", 50, 0, 1.0, "back_out");

// Per-glyph animator
rich_text.add_animator(0, 5, "opacity", 0.0, 1.0, 1.5, "ease_out", 0.05);
rich_text.add_animator(6, 11, "y", 20.0, 0.0, 1.0, "bounce_out", 0.03);

// Text with shadow
let shadow_text = s3_bg.add_text(#{
    content: "DROP SHADOW",
    size: 80.0,
    weight: "black",
    color: "#ffffff",
    text_shadow_color: "#ff00ff",
    text_shadow_blur: 30.0,
    text_shadow_x: 10.0,
    text_shadow_y: 10.0,
    position: "absolute", top: 300, width: "100%",
    justify_content: "center"
});
shadow_text.animate("opacity", 0.0, 1.0, 1.2, "ease_out");
shadow_text.animate("x", -100, 0, 1.0, "elastic_out");

// Path animate — orbiting dot
let orbit_dot = s3_bg.add_circle(#{
    cx: 0.0, cy: 0.0, r: 15.0,
    fill: "#ffff00",
    position: "absolute"
});
orbit_dot.path_animate("M 600 540 A 360 360 0 1 1 600 541", 3.0, "linear");

// Path morph test
let morph_shape = s3_bg.add_path(#{
    commands: [
        #{ type: "move", x: 960.0, y: 200.0 },
        #{ type: "line", x: 1060.0, y: 400.0 },
        #{ type: "line", x: 860.0, y: 400.0 },
        #{ type: "close" }
    ],
    fill: "#00ff88",
    position: "absolute"
});
morph_shape.path_morph(
    "M 960 200 L 1060 400 L 860 400 Z",
    "M 960 200 C 1160 200 1160 400 960 400 C 760 400 760 200 960 200 Z",
    2.0,
    "ease_in_out"
);

// animate_color on box
let color_box = s3_bg.add_box(#{
    width: 200.0, height: 200.0,
    position: "absolute", bottom: 100, left: 100,
    border_radius: 20.0
});
color_box.animate_color("#ff0000", "#0000ff", 2.0, "ease_in_out");

// ============================================================
// SCENE 4: TEMPLATES GALORE (12-17s)
// ============================================================
let s4 = movie.add_scene(2.0);

let s4_bg = s4.add_box(#{
    width: "100%", height: "100%", bg_color: "#111118"
});

// Lower third
let lt = s4.add_lower_third(#{
    text: "Kitchen Sink Demo — Testing All APIs",
    bg_color: "#000000cc",
    text_color: "#ffffff",
    accent_color: "#ff00ff",
    text_size: 28.0
});
lt.animate("y", 100, 0, 0.8, "back_out");

// Split screen
let ss = s4.add_split_screen(#{
    left: "C:/Projects/gemmy/outputs/agent_runs/agent_20260628_211837/cosmic_bg.png",
    right: "C:/Projects/gemmy/outputs/agent_runs/agent_20260628_142245/hero.png",
    ratio: 0.4,
    gap: 8.0
});
ss.animate("opacity", 0.0, 1.0, 1.0, "ease_out");

// Bento grid
let bento = s4.add_bento_grid(#{
    columns: 2, rows: 2, gap: 12.0, padding: 40.0,
    items: [
        #{ bg_color: "#1a1a2e", text: "Feature A", text_color: "#ffffff" },
        #{ bg_color: "#16213e", text: "Feature B", text_color: "#cccccc" },
        #{ bg_color: "#0f3460", text: "Feature C", text_color: "#aaaaaa" },
        #{ bg_color: "#533483", text: "Feature D", text_color: "#eeeeee" }
    ]
});
bento.animate("scale", 0.9, 1.0, 1.2, "back_out");

// Quote card
let qc = s4.add_quote_card(#{
    quote: "Director is the most powerful programmatic video engine I've ever used.",
    author: "A Happy User",
    source: "Testimonial",
    bg_color: "#1a1a2e",
    text_color: "#ffffff",
    accent_color: "#00ffff"
});
qc.animate("opacity", 0.0, 1.0, 1.5, "ease_out");

// Countdown timer
let cd = s4.add_countdown_timer(#{
    days: 0, hours: 2, minutes: 45, seconds: 30,
    header: "Launch In:",
    segment_bg: "#1a1a2e",
    text_color: "#ffffff"
});
cd.animate("scale", 0.8, 1.0, 1.0, "bounce_out");

// Progress bar
let pb = s4.add_progress_bar(#{
    progress: 75.0,
    label: "Completion",
    bar_color: "#00ff88",
    bg_color: "#1a1a2e",
    text_color: "#ffffff"
});
pb.animate("opacity", 0.0, 1.0, 1.2, "linear");

// Subscribe button
let sub = s4.add_subscribe_button(#{
    channel_name: "Director Engine",
    sub_count: "12.5K",
    button_text: "SUBSCRIBE",
    button_color: "#ff0000",
    bg_color: "#1a1a2e",
    position: "bottom_center"
});
sub.animate("y", 50, 0, 0.8, "back_out");

// Social handles
let social = s4.add_social_handle_overlay(#{
    position: "top_right",
    platforms: [
        #{ platform: "YouTube", handle: "@director_engine", color: "#ff0000" },
        #{ platform: "Twitter", handle: "@director_dev", color: "#1da1f2" },
        #{ platform: "GitHub", handle: "director-org", color: "#333333" }
    ]
});
social.animate("opacity", 0.0, 1.0, 1.0, "ease_out");

// ============================================================
// SCENE 5: VIDEO + LOTTIE + ADVANCED EFFECTS + COMPOSITION (17-21s)
// ============================================================
let s5 = movie.add_scene(2.0);

let s5_bg = s5.add_video("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_203324/city_pan.mp4", #{
    width: "100%", height: "100%", object_fit: "cover"
});
s5_bg.apply_effect("directional_blur", #{ strength: 8.0, angle: 45.0, samples: 16 });

// Overlay with grain
let s5_overlay = s5.add_box(#{
    width: "100%", height: "100%",
    bg_color: "#00000066",
    position: "absolute", top: 0, left: 0
});
s5_overlay.apply_effect("grain", #{ intensity: 0.08, size: 1.5 });

// Lottie animation
let lottie_node = s5.add_lottie("crates/director-core/tests/fixtures/lottie_simple.json", #{
    width: 400.0, height: 400.0,
    position: "absolute", top: 200, left: 200,
    speed: 1.5,
    "loop": true
});
lottie_node.animate("opacity", 0.0, 1.0, 1.0, "ease_out");
lottie_node.animate("scale", 0.5, 1.0, 1.2, "back_out");

// Custom SkSL shader
let shader_box = s5.add_box(#{
    width: 400.0, height: 400.0,
    position: "absolute", top: 200, right: 200
});
shader_box.apply_effect("shader", #{
    code: "half4 main(float2 pos) { return half4(pos.x/1920.0, pos.y/1080.0, 0.5, 1.0); }",
    uniforms: #{}
});
shader_box.animate("opacity", 0.0, 0.8, 2.0, "ease_in_out");

// Nested composition
let inner_movie = new_director(400, 400, 30.0);
let inner_scene = inner_movie.add_scene(2.0);
inner_scene.add_text(#{
    content: "NESTED",
    size: 48.0,
    color: "#ffffff",
    weight: "bold",
    position: "absolute",
    top: 150, width: "100%",
    justify_content: "center"
});
let comp = s5.add_composition(inner_movie, #{
    width: 400.0, height: 400.0,
    position: "absolute", bottom: 100, left: 100
});
comp.animate("opacity", 0.0, 1.0, 1.5, "ease_out");

// Scene-scoped narration audio
let scene_narration = s5.add_audio("C:/Projects/gemmy/outputs/agent_runs/agent_20260628_212629/narration.wav");
scene_narration.animate_volume(0.0, 1.0, 1.0, "linear");

// ============================================================
// SCENE 6: TRACK WORKFLOW + CLIPS + PROPERTIES + DESTROY (21-24s)
// ============================================================
let s6 = movie.add_scene(2.0);

let s6_bg = s6.add_box(#{
    width: "100%", height: "100%", bg_color: "#0a0a0f"
});

// Create a track and add scene clips manually
let track1 = movie.add_track("Bonus Track");
let clip1 = track1.add_clip(s1, 0.0, 4.0);
clip1.set_clip_fade_in(0.5);
clip1.set_clip_fade_out(0.5);
clip1.set_clip_z_index(10);

let track2 = movie.add_track("Overlay Track");
let clip2 = track2.add_clip(s3, 8.0, 4.0);
clip2.set_clip_speed(1.5);

// Property manipulation demo
let prop_box = s6_bg.add_box(#{
    width: 300.0, height: 300.0,
    bg_color: "#ff0000",
    position: "absolute", top: 100, left: 100,
    border_radius: 20.0
});
prop_box.set_pivot(0.5, 0.5);
prop_box.set_z_index(100);
prop_box.set_color("#00ff00");
prop_box.set_style(#{ border_radius: 60.0, border_width: 4.0, border_color: "#ffffff" });
prop_box.animate("rotation", 0.0, 360.0, 2.0, "ease_in_out");
prop_box.animate("scale", 1.0, 1.5, 2.0, "ease_in_out");

// set_content demo
let mutable_text = s6_bg.add_text(#{
    content: "ORIGINAL",
    size: 48.0, color: "#ffffff",
    position: "absolute", top: 500, left: 100
});
mutable_text.set_content("UPDATED TEXT");
mutable_text.animate("opacity", 0.0, 1.0, 1.0, "ease_out");

// Audio reactive binding
let reactive_orb = s6_bg.add_circle(#{
    cx: 960.0, cy: 540.0, r: 50.0,
    fill: "#ff00ff",
    position: "absolute"
});
reactive_orb.bind_audio(global_music.id(), "bass", "scale");

// set_blur instant setter
let blur_box = s6_bg.add_box(#{
    width: 200.0, height: 200.0,
    bg_color: "#00ffff",
    position: "absolute", bottom: 100, right: 100
});
blur_box.set_blur(10.0);
blur_box.animate("opacity", 0.0, 1.0, 1.5, "linear");

// destroy demo (create then remove — won't be visible)
let temp_node = s6_bg.add_box(#{
    width: 10.0, height: 10.0, bg_color: "#ffffff"
});
temp_node.destroy();

// ============================================================
// TRANSITIONS (exercise all transition types)
// ============================================================
movie.add_transition(s1, s2, "fade", 0.5, "ease_in_out");
movie.add_transition(s2, s3, "slide_left", 0.5, "ease_out");
movie.add_transition(s3, s4, "wipe_right", 0.5, "ease_in_out");
movie.add_transition(s4, s5, "circle_open", 0.8, "ease_in_out");
movie.add_wave_transition(s5, s6, 0.6, "ease_out", 20.0, 8.0);

// Glitch, iris, spiral transitions between some scenes via extra short scenes
// (We create micro-scenes just to test these transitions)
let glitch_scene = movie.add_scene(0.5);
let iris_scene = movie.add_scene(0.5);
let spiral_scene = movie.add_scene(0.5);
glitch_scene.add_box(#{ width: "100%", height: "100%", bg_color: "#000000" });
iris_scene.add_box(#{ width: "100%", height: "100%", bg_color: "#000000" });
spiral_scene.add_box(#{ width: "100%", height: "100%", bg_color: "#000000" });

movie.add_glitch_transition(s1, glitch_scene, 0.3, "linear", 0.5);
movie.add_iris_transition(glitch_scene, iris_scene, 0.3, "ease_in_out", 0.0, 500.0);
movie.add_spiral_transition(iris_scene, spiral_scene, 0.3, "ease_in_out", 2.0);

// ============================================================
// PAN + ANIMATE_PAN on global music
// ============================================================
global_music.set_pan(0.0);
global_music.animate_pan(0.0, 0.5, 3.0, "ease_in_out");

// ============================================================
// AUDIO EFFECTS: delay, eq, compressor
// ============================================================
let delay_idx = global_music.add_effect("delay", #{ time: 0.3, feedback: 0.4, wet: 0.2 });
let eq_idx = global_music.add_effect("eq", #{ type: "peak", frequency: 1000.0, q: 1.0, gain: 3.0 });
let comp_idx = global_music.add_effect("compressor", #{ threshold: -20.0, ratio: 4.0 });

// Remove the delay effect after a while (demonstrate remove_effect)
global_music.remove_effect(delay_idx);

movie
