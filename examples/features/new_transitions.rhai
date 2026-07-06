// New Transitions Showcase
//
// Demonstrates the 4 new transitions added to close the Remotion gap:
// - Dissolve: Crossfade with progressive blur (classic film dissolve)
// - Flip: 3D card-flip effect rotating on the X-axis
// - ClockWipe: Radial sweep from 12 o'clock, clockwise
// - Cube: 3D cube rotation between scenes

let movie = new_director(1920, 1080, 30);

// ============================================================
// SCENE 1: Red (3.0s)
// ============================================================
let s1 = movie.add_scene(3.0);
let root1 = s1.add_box(#{
    width: "100%",
    height: "100%",
    justify_content: "center",
    align_items: "center",
    bg_color: "#e74c3c"
});
root1.add_text(#{ content: "DISSOLVE", size: 96.0, color: "#ffffff", weight: 800 });
root1.add_text(#{ content: "Crossfade + blur transition", size: 36.0, color: "#ffcccc" });

// ============================================================
// SCENE 2: Teal (3.0s)
// ============================================================
let s2 = movie.add_scene(3.0);
let root2 = s2.add_box(#{
    width: "100%",
    height: "100%",
    justify_content: "center",
    align_items: "center",
    bg_color: "#1abc9c"
});
root2.add_text(#{ content: "FLIP", size: 96.0, color: "#ffffff", weight: 800 });
root2.add_text(#{ content: "3D card flip on X-axis", size: 36.0, color: "#ccffee" });

// ============================================================
// SCENE 3: Purple (3.0s)
// ============================================================
let s3 = movie.add_scene(3.0);
let root3 = s3.add_box(#{
    width: "100%",
    height: "100%",
    justify_content: "center",
    align_items: "center",
    bg_color: "#9b59b6"
});
root3.add_text(#{ content: "CLOCK WIPE", size: 96.0, color: "#ffffff", weight: 800 });
root3.add_text(#{ content: "Radial sweep from 12 o'clock", size: 36.0, color: "#eeccff" });

// ============================================================
// SCENE 4: Orange (3.0s)
// ============================================================
let s4 = movie.add_scene(3.0);
let root4 = s4.add_box(#{
    width: "100%",
    height: "100%",
    justify_content: "center",
    align_items: "center",
    bg_color: "#e67e22"
});
root4.add_text(#{ content: "CUBE", size: 96.0, color: "#ffffff", weight: 800 });
root4.add_text(#{ content: "3D cube rotation transition", size: 36.0, color: "#ffe5cc" });

// ============================================================
// SCENE 5: Dark (3.0s)
// ============================================================
let s5 = movie.add_scene(3.0);
let root5 = s5.add_box(#{
    width: "100%",
    height: "100%",
    justify_content: "center",
    align_items: "center",
    bg_color: "#2c3e50"
});
root5.add_text(#{ content: "END", size: 96.0, color: "#ffffff", weight: 800 });

// ============================================================
// Transitions
// ============================================================
// movie.add_transition(from, to, type, duration, easing)
movie.add_transition(s1, s2, "dissolve", 1.2, "ease_in_out");
movie.add_transition(s2, s3, "flip", 1.0, "ease_in_out");
movie.add_transition(s3, s4, "clock_wipe", 1.2, "ease_in_out");
movie.add_transition(s4, s5, "cube", 1.0, "ease_in_out");

movie
