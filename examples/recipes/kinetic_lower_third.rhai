// L3 Recipe: Kinetic lower-third package
//
// Horizontal 1920×1080 package for interview / podcast graphics:
// cold open title → smart lower third entrance → beat line → end slate.
// Uses L1 (typewriter, fly/pop) + L2 (smart lower third, title card).
//
// Props:
//   name, role, show_title, accent, bg, tagline

register_composition("kinetic_lower_third", #{
    width: 1920,
    height: 1080,
    fps: 30,
    duration: 10.0,
    default_props: #{
        name: "Jordan Lee",
        role: "Host · Night Shift",
        show_title: "UNFILTERED",
        tagline: "Ask the question they won't.",
        accent: "#f472b6",
        bg: "#020617"
    }
}, |props| {
    let movie = new_director(1920, 1080, 30);
    let name = props["name"];
    let role = props["role"];
    let show_title = props["show_title"];
    let tagline = props["tagline"];
    let accent = props["accent"];
    let bg = props["bg"];

    // Cold open
    let s0 = movie.add_scene(2.8);
    s0.add_box(#{
        width: "100%", height: "100%", bg_color: bg
    });
    let open = s0.add_box(#{
        width: "100%", height: "100%",
        flex_direction: "column",
        justify_content: "center",
        align_items: "center",
        gap: 20.0
    });
    let brand = open.add_text(#{
        content: show_title,
        size: 28.0,
        color: accent,
        weight: "bold"
    });
    brand.fade_in(0.3);

    let headline = open.add_text(#{
        content: tagline,
        size: 56.0,
        color: "#f8fafc",
        weight: "bold"
    });
    headline.typewriter(1.2, 0.15);

    // Interview plate
    let s1 = movie.add_scene(4.5);
    s1.add_box(#{
        width: "100%", height: "100%", bg_color: "#0b1224"
    });
    // faux b-roll panel
    let panel = s1.add_box(#{
        width: "42%",
        height: "70%",
        position: "absolute",
        right: 80.0,
        top: 120.0,
        border_radius: 24.0,
        bg_color: "#1e293b"
    });
    panel.ken_burns(4.5, 1.08, -18.0, 10.0);

    let lt = s1.add_smart_lower_third(#{
        name: name,
        title: role,
        accent_color: accent,
        bg_color: "#020617ee",
        margin: 64.0,
        name_size: 40.0,
        title_size: 24.0
    });
    lt.fly_in_left(0.55, 0.2, 100.0);

    // End slate
    let s2 = movie.add_scene(2.7);
    let end = s2.add_box(#{
        width: "100%", height: "100%",
        flex_direction: "column",
        justify_content: "center",
        align_items: "center",
        gap: 18.0,
        bg_color: bg
    });
    let out = end.add_text(#{
        content: show_title,
        size: 48.0,
        color: accent,
        weight: "bold"
    });
    out.pop_in(0.45);
    let bye = end.add_text(#{
        content: "Subscribe for the full episode",
        size: 28.0,
        color: "#94a3b8"
    });
    bye.fade_in(0.4, 0.25);

    movie
});
