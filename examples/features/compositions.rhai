// Named compositions with default props.
//
// Register two compositions; hosts list them via `director-engine compositions`
// and render with `--composition <name> --props '{...}'`.
// This script returns unit (not a MovieHandle) after registration.

register_composition("intro", #{
    width: 1280,
    height: 720,
    fps: 30,
    duration: 3.0,
    default_props: #{
        title: "Hello Director",
        subtitle: "Named compositions",
        bg: "#0f172a",
        accent: "#38bdf8"
    }
}, |props| {
    let movie = new_director(1280, 720, 30);
    let scene = movie.add_scene(3.0);

    let root = scene.add_box(#{
        width: "100%",
        height: "100%",
        justify_content: "center",
        align_items: "center",
        flex_direction: "column",
        gap: 16.0,
        bg_color: props["bg"]
    });

    root.add_text(#{
        content: props["title"],
        size: 64.0,
        color: "#ffffff",
        weight: "bold"
    });

    root.add_text(#{
        content: props["subtitle"],
        size: 28.0,
        color: props["accent"]
    });

    movie
});

register_composition("card", #{
    width: 1080,
    height: 1080,
    fps: 30,
    duration: 2.5,
    default_props: #{
        label: "Square card",
        value: "42"
    }
}, |props| {
    let movie = new_director(1080, 1080, 30);
    let scene = movie.add_scene(2.5);

    let root = scene.add_box(#{
        width: "100%",
        height: "100%",
        justify_content: "center",
        align_items: "center",
        flex_direction: "column",
        gap: 24.0,
        bg_color: "#111827"
    });

    root.add_text(#{
        content: props["label"],
        size: 36.0,
        color: "#9ca3af"
    });

    root.add_text(#{
        content: props["value"],
        size: 120.0,
        color: "#f9fafb",
        weight: "bold"
    });

    movie
});
