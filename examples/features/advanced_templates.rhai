// Advanced Templates Showcase
//
// Demonstrates the newer higher-level templates for short-form video:
// - Subscribe Button
// - Progress Bar
// - Countdown Timer
// - Quote Card
// - Social Handle Overlay
//
// These build on the basic templates (title_card, lower_third, etc.)
// and solve even more common short-form layout problems.

let movie = new_director(1920, 1080, 30.0);

// ============================================================
// SCENE 1: Subscribe Button (4.0s)
// ============================================================
let s1 = movie.add_scene(4.0);

s1.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0f0f23"
});

s1.add_subscribe_button(#{
    channel_name: "Director Engine",
    sub_count: "128K",
    button_text: "SUBSCRIBE",
    bg_color: "#1a1a2e",
    button_color: "#ff3366",
    text_color: "#ffffff",
    position: "bottom_center",
    width: 420.0,
    height: 72.0,
    border_radius: 36.0,
    shadow_color: "rgba(0,0,0,0.4)",
    shadow_blur: 20.0,
    shadow_y: 8.0
});

// ============================================================
// SCENE 2: Progress Bar (3.0s)
// ============================================================
let s2 = movie.add_scene(3.0);

s2.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a0a0a",
    justify_content: "center",
    align_items: "center",
    flex_direction: "column"
});

s2.add_progress_bar(#{
    label: "Rendering",
    progress: 73.0,
    bar_color: "#4ecdc4",
    bg_color: "#1a1a2e",
    text_color: "#ffffff",
    width: 700.0,
    height: 28.0,
    border_radius: 14.0
});

// ============================================================
// SCENE 3: Countdown Timer (4.0s)
// ============================================================
let s3 = movie.add_scene(4.0);

s3.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#050508",
    justify_content: "center",
    align_items: "center"
});

s3.add_countdown_timer(#{
    header: "LAUNCHING IN",
    days: 2,
    hours: 14,
    minutes: 35,
    seconds: 42,
    segment_bg: "#1a1a2e",
    text_color: "#ffffff",
    label_color: "#8888aa",
    gap: 16.0
});

// ============================================================
// SCENE 4: Quote Card (5.0s)
// ============================================================
let s4 = movie.add_scene(5.0);

s4.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0f0f23",
    justify_content: "center",
    align_items: "center"
});

s4.add_quote_card(#{
    quote: "The best way to predict the future is to invent it.",
    author: "Alan Kay",
    source: "Computer Scientist",
    bg_color: "#1a1a2e",
    text_color: "#e0e0e0",
    label_color: "#8888aa",
    accent_color: "#8b5cf6",
    border_radius: 24.0,
    max_width: 900.0,
    shadow_color: "rgba(0,0,0,0.3)",
    shadow_blur: 25.0,
    shadow_y: 10.0
});

// ============================================================
// SCENE 5: Social Handle Overlay (4.0s)
// ============================================================
let s5 = movie.add_scene(4.0);

s5.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a0a0a"
});

s5.add_social_handle_overlay(#{
    position: "bottom_left",
    bg_color: "rgba(20,20,30,0.8)",
    text_color: "#ffffff",
    icon_size: 24.0,
    gap: 10.0,
    margin: 24.0,
    platforms: [
        #{ platform: "YouTube", handle: "@DirectorEngine", color: "#ff0000" },
        #{ platform: "Twitter", handle: "@director_engine", color: "#1da1f2" },
        #{ platform: "GitHub", handle: "director-engine", color: "#333333" },
        #{ platform: "Discord", handle: "discord.gg/director", color: "#5865f2" }
    ]
});

// ============================================================
// Transitions
// ============================================================
movie.add_transition(s1, s2, "fade", 1.0, "ease_in_out");
movie.add_transition(s2, s3, "slide_left", 1.0, "ease_in_out");
movie.add_transition(s3, s4, "fade", 1.0, "ease_in_out");
movie.add_transition(s4, s5, "slide_right", 1.0, "ease_in_out");

movie
