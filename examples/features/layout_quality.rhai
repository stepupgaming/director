// Layout Quality Showcase
//
// Demonstrates the new layout features that close the gap with Remotion:
//   - Radial gradients
//   - Conic (sweep) gradients
//   - Backdrop blur (glassmorphism)
//   - Multiple/stacked shadows
//   - aspect_ratio property
//   - Series/Sequence helpers for sequential scene building

// Use the new Sequence builder to chain scenes back-to-back
// (mirrors Remotion's <Series> primitive)
let movie = new_director(1920, 1080, 30);
let seq = add_sequence(movie);

// ---- Scene 1: Glassmorphism (backdrop blur) ----
let s1 = seq.next(3.0);
let s1_bg = s1.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "column",
    justify_content: "center",
    align_items: "center",
    gap: 40.0,
    bg_gradient: #{
        colors: ["#1e3a5f", "#2d6a8e", "#0f2027"],
        type: "radial",
        center: [0.5, 0.3],
        radius: 1.0
    }
});

// Glass card with backdrop blur
let glass_card = s1_bg.add_box(#{
    width: 600.0,
    height: 400.0,
    flex_direction: "column",
    justify_content: "center",
    align_items: "center",
    gap: 20.0,
    bg_color: "#ffffff26",
    border_radius: 24.0,
    backdrop_blur: 20.0,
    border_width: 1.0,
    border_color: "#ffffff44",
    box_shadows: [
        #{ color: "#00000055", blur: 40.0, x: 0.0, y: 20.0 },
        #{ color: "#00000033", blur: 80.0, x: 0.0, y: 40.0 }
    ]
});

glass_card.add_text(#{
    content: "Glassmorphism",
    font_size: 48.0,
    color: "#ffffff",
    font_weight: "bold"
});
glass_card.add_text(#{
    content: "backdrop_blur + stacked shadows",
    font_size: 22.0,
    color: "#ffffffaa"
});

// ---- Scene 2: Conic gradient + aspect_ratio ----
let s2 = seq.next(3.0);
let s2_bg = s2.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "row",
    justify_content: "space_evenly",
    align_items: "center",
    bg_color: "#0a0a0a"
});

// Conic gradient card with aspect_ratio
let conic_card = s2_bg.add_box(#{
    width: 500.0,
    aspect_ratio: "1:1",
    flex_direction: "column",
    justify_content: "center",
    align_items: "center",
    bg_gradient: #{
        colors: ["#ff006e", "#fb5607", "#ffbe0b", "#8338ec", "#3a86ff", "#ff006e"],
        type: "conic",
        center: [0.5, 0.5],
        angle: 0.0
    },
    border_radius: 250.0,
    box_shadows: [
        #{ color: "#ff006e44", blur: 30.0, x: 0.0, y: 0.0 },
        #{ color: "#3a86ff44", blur: 60.0, x: 10.0, y: 10.0 }
    ]
});
conic_card.add_text(#{
    content: "Conic",
    font_size: 40.0,
    color: "#ffffff",
    font_weight: "bold"
});

// Radial gradient card
let radial_card = s2_bg.add_box(#{
    width: 500.0,
    aspect_ratio: "1:1",
    flex_direction: "column",
    justify_content: "center",
    align_items: "center",
    bg_gradient: #{
        colors: ["#00ff88", "#00aa44", "#003311"],
        type: "radial",
        center: [0.5, 0.5],
        radius: 0.6
    },
    border_radius: 250.0
});
radial_card.add_text(#{
    content: "Radial",
    font_size: 40.0,
    color: "#ffffff",
    font_weight: "bold"
});

// ---- Scene 3: Series/Sequence demo with multiple stacked shadows ----
let s3 = seq.next(4.0);
let s3_bg = s3.add_box(#{
    width: "100%", height: "100%",
    flex_direction: "column",
    justify_content: "center",
    align_items: "center",
    gap: 60.0,
    bg_color: "#f0f0f5"
});

s3_bg.add_text(#{
    content: "Stacked Shadow Depth",
    font_size: 42.0,
    color: "#1a1a2e",
    font_weight: "bold"
});

// Card with 3 stacked shadows for realistic depth
let depth_card = s3_bg.add_box(#{
    width: 500.0,
    height: 300.0,
    flex_direction: "column",
    justify_content: "center",
    align_items: "center",
    bg_color: "#ffffff",
    border_radius: 20.0,
    box_shadows: [
        #{ color: "#0000001a", blur: 10.0, x: 0.0, y: 4.0 },
        #{ color: "#0000001a", blur: 20.0, x: 0.0, y: 10.0 },
        #{ color: "#0000000d", blur: 40.0, x: 0.0, y: 20.0 }
    ]
});
depth_card.add_text(#{
    content: "3-layer shadow",
    font_size: 28.0,
    color: "#333333",
    font_weight: "bold"
});

// Animate the card floating
depth_card.animate("y", 0.0, -20.0, 2.0, "ease_in_out");

s3_bg.add_text(#{
    content: "Built with add_sequence() — no manual start_time needed",
    font_size: 18.0,
    color: "#666666"
});

movie
