// L3 Recipe: Quote card
//
// Vertical 1080×1920. Quote types on; attribution fades after.
// Props: quote, attribution, accent, bg

register_composition("quote_card", #{
    width: 1080,
    height: 1920,
    fps: 30,
    duration: 6.5,
    default_props: #{
        quote: "Make the next cut obvious.",
        attribution: "Night edit",
        accent: "#fbbf24",
        bg: "#0c0a09"
    }
}, |props| {
    let movie = new_director(1080, 1920, 30);
    let quote = props["quote"];
    let attribution = props["attribution"];
    let accent = props["accent"];
    let bg = props["bg"];

    let scene = movie.add_scene(6.5);
    let root = scene.add_box(#{
        width: "100%",
        height: "100%",
        flex_direction: "column",
        justify_content: "center",
        align_items: "stretch",
        gap: 36.0,
        padding: 80.0,
        bg_color: bg
    });

    let bar = root.add_box(#{
        width: 88.0,
        height: 8.0,
        border_radius: 4.0,
        bg_color: accent
    });
    bar.fade_in(0.3);

    let body = root.add_text(#{
        content: quote,
        width: "100%",
        size: 64.0,
        color: "#fafaf9",
        weight: "bold"
    });
    body.typewriter(2.2, 0.25);

    let by = root.add_text(#{
        content: attribution,
        size: 32.0,
        color: accent
    });
    by.fade_in(0.45, 2.6);

    movie
});
