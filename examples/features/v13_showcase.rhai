// Director V1 Showcase — All 13 New Features
// A polished 18-second showcase demonstrating every new capability.

let movie = new_director(1920, 1080, 30);

// ============================================================
// FEATURE 1 & 2: letter_spacing + line_height
// ============================================================
let s_title = movie.add_scene(3.0);

s_title.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a0a1a",
    justify_content: "center",
    align_items: "center",
    flex_direction: "column"
});

s_title.add_text(#{
    content: "DIRECTOR V1",
    size: 120.0,
    color: "#ffffff",
    weight: 800,
    letter_spacing: 24.0
});

s_title.add_text(#{
    content: "13 New Features\nClosing the Remotion gap",
    size: 36.0,
    color: "#a0a0ff",
    line_height: 1.6,
    align: "center"
});

// ============================================================
// FEATURE 3: Dissolve transition
// ============================================================
let s_dissolve_a = movie.add_scene(2.0);
s_dissolve_a.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a3a5c",
    justify_content: "center",
    align_items: "center"
});
s_dissolve_a.add_text(#{ content: "DISSOLVE", size: 96.0, color: "#ffffff", weight: 800 });

let s_dissolve_b = movie.add_scene(2.0);
s_dissolve_b.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#5c1a3a",
    justify_content: "center",
    align_items: "center"
});
s_dissolve_b.add_text(#{ content: "TRANSITION", size: 96.0, color: "#ffffff", weight: 800 });

movie.add_transition(s_dissolve_a, s_dissolve_b, "dissolve", 1.0, "ease_in_out");

// ============================================================
// FEATURE 4: Flip transition
// ============================================================
let s_flip_a = movie.add_scene(2.0);
s_flip_a.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a5c3a",
    justify_content: "center",
    align_items: "center"
});
s_flip_a.add_text(#{ content: "FLIP", size: 120.0, color: "#ffffff", weight: 800 });

let s_flip_b = movie.add_scene(2.0);
s_flip_b.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#5c3a1a",
    justify_content: "center",
    align_items: "center"
});
s_flip_b.add_text(#{ content: "ROTATION", size: 120.0, color: "#ffffff", weight: 800 });

movie.add_transition(s_flip_a, s_flip_b, "flip", 1.0, "ease_in_out");

// ============================================================
// FEATURE 5: ClockWipe transition
// ============================================================
let s_clock_a = movie.add_scene(2.0);
s_clock_a.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#3a1a5c",
    justify_content: "center",
    align_items: "center"
});
s_clock_a.add_text(#{ content: "CLOCK", size: 120.0, color: "#ffffff", weight: 800 });

let s_clock_b = movie.add_scene(2.0);
s_clock_b.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a5c5c",
    justify_content: "center",
    align_items: "center"
});
s_clock_b.add_text(#{ content: "WIPE", size: 120.0, color: "#ffffff", weight: 800 });

movie.add_transition(s_clock_a, s_clock_b, "clock_wipe", 1.0, "ease_in_out");

// ============================================================
// FEATURE 6: Cube transition
// ============================================================
let s_cube_a = movie.add_scene(2.0);
s_cube_a.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#5c1a1a",
    justify_content: "center",
    align_items: "center"
});
s_cube_a.add_text(#{ content: "CUBE", size: 120.0, color: "#ffffff", weight: 800 });

let s_cube_b = movie.add_scene(2.0);
s_cube_b.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a5c",
    justify_content: "center",
    align_items: "center"
});
s_cube_b.add_text(#{ content: "3D", size: 120.0, color: "#ffffff", weight: 800 });

movie.add_transition(s_cube_a, s_cube_b, "cube", 1.0, "ease_in_out");

// ============================================================
// FEATURE 7: noise2d / noise3d
// ============================================================
let s_noise = movie.add_scene(3.0);

s_noise.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a0a0a",
    justify_content: "center",
    align_items: "center",
    flex_direction: "column"
});

s_noise.add_text(#{
    content: "NOISE",
    size: 72.0,
    color: "#ffffff",
    weight: 800,
    margin_bottom: 30.0
});

// Build a bar chart with noise-driven heights
let bar_container = s_noise.add_box(#{
    width: "60%",
    height: "30%",
    justify_content: "center",
    align_items: "flex_end",
    flex_direction: "row",
    gap: 6.0
});

for i in 0..20 {
    let h_pct = 20.0 + noise2d(i * 0.3, 0.0) * 50.0;
    let bar = bar_container.add_box(#{
        width: "4%",
        height: h_pct + "%",
        bg_color: "#4a90d9",
        border_radius: 3.0
    });
}

// ============================================================
// FEATURE 8: interpolate_color
// ============================================================
let s_color = movie.add_scene(3.0);

s_color.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a0a0a",
    justify_content: "center",
    align_items: "center",
    flex_direction: "column"
});

s_color.add_text(#{
    content: "COLOR INTERPOLATION",
    size: 64.0,
    color: "#ffffff",
    weight: 800,
    margin_bottom: 30.0
});

// Show static color swatches at different blend percentages
let c0 = interpolate_color("#ff6b6b", "#4ecdc4", 0.0);
let c25 = interpolate_color("#ff6b6b", "#4ecdc4", 0.25);
let c50 = interpolate_color("#ff6b6b", "#4ecdc4", 0.5);
let c75 = interpolate_color("#ff6b6b", "#4ecdc4", 0.75);
let c100 = interpolate_color("#ff6b6b", "#4ecdc4", 1.0);

