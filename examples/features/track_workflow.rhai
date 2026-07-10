// Track-Based Workflow
//
// Demonstrates the track-based compositing model for video editing:
// - Creating named tracks
// - Adding scenes directly to tracks
// - Adding clips with timing control
// - Track opacity and z-index layering
// - Track introspection
//
// Tracks are a flat layer stack (Track 0 = background, Track 1 = content, etc.)
// that provide global z-ordering without scene graph nesting limitations.

let movie = new_director(1920, 1080, 30.0);

// ============================================================
// SETUP TRACKS
// ============================================================

// Background track (Track 0)
let bg_track = movie.add_track("Background");

// Content track (Track 1) — sits above background
let content_track = movie.add_track("Content");

// Overlay track (Track 2) — sits above everything
let overlay_track = movie.add_track("Overlays");

// ============================================================
// BACKGROUND TRACK: Two background clips
// ============================================================

let bg1 = movie.add_scene_on_track(bg_track, 4.0);
bg1.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0f0f23"
});

let bg2 = movie.add_scene_on_track(bg_track, 4.0);
bg2.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e"
});

// ============================================================
// CONTENT TRACK: Title cards with lower thirds
// ============================================================

let content1 = movie.add_scene_on_track(content_track, 4.0);
content1.add_title_card(#{
    title: "Part One",
    subtitle: "Introduction",
    bg_color: "#0f0f23",
    title_color: "#ffffff"
});

let content2 = movie.add_scene_on_track(content_track, 4.0);
content2.add_title_card(#{
    title: "Part Two",
    subtitle: "The Main Event",
    bg_color: "#1a1a2e",
    title_color: "#ffffff"
});

// ============================================================
// OVERLAY TRACK: Lower thirds that float above content
// ============================================================

let overlay1 = movie.add_scene_on_track(overlay_track, 4.0);
overlay1.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "transparent"
});
overlay1.add_lower_third(#{
    text: "Welcome to the show",
    bg_color: "#ff3366",
    text_color: "#ffffff",
    height: 12.0
});

let overlay2 = movie.add_scene_on_track(overlay_track, 4.0);
overlay2.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "transparent"
});
overlay2.add_lower_third(#{
    text: "Don't forget to subscribe!",
    bg_color: "#4ecdc4",
    text_color: "#ffffff",
    height: 12.0
});

// ============================================================
// DEMONSTRATE CLIP MANIPULATION
// ============================================================

// Add a manual clip on the overlay track with a specific start time
let manual_scene = movie.add_scene(3.0);
manual_scene.add_watermark(#{
    text: "MANUAL CLIP",
    corner: "top_right",
    color: "#ffffff",
    text_size: 14.0,
    opacity: 0.4
});

let manual_clip = content_track.add_clip(manual_scene, 8.0, 3.0);
manual_clip.set_clip_z_index(100);

// ============================================================
// TRACK INTROSPECTION
// ============================================================

// List clips on the overlay track
let overlay_clips = overlay_track.list_clips();
for clip_info in overlay_clips {
    print(`Clip: ${clip_info.start_time}s - ${clip_info.duration}s`);
}

// Get clip count per track
let bg_count = bg_track.get_track_clip_count();
let content_count = content_track.get_track_clip_count();
let overlay_count = overlay_track.get_track_clip_count();
print(`Background clips: ${bg_count}`);
print(`Content clips: ${content_count}`);
print(`Overlay clips: ${overlay_count}`);

// ============================================================
// TRANSITIONS
// ============================================================

movie.add_transition(bg1, bg2, "fade", 1.0, "ease_in_out");
movie.add_transition(content1, content2, "slide_left", 1.0, "ease_in_out");

movie
