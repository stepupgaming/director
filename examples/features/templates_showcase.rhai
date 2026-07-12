// Templates Showcase
//
// Demonstrates the higher-level template primitives for short-form video:
// - Title Card
// - Lower Third
// - Split Screen
// - Reaction Overlay
// - Bento Grid
// - Watermark
//
// Templates are sugar over add_box/add_text/add_image that solve common
// layout problems without manual flexbox/grid configuration.

let movie = new_director(1920, 1080, 30);

// ============================================================
// SCENE 1: Title Card (3.0s)
// ============================================================
let s1 = movie.add_scene(3.0);

s1.add_title_card(#{
    title: "Director Templates",
    subtitle: "Higher-level layout primitives",
    bg_color: "#0f0f23",
    title_color: "#ffffff",
    subtitle_color: "#a0a0b0",
    title_size: 96.0,
    subtitle_size: 42.0,
    shadow_color: "rgba(139, 92, 246, 0.3)",
    shadow_blur: 30.0,
    shadow_y: 10.0
});

// ============================================================
// SCENE 2: Lower Third (4.0s)
// ============================================================
let s2 = movie.add_scene(4.0);

// Background so the lower third stands out
s2.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e"
});

s2.add_lower_third(#{
    text: "Subscribe for more content like this!",
    bg_color: "#ff3366",
    text_color: "#ffffff",
    text_size: 36.0,
    height: 15.0,
    accent_color: "#ffffff"
});

// ============================================================
// SCENE 3: Split Screen (5.0s)
// ============================================================
let s3 = movie.add_scene(5.0);

// Note: Replace "left.jpg" and "right.jpg" with your actual image paths
s3.add_split_screen(#{
    left: "assets/left.jpg",
    right: "assets/right.jpg",
    ratio: 0.5,
    gap: 8.0,
    vertical: false
});

// ============================================================
// SCENE 4: Reaction Overlay (6.0s)
// ============================================================
let s4 = movie.add_scene(6.0);

// Note: Replace "main.jpg" and "facecam.jpg" with your actual image paths
s4.add_reaction_overlay(#{
    main: "assets/main.jpg",
    facecam: "assets/facecam.jpg",
    corner: "bottom_right",
    facecam_size: 25.0,
    facecam_margin: 20.0,
    border_radius: 16.0,
    border_width: 3.0,
    border_color: "#ffffff"
});

// ============================================================
// SCENE 5: Bento Grid (5.0s)
// ============================================================
let s5 = movie.add_scene(5.0);

s5.add_bento_grid(#{
    columns: 2,
    rows: 2,
    gap: 16.0,
    padding: 24.0,
    items: [
        #{
            text: "Feature One",
            text_color: "#ffffff",
            bg_color: "#ff6b6b",
            border_radius: 16.0
        },
        #{
            text: "Feature Two",
            text_color: "#ffffff",
            bg_color: "#4ecdc4",
            border_radius: 16.0
        },
        #{
            text: "Feature Three",
            text_color: "#ffffff",
            bg_color: "#ffe66d",
            border_radius: 16.0
        },
        #{
            text: "Feature Four",
            text_color: "#ffffff",
            bg_color: "#a29bfe",
            border_radius: 16.0
        }
    ]
});

// ============================================================
// SCENE 6: Watermark (3.0s)
// ============================================================
let s6 = movie.add_scene(3.0);

s6.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a0a0a"
});

// Text watermark
s6.add_watermark(#{
    text: "@DirectorEngine",
    corner: "bottom_right",
    color: "#ffffff",
    text_size: 18.0,
    opacity: 0.5,
    margin: 24.0
});

// Image watermark (commented out — uncomment and set path to use)
// s6.add_watermark(#{
//     image: "assets/logo.png",
//     corner: "top_left",
//     size: 48.0,
//     opacity: 0.7,
//     margin: 16.0
// });

// ============================================================
// Transitions between scenes
// ============================================================
movie.add_transition(s1, s2, "fade", 1.0, "ease_in_out");
movie.add_transition(s2, s3, "slide_left", 1.0, "ease_in_out");
movie.add_transition(s3, s4, "fade", 1.0, "ease_in_out");
movie.add_transition(s4, s5, "slide_right", 1.0, "ease_in_out");
movie.add_transition(s5, s6, "fade", 1.0, "ease_in_out");

movie
