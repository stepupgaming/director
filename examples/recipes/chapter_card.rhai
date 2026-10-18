// L3 Recipe: Chapter card
//
// Horizontal 1920×1080. Index and title fly in from the left.
// Props: kicker, title, index, accent, bg

register_composition("chapter_card", #{
    width: 1920,
    height: 1080,
    fps: 30,
    duration: 6.4,
    default_props: #{
        kicker: "Chapter",
        title: "The hard cut",
        index: "02",
        accent: "#818cf8",
        bg: "#0b1020"
    }
}, |props| {
    let movie = new_director(1920, 1080, 30);
    let kicker = props["kicker"];
    let title = props["title"];
    let index = props["index"];
    let accent = props["accent"];
    let bg = props["bg"];

    let scene = movie.add_scene(6.4);
    let root = scene.add_box(#{
        width: "100%",
        height: "100%",
        flex_direction: "row",
        justify_content: "flex_start",
        align_items: "center",
        gap: 64.0,
        padding: 120.0,
        bg_color: bg
    });

    let num = root.add_text(#{
        content: index,
        size: 140.0,
        color: accent,
        weight: "bold"
    });
    num.fly_in_left(0.45, 0.15, 90.0);

    let stack = root.add_box(#{
        flex_direction: "column",
        justify_content: "center",
        align_items: "flex_start",
        gap: 20.0
    });

    let kick = stack.add_text(#{
        content: kicker,
        size: 28.0,
        color: accent,
        weight: "bold"
    });
    kick.fade_in(0.35);

    let bar = stack.add_box(#{
        width: 72.0,
        height: 6.0,
        border_radius: 3.0,
        bg_color: accent
    });
    bar.fade_in(0.3, 0.2);

    let head = stack.add_text(#{
        content: title,
        size: 72.0,
        color: "#f8fafc",
        weight: "bold"
    });
    head.fly_in_left(0.45, 0.35, 90.0);

    movie
});
