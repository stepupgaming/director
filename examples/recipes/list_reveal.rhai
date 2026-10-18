// L3 Recipe: List reveal
//
// Horizontal 1920×1080. Three lines fly in on a stagger.
// Props: line_1, line_2, line_3, accent, bg

register_composition("list_reveal", #{
    width: 1920,
    height: 1080,
    fps: 30,
    duration: 5.5,
    default_props: #{
        line_1: "Open with the claim",
        line_2: "Show the proof",
        line_3: "Leave the next step",
        accent: "#34d399",
        bg: "#020617"
    }
}, |props| {
    let movie = new_director(1920, 1080, 30);
    let line_1 = props["line_1"];
    let line_2 = props["line_2"];
    let line_3 = props["line_3"];
    let accent = props["accent"];
    let bg = props["bg"];

    let scene = movie.add_scene(5.5);
    let root = scene.add_box(#{
        width: "100%",
        height: "100%",
        flex_direction: "column",
        justify_content: "center",
        align_items: "flex_start",
        gap: 32.0,
        padding: 120.0,
        bg_color: bg
    });

    let a = root.add_text(#{
        content: line_1,
        size: 56.0,
        color: "#f8fafc",
        weight: "bold"
    });
    a.fly_in_left(0.45, 0.2, 90.0);

    let b = root.add_text(#{
        content: line_2,
        size: 56.0,
        color: "#f8fafc",
        weight: "bold"
    });
    b.fly_in_left(0.45, 0.8, 90.0);

    let c = root.add_text(#{
        content: line_3,
        size: 56.0,
        color: accent,
        weight: "bold"
    });
    c.fly_in_left(0.45, 1.4, 90.0);

    movie
});
