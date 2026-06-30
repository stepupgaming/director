// Test: Text fit modes — shrink, auto, wrap

let movie = new_director(1280, 720, 30);
let scene = movie.add_scene(2.0);

// Background
scene.add_box(#{
    bg_color: "#1a1a2e",
    width: "100%",
    height: "100%"
});

// Row container for side-by-side comparison
let row = scene.add_box(#{
    width: "100%",
    height: "100%",
    flex_direction: "row",
    align_items: "center",
    justify_content: "center",
    gap: 40
});

// --- SHRINK: large text in a small box ---
let shrink_container = row.add_box(#{
    bg_color: "#2d2d44",
    width: 200,
    height: 60,
    align_items: "center",
    justify_content: "center"
});
shrink_container.add_text(#{
    content: "Shrink Me",
    size: 48,
    color: "#ffffff",
    fit: "shrink"
});

// --- AUTO: text sizes to content width ---
let auto_container = row.add_box(#{
    bg_color: "#2d2d44",
    align_items: "center",
    justify_content: "center",
    padding: 16
});
auto_container.add_text(#{
    content: "Auto Width",
    size: 32,
    color: "#4ecdc4",
    fit: "auto"
});

// --- WRAP: text wraps at fixed width ---
let wrap_container = row.add_box(#{
    bg_color: "#2d2d44",
    width: 200,
    align_items: "center",
    justify_content: "center",
    padding: 12
});
wrap_container.add_text(#{
    content: "Wrap mode text that wraps naturally inside its container",
    size: 18,
    color: "#ff6b6b",
    fit: "wrap",
    width: 180
});

movie
