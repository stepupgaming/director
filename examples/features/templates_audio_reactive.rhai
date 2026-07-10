// Templates + Audio Reactivity
//
// Demonstrates combining higher-level templates with audio reactivity.
// Templates return NodeHandles, so you can call set_pivot and bind_audio
// on them just like any other node.
//
// Key insight: bind_audio on a template root affects the entire template,
// making it pulse/scale/rotate as a unit. Combine this with manually
// created nodes for a rich, layered audio-visual experience.

let movie = new_director(1920, 1080, 30.0);

// Add global audio track
let music = movie.add_audio("assets/music.mp3");
let track_id = music.id();

// ============================================================
// SCENE 1: Reactive Bento Grid (8.0s)
// ============================================================
let s1 = movie.add_scene(8.0);

// Dark background
s1.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#050508"
});

// Title that pulses to the overall energy
let title = s1.add_title_card(#{
    title: "AUDIO REACTIVE",
    subtitle: "Templates + bind_audio",
    bg_color: "transparent",
    title_color: "#ffffff",
    subtitle_color: "#8888aa",
    title_size: 64.0,
    subtitle_size: 28.0
});
title.set_pivot(0.5, 0.5);
title.bind_audio(track_id, "bass", "scale");

// Bento grid with colorful cells — the whole grid scales to mids
let grid = s1.add_bento_grid(#{
    columns: 2,
    rows: 2,
    gap: 20.0,
    padding: 120.0,
    items: [
        #{
            text: "BASS",
            text_color: "#ffffff",
            bg_color: "#ff3366",
            border_radius: 20.0
        },
        #{
            text: "MIDS",
            text_color: "#ffffff",
            bg_color: "#4ecdc4",
            border_radius: 20.0
        },
        #{
            text: "HIGHS",
            text_color: "#ffffff",
            bg_color: "#3366ff",
            border_radius: 20.0
        },
        #{
            text: "ENERGY",
            text_color: "#ffffff",
            bg_color: "#ffe66d",
            border_radius: 20.0
        }
    ]
});
grid.set_pivot(0.5, 0.5);
grid.bind_audio(track_id, "mids", "scale");

// Lower third that wobbles with highs
let banner = s1.add_lower_third(#{
    text: "Reacting to audio in real-time",
    bg_color: "rgba(20, 20, 30, 0.8)",
    text_color: "#ffffff",
    text_size: 28.0,
    height: 12.0,
    accent_color: "#8b5cf6"
});
banner.bind_audio(track_id, "highs", "scale");

// ============================================================
// SCENE 2: Reactive Reaction Overlay (8.0s)
// ============================================================
let s2 = movie.add_scene(8.0);

// Background video reacting to bass
let bg = s2.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a0a0a"
});

// Note: Replace with actual video/image paths for real use
let overlay = s2.add_reaction_overlay(#{
    main: "assets/background.jpg",
    facecam: "assets/facecam.jpg",
    corner: "bottom_right",
    facecam_size: 28.0,
    facecam_margin: 24.0,
    border_radius: 16.0,
    border_width: 4.0,
    border_color: "#ffffff"
});

// The entire overlay pulses to the beat
overlay.set_pivot(0.5, 0.5);
overlay.bind_audio(track_id, "bass", "scale");

// Social handles that rotate with the music
let social = s2.add_social_handle_overlay(#{
    position: "top_left",
    bg_color: "rgba(20, 20, 30, 0.7)",
    text_color: "#ffffff",
    icon_size: 28.0,
    gap: 14.0,
    margin: 24.0,
    platforms: [
        #{ platform: "YouTube", handle: "@DirectorEngine", color: "#ff0000" },
        #{ platform: "Twitter", handle: "@director_engine", color: "#1da1f2" }
    ]
});
social.bind_audio(track_id, "mids", "rotation");

// Manual audio-reactive orb in the center
let orb_container = bg.add_box(#{
    width: "100%",
    height: "100%",
    position: "absolute",
    justify_content: "center",
    align_items: "center"
});

let orb = orb_container.add_box(#{
    width: 300.0,
    height: 300.0,
    bg_color: "#8b5cf6",
    border_radius: 150.0,
    shadow_color: "rgba(139, 92, 246, 0.5)",
    shadow_blur: 60.0
});
orb.set_pivot(0.5, 0.5);
orb.bind_audio(track_id, "bass", "scale");

// Progress bar that fills with overall energy
let bar = s2.add_progress_bar(#{
    label: "Energy Level",
    progress: 65.0,
    bar_color: "#ff3366",
    bg_color: "#1a1a2e",
    text_color: "#ffffff",
    width: 600.0,
    height: 20.0,
    border_radius: 10.0
});
bar.set_pivot(0.5, 0.5);
bar.bind_audio(track_id, "highs", "scale");

// ============================================================
// SCENE 3: Quote Card reacting to highs (6.0s)
// ============================================================
let s3 = movie.add_scene(6.0);

s3.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0f0f23",
    justify_content: "center",
    align_items: "center"
});

let quote = s3.add_quote_card(#{
    quote: "Music is the shorthand of emotion.",
    author: "Leo Tolstoy",
    bg_color: "#1a1a2e",
    text_color: "#e0e0e0",
    label_color: "#8888aa",
    accent_color: "#ff3366",
    border_radius: 24.0,
    max_width: 800.0,
    shadow_color: "rgba(0,0,0,0.3)",
    shadow_blur: 25.0
});
quote.set_pivot(0.5, 0.5);
quote.bind_audio(track_id, "mids", "scale");

// ============================================================
// Transitions
// ============================================================
movie.add_transition(s1, s2, "fade", 1.5, "ease_in_out");
movie.add_transition(s2, s3, "circle_open", 1.5, "ease_in_out");

movie