let row = s_color.add_box(#{
    width: "80%",
    height: 120.0,
    justify_content: "center",
    align_items: "center",
    flex_direction: "row",
    gap: 12.0
});

row.add_box(#{ width: 120.0, height: 120.0, bg_color: c0, border_radius: 12.0 });
row.add_box(#{ width: 120.0, height: 120.0, bg_color: c25, border_radius: 12.0 });
row.add_box(#{ width: 120.0, height: 120.0, bg_color: c50, border_radius: 12.0 });
row.add_box(#{ width: 120.0, height: 120.0, bg_color: c75, border_radius: 12.0 });
row.add_box(#{ width: 120.0, height: 120.0, bg_color: c100, border_radius: 12.0 });

s_color.add_text(#{
    content: "0%        25%        50%        75%        100%",
    size: 20.0,
    color: "#8888aa",
    margin_top: 16.0
});

// ============================================================
// FEATURE 9: measure_text
// ============================================================
let s_measure = movie.add_scene(3.0);

s_measure.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#111111",
    justify_content: "center",
    align_items: "center",
    flex_direction: "column"
});

s_measure.add_text(#{
    content: "MEASURE",
    size: 72.0,
    color: "#ffffff",
    weight: 800,
    margin_bottom: 10.0
});

let w = measure_text("MEASURE", 72.0);

s_measure.add_box(#{
    width: w + 40.0,
    height: 6.0,
    bg_color: "#ffd700",
    margin_bottom: 10.0
});

s_measure.add_text(#{
    content: "Width: " + w + " px",
    size: 28.0,
    color: "#cccccc"
});

// ============================================================
// FEATURE 10: add_loop
// ============================================================
let s_loop = movie.add_scene(1.5);
s_loop.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a3e",
    justify_content: "center",
    align_items: "center"
});
s_loop.add_text(#{ content: "LOOP × 3", size: 96.0, color: "#ffffff", weight: 800 });

movie.add_loop(s_loop, 3);

// ============================================================
// FEATURE 11: add_freeze
// ============================================================
let s_freeze = movie.add_scene(2.0);
s_freeze.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#3a1a1a",
    justify_content: "center",
    align_items: "center"
});
s_freeze.add_text(#{ content: "FREEZE", size: 96.0, color: "#ffffff", weight: 800 });

movie.add_freeze(s_freeze, 1.5, 1.5);

// ============================================================
// FEATURE 12: add_counter (actual number interpolation)
// ============================================================
let s_counter = movie.add_scene(4.0);

s_counter.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a1a0a",
    justify_content: "center",
    align_items: "center",
    flex_direction: "column"
});

s_counter.add_counter(#{
    start: 0.0,
    end: 10000.0,
    duration: 3.5,
    decimals: 2,
    thousands_sep: true,
    prefix: "$",
    suffix: " USD",
    text_color: "#00ff88",
    font_size: 140.0,
    easing: "ease_out"
});

s_counter.add_text(#{
    content: "Number Interpolation",
    size: 28.0,
    color: "#8888aa",
    margin_top: 20.0
});

// ============================================================
// FEATURE 13: clip_path
// ============================================================
let s_clip = movie.add_scene(3.0);

let root_clip = s_clip.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a0a1a",
    justify_content: "space_evenly",
    align_items: "center",
    flex_direction: "row"
});

// Triangle
let tri = root_clip.add_box(#{
    width: 300.0,
    height: 300.0,
    bg_color: "#ff6b6b",
    clip_path: [[0.5, 0.0], [1.0, 1.0], [0.0, 1.0]]
});
tri.add_text(#{ content: "△", size: 64.0, color: "#ffffff" });

// Diamond
let dia = root_clip.add_box(#{
    width: 300.0,
    height: 300.0,
    bg_color: "#4ecdc4",
    clip_path: [[0.5, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 0.5]]
});
dia.add_text(#{ content: "◇", size: 64.0, color: "#ffffff" });

// Pentagon
let pent = root_clip.add_box(#{
    width: 300.0,
    height: 300.0,
    bg_color: "#ffe66d",
    clip_path: [[0.5, 0.0], [1.0, 0.38], [0.82, 1.0], [0.18, 1.0], [0.0, 0.38]]
});
pent.add_text(#{ content: "⬠", size: 64.0, color: "#ffffff" });

// ============================================================
// Transitions between showcase scenes
// ============================================================
movie.add_transition(s_title, s_dissolve_a, "fade", 0.8, "ease_in_out");
movie.add_transition(s_dissolve_b, s_flip_a, "fade", 0.8, "ease_in_out");
movie.add_transition(s_flip_b, s_clock_a, "fade", 0.8, "ease_in_out");
movie.add_transition(s_clock_b, s_cube_a, "fade", 0.8, "ease_in_out");
movie.add_transition(s_cube_b, s_noise, "fade", 0.8, "ease_in_out");
movie.add_transition(s_noise, s_color, "slide_left", 1.0, "ease_in_out");
movie.add_transition(s_color, s_measure, "slide_left", 1.0, "ease_in_out");
movie.add_transition(s_measure, s_loop, "fade", 0.8, "ease_in_out");
movie.add_transition(s_loop, s_freeze, "fade", 0.8, "ease_in_out");
movie.add_transition(s_freeze, s_counter, "fade", 0.8, "ease_in_out");
movie.add_transition(s_counter, s_clip, "fade", 0.8, "ease_in_out");

movie
