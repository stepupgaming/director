// Text, Shapes & Utilities Showcase
//
// Demonstrates features added to close the Remotion gap:
// - letter_spacing / line_height on text nodes
// - clip_path polygon on boxes
// - noise2d / noise3d for organic animation
// - interpolate_color for color blending
// - measure_text for data-driven layout

let movie = new_director(1920, 1080, 30.0);

// ============================================================
// SCENE 1: Typography (letter_spacing + line_height) (5.0s)
// ============================================================
let s1 = movie.add_scene(5.0);

s1.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0f0f23",
    justify_content: "center",
    align_items: "center",
    flex_direction: "column"
});

// Title with wide letter spacing (cinematic feel)
s1.add_text(#{
    content: "LETTER SPACING",
    size: 72.0,
    color: "#ffffff",
    weight: 800,
    letter_spacing: 8.0,
    margin_bottom: 10.0
});

// Body with custom line height
s1.add_text(#{
    content: "Line height controls the vertical rhythm\nof multi-line text blocks for better\nreadability and visual hierarchy.",
    size: 28.0,
    color: "#a0a0b0",
    line_height: 1.8,
    align: "center",
    max_width: 800.0
});

// ============================================================
// SCENE 2: Clip-Path Shapes (5.0s)
// ============================================================
let s2 = movie.add_scene(5.0);

let root2 = s2.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e",
    justify_content: "space_evenly",
    align_items: "center",
    flex_direction: "row"
});

// Triangle
let tri = root2.add_box(#{
    width: 300.0,
    height: 300.0,
    bg_color: "#ff6b6b",
    clip_path: [[0.5, 0.0], [1.0, 1.0], [0.0, 1.0]]
});
tri.add_text(#{ content: "△", size: 48.0, color: "#ffffff" });

// Diamond
let diamond = root2.add_box(#{
    width: 300.0,
    height: 300.0,
    bg_color: "#4ecdc4",
    clip_path: [[0.5, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 0.5]]
});
diamond.add_text(#{ content: "◇", size: 48.0, color: "#ffffff" });

// Pentagon
let pent = root2.add_box(#{
    width: 300.0,
    height: 300.0,
    bg_color: "#9b59b6",
    clip_path: [[0.5, 0.0], [1.0, 0.38], [0.82, 1.0], [0.18, 1.0], [0.0, 0.38]]
});
pent.add_text(#{ content: "⬠", size: 48.0, color: "#ffffff" });

// ============================================================
// SCENE 3: Noise & Color Utilities (5.0s)
// ============================================================
let s3 = movie.add_scene(5.0);

s3.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a0a0a",
    justify_content: "center",
    align_items: "center",
    flex_direction: "column"
});

// Demonstrate noise2d producing organic values
let n1 = noise2d(0.5, 1.0);
let n2 = noise2d(1.5, 1.0);
let n3 = noise2d(2.5, 1.0);

// Demonstrate interpolate_color
let mid_color = interpolate_color("#ff6b6b", "#4ecdc4", 0.5);
let end_color = interpolate_color("#ff6b6b", "#4ecdc4", 1.0);

// Measure text width for dynamic layout
let text_w = measure_text("MEASURED TEXT", 48.0);

s3.add_text(#{
    content: "NOISE & COLOR UTILITIES",
    size: 52.0,
    color: "#ffffff",
    weight: 800,
    margin_bottom: 30.0
});

// Color interpolation display
let color_box = s3.add_box(#{
    width: 400.0,
    height: 80.0,
    bg_color: mid_color,
    margin_bottom: 10.0,
    border_radius: 10.0
});
color_box.add_text(#{ content: "50% blend", size: 24.0, color: "#ffffff" });

let end_box = s3.add_box(#{
    width: 400.0,
    height: 80.0,
    bg_color: end_color,
    margin_bottom: 10.0,
    border_radius: 10.0
});
end_box.add_text(#{ content: "100% blend", size: 24.0, color: "#ffffff" });

// Show measured text width
s3.add_text(#{
    content: "Measured width: " + text_w.to_string() + " px",
    size: 20.0,
    color: "#8888aa"
});

// ============================================================
// Transitions
// ============================================================
movie.add_transition(s1, s2, "fade", 1.0, "ease_in_out");
movie.add_transition(s2, s3, "slide_left", 1.0, "ease_in_out");

movie
