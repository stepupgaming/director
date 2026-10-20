// New Templates & Scene Primitives Showcase
//
// Demonstrates features added to close the Remotion gap:
// - add_counter: Number ticker with formatting
// - add_waveform_visualizer: Audio-reactive bar visualizer
// - add_loop: Repeat a scene N times
// - add_freeze: Hold a scene at a specific frame

let movie = new_director(1920, 1080, 30);

// ============================================================
// SCENE 1: Counter / Number Ticker (4.0s)
// ============================================================
let s1 = movie.add_scene(4.0);

s1.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0f0f23",
    justify_content: "center",
    align_items: "center"
});

// Counter with thousands separator and dollar prefix
s1.add_counter(#{
    start: 0.0,
    end: 9999.0,
    duration: 3.0,
    decimals: 0,
    prefix: "$",
    suffix: "",
    thousands_sep: true,
    text_color: "#ffffff",
    font_size: 120.0,
    easing: "ease_out"
});

// Subtitle
s1.add_text(#{
    content: "Total Revenue",
    size: 32.0,
    color: "#8888aa",
    margin_top: 20.0
});

// ============================================================
// SCENE 2: Loop Scene (repeated 3 times)
// ============================================================
let s2 = movie.add_scene(1.5);

s2.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e",
    justify_content: "center",
    align_items: "center"
});

s2.add_text(#{ content: "LOOP DEMO", size: 72.0, color: "#ffffff", weight: 800 });

// Repeat scene 2 three times (total 4.5s)
movie.add_loop(s2, 3);

// ============================================================
// SCENE 3: Waveform Visualizer (5.0s) — FFT-driven spectrum
// ============================================================
let s3 = movie.add_scene(5.0);

// Add audio for true reactivity
let music = movie.add_audio("examples/assets/tone.wav");
let track_id = music.id();

// Background
s3.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a0a0a",
    justify_content: "center",
    align_items: "center"
});

// Title
s3.add_text(#{
    content: "SPECTRUM VISUALIZER",
    size: 42.0,
    color: "#ffffff",
    weight: 700,
    margin_bottom: 40.0
});

// Audio-reactive spectrum bars driven by FFT
s3.add_waveform_visualizer(#{
    track_id: track_id,
    bars: 40,
    bar_color: "#00d4ff",
    gap: 3.0,
    max_height: 70.0,
    min_height: 5.0,
    position: "center",
    smoothing: 0.55,
    border_radius: 4.0
});

// ============================================================
// SCENE 4: Freeze Scene (hold frame)
// ============================================================
let s4 = movie.add_scene(2.0);

s4.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#2c3e50",
    justify_content: "center",
    align_items: "center"
});

s4.add_text(#{ content: "BEFORE FREEZE", size: 64.0, color: "#ffffff", weight: 800 });

// Hold the last frame of s4 for an additional 2 seconds
movie.add_freeze(s4, 2.0, 2.0);

// ============================================================
// Transitions
// ============================================================
movie.add_transition(s1, s2, "fade", 1.0, "ease_in_out");
movie.add_transition(s2, s3, "slide_left", 1.0, "ease_in_out");
movie.add_transition(s3, s4, "fade", 1.0, "ease_in_out");

movie
