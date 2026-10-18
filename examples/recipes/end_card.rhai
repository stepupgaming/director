// L3 Recipe: End card
//
// Vertical 1080×1920. Headline pops; subhead and CTA fade.
// Props: headline, subhead, cta, accent, bg

register_composition("end_card", #{
    width: 1080,
    height: 1920,
    fps: 30,
    duration: 6.0,
    default_props: #{
        headline: "That's the cut.",
        subhead: "Save this for the next edit.",
        cta: "Follow for the next one",
        accent: "#fb7185",
        bg: "#18181b"
    }
}, |props| {
    let movie = new_director(1080, 1920, 30);
    let headline = props["headline"];
    let subhead = props["subhead"];
    let cta = props["cta"];
    let accent = props["accent"];
    let bg = props["bg"];

    let scene = movie.add_scene(6.0);
    let root = scene.add_box(#{
        width: "100%",
        height: "100%",
        flex_direction: "column",
        justify_content: "center",
        align_items: "stretch",
        gap: 32.0,
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

    let head = root.add_text(#{
        content: headline,
        width: "100%",
        size: 72.0,
        color: "#fafaf9",
        weight: "bold"
    });
    head.pop_in(0.45);

    let sub = root.add_text(#{
        content: subhead,
        width: "100%",
        size: 34.0,
        color: "#d4d4d8"
    });
    sub.fade_in(0.4, 0.4);

    let action = root.add_text(#{
        content: cta,
        width: "100%",
        size: 32.0,
        color: accent,
        weight: "bold"
    });
    action.fade_in(0.4, 0.95);

    movie
});
