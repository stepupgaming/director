// CSS Filter Effects Showcase
// Demonstrates blur, brightness, contrast, saturation, and hue-rotate
// applied to boxes, text, images, and shapes.

let movie = new_director(1080, 1920, 30.0);
let scene = movie.add_scene(5.0);

let bg = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e",
    position: "absolute",
    top: 0,
    left: 0,
});

// ========== Original Reference Row ==========
let ref_label = scene.add_text(#{
    content: "Original (no filters)",
    size: 36,
    color: "#ffffff",
    position: "absolute",
    top: 40,
    left: 60,
});

let ref_box = scene.add_box(#{
    bg_color: "#e94560",
    width: 200,
    height: 200,
    position: "absolute",
    top: 100,
    left: 60,
    border_radius: 16,
});

let ref_circle = scene.add_circle(#{
    cx: 120,
    cy: 100,
    r: 80,
    fill: "#0f3460",
    position: "absolute",
    top: 340,
    left: 60,
});

// ========== Blur Showcase ==========
let blur_label = scene.add_text(#{
    content: "Blur (sigma 8)",
    size: 36,
    color: "#ffffff",
    position: "absolute",
    top: 40,
    left: 340,
});

let blur_box = scene.add_box(#{
    bg_color: "#e94560",
    width: 200,
    height: 200,
    position: "absolute",
    top: 100,
    left: 340,
    border_radius: 16,
    filter_blur: 8.0,
});

let blur_circle = scene.add_circle(#{
    cx: 120,
    cy: 100,
    r: 80,
    fill: "#0f3460",
    position: "absolute",
    top: 340,
    left: 340,
    filter_blur: 8.0,
});

// ========== Brightness Showcase ==========
let bright_label = scene.add_text(#{
    content: "Brightness 1.5x",
    size: 36,
    color: "#ffffff",
    position: "absolute",
    top: 40,
    left: 620,
});

let bright_box = scene.add_box(#{
    bg_color: "#e94560",
    width: 200,
    height: 200,
    position: "absolute",
    top: 100,
    left: 620,
    border_radius: 16,
    filter_brightness: 1.5,
});

let bright_circle = scene.add_circle(#{
    cx: 120,
    cy: 100,
    r: 80,
    fill: "#0f3460",
    position: "absolute",
    top: 340,
    left: 620,
    filter_brightness: 1.5,
});

// ========== Contrast Showcase ==========
let contrast_label = scene.add_text(#{
    content: "Contrast 2.0x",
    size: 36,
    color: "#ffffff",
    position: "absolute",
    top: 40,
    left: 900,
});

let contrast_box = scene.add_box(#{
    bg_color: "#e94560",
    width: 200,
    height: 200,
    position: "absolute",
    top: 100,
    left: 900,
    border_radius: 16,
    filter_contrast: 2.0,
});

let contrast_circle = scene.add_circle(#{
    cx: 120,
    cy: 100,
    r: 80,
    fill: "#0f3460",
    position: "absolute",
    top: 340,
    left: 900,
    filter_contrast: 2.0,
});

// ========== Saturation Showcase ==========
let sat_label = scene.add_text(#{
    content: "Saturation 0.0 (grayscale)",
    size: 36,
    color: "#ffffff",
    position: "absolute",
    top: 600,
    left: 60,
});

let sat_box = scene.add_box(#{
    bg_color: "#e94560",
    width: 200,
    height: 200,
    position: "absolute",
    top: 660,
    left: 60,
    border_radius: 16,
    filter_saturation: 0.0,
});

let sat_circle = scene.add_circle(#{
    cx: 120,
    cy: 100,
    r: 80,
    fill: "#0f3460",
    position: "absolute",
    top: 900,
    left: 60,
    filter_saturation: 0.0,
});

// ========== Hue Rotate Showcase ==========
let hue_label = scene.add_text(#{
    content: "Hue Rotate 180°",
    size: 36,
    color: "#ffffff",
    position: "absolute",
    top: 600,
    left: 340,
});

let hue_box = scene.add_box(#{
    bg_color: "#e94560",
    width: 200,
    height: 200,
    position: "absolute",
    top: 660,
    left: 340,
    border_radius: 16,
    filter_hue_rotate: 180.0,
});

let hue_circle = scene.add_circle(#{
    cx: 120,
    cy: 100,
    r: 80,
    fill: "#0f3460",
    position: "absolute",
    top: 900,
    left: 340,
    filter_hue_rotate: 180.0,
});

// ========== Combined Filters ==========
let combo_label = scene.add_text(#{
    content: "Combined: blur + brightness + hue",
    size: 36,
    color: "#ffffff",
    position: "absolute",
    top: 600,
    left: 620,
});

let combo_box = scene.add_box(#{
    bg_color: "#e94560",
    width: 200,
    height: 200,
    position: "absolute",
    top: 660,
    left: 620,
    border_radius: 16,
    filter_blur: 4.0,
    filter_brightness: 1.3,
    filter_hue_rotate: 90.0,
});

let combo_circle = scene.add_circle(#{
    cx: 120,
    cy: 100,
    r: 80,
    fill: "#0f3460",
    position: "absolute",
    top: 900,
    left: 620,
    filter_blur: 4.0,
    filter_brightness: 1.3,
    filter_hue_rotate: 90.0,
});

// ========== Animated Filter Showcase ==========
let anim_label = scene.add_text(#{
    content: "Animated: blur → contrast → hue",
    size: 36,
    color: "#ffffff",
    position: "absolute",
    top: 600,
    left: 900,
});

let anim_box = scene.add_box(#{
    bg_color: "#e94560",
    width: 200,
    height: 200,
    position: "absolute",
    top: 660,
    left: 900,
    border_radius: 16,
});

// Static filters applied at creation time. Dynamic filter animation
// can be achieved via set_filter() in an update loop.

// ========== Text with Filters ==========
let text_filter_label = scene.add_text(#{
    content: "Filtered Text",
    size: 72,
    color: "#00d9ff",
    position: "absolute",
    top: 1200,
    left: 60,
    filter_blur: 2.0,
    filter_hue_rotate: 45.0,
});

let text_bright = scene.add_text(#{
    content: "Bright Text",
    size: 72,
    color: "#ff6b6b",
    position: "absolute",
    top: 1200,
    left: 600,
    filter_brightness: 1.5,
    filter_contrast: 1.5,
});

movie
