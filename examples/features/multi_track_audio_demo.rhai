// Multi-Track Timeline + Audio Effects Demo
//
// Demonstrates:
// - Multiple tracks with overlapping clips
// - Clip speed, range, and start-time manipulation
// - Audio pan, reverb, delay, EQ, and compressor effects
// - Track enable/disable and opacity control

let movie = new_director(1920, 1080, 30);

// ============================================================
// SCENE DEFINITIONS
// ============================================================

// Scene A: Red box (plays first on default track)
let scene_a = movie.add_scene(3.0);
let box_a = scene_a.add_box(#{ width: "100%", height: "100%", bg_color: "#ff6b6b",
    justify_content: "center", align_items: "center" });
box_a.add_text(#{ content: "Scene A — Main Track", size: 48.0, color: "#ffffff" });

// Scene B: Blue box (added to second track as overlay)
let scene_b = movie.add_scene(3.0);
let box_b = scene_b.add_box(#{ width: "100%", height: "100%", bg_color: "#4ecdc4",
    justify_content: "center", align_items: "center" });
box_b.add_text(#{ content: "Scene B — Track 2 (overlay)", size: 42.0, color: "#ffffff" });

// Scene C: Purple box (start later with speed change)
let scene_c = movie.add_scene(3.0);
let box_c = scene_c.add_box(#{ width: "100%", height: "100%", bg_color: "#845ec2",
    justify_content: "center", align_items: "center" });
box_c.add_text(#{ content: "Scene C — Slow Motion", size: 48.0, color: "#ffffff" });

// Scene D: Yellow box (trimmed source range)
let scene_d = movie.add_scene(3.0);
let box_d = scene_d.add_box(#{ width: "100%", height: "100%", bg_color: "#ffc75f",
    justify_content: "center", align_items: "center" });
box_d.add_text(#{ content: "Scene D — Range Trimmed", size: 42.0, color: "#000000" });

// ============================================================
// MULTI-TRACK TIMELINE — adjust default track clips
// ============================================================

// Default track already has scene_a (0s), scene_b (3s), scene_c (6s), scene_d (9s).
// Let's reposition and modify clips on the default track.

let track_0 = movie.get_track(0);
set_track_opacity(track_0, 0.9);

// Scene C: start at 3.0 with scene's default duration, 0.5x speed
let c_clip = add_clip(track_0, scene_c, 3.0, 4.0);
set_clip_speed(c_clip, 0.5);

// Scene D: start at 7.0, trimmed to 2s
let d_clip = add_clip(track_0, scene_d, 7.0, 2.0);
set_clip_range(d_clip, 0.5, 2.5);

// ============================================================
// TRACK 2: Overlay track with scene_b
// ============================================================
let track_2 = movie.add_track("Overlay");
add_clip(track_2, scene_b, 0.5, 4.0);
set_track_opacity(track_2, 0.8);

// ============================================================
// AUDIO EFFECTS DEMO
// ============================================================

// Add audio (skipped silently if file doesn't exist — visual demo still works)
let music = movie.add_audio("examples/assets/tone.wav");
let track_id = music.id();

// --- Pan animation ---
music.set_pan(0.5);                     // Start center
music.animate_pan(0.5, 1.0, 2.0, "ease_in_out");  // → right
music.animate_pan(1.0, 0.0, 2.0, "ease_in_out");   // → left
music.animate_pan(0.0, 0.5, 2.0, "ease_out");      // → center

// --- Reverb effect with animated wet mix ---
let reverb_idx = music.add_effect("reverb", #{ decay: 0.6, wet: 0.3, dry: 0.7 });
music.animate_effect_param(reverb_idx, "wet", 0.3, 0.8, 3.0, "ease_in_out");

// --- Delay effect ---
let delay_idx = music.add_effect("delay", #{ delay_time: 0.25, feedback: 0.4, wet: 0.2 });

// --- EQ: bass shelf boost + lowpass cut ---
let eq_idx = music.add_effect("eq", #{ type: "low_shelf", frequency: 200.0, gain: 4.0 });
let high_cut = music.add_effect("filter", #{ type: "lowpass", frequency: 8000.0, q: 0.7 });

// --- Compressor ---
let comp_idx = music.add_effect("compressor", #{
    threshold: -18.0, ratio: 3.0, knee: 6.0,
    attack: 0.005, release: 0.1, makeup_gain: 2.0
});

// Uncomment to remove high-cut filter mid-scene:
// if time >= 4.0 { music.remove_effect(high_cut); }

// ============================================================
// AUDIO-REACTIVE ELEMENT
// ============================================================
let bass_orb = box_a.add_box(#{
    width: 80.0, height: 80.0,
    bg_color: "#ffffff40", border_radius: 40.0,
    position: "absolute", bottom: 100.0
});
bass_orb.set_pivot(0.5, 0.5);
bass_orb.bind_audio(track_id, "bass", "scale");

// ============================================================
// TRANSITION
// ============================================================
movie.add_transition(scene_c, scene_d, "fade", 0.8, "ease_in_out");

movie
