// L3 Recipe: Countdown
//
// Vertical 1080×1920, under 8s. L1 has no numeric tween, so each beat
// pops the next integer down from `start` to 0.
// Props: start (number), label, accent, bg

register_composition("countdown", #{
    width: 1080,
    height: 1920,
    fps: 30,
    duration: 6.8,
    default_props: #{
        start: 4,
        label: "to go",
        accent: "#fb923c",
        bg: "#111827"
    }
}, |props| {
    let movie = new_director(1080, 1920, 30);
    let start = props["start"];
    let label = props["label"];
    let accent = props["accent"];
    let bg = props["bg"];

    let steps = 5;
    let i = 0;
    while i < steps {
        let shown = start * (steps - 1 - i) / (steps - 1);
        let dur = if i == steps - 1 { 2.2 } else { 1.15 };
        let last = i == steps - 1;

        let scene = movie.add_scene(dur);
        let root = scene.add_box(#{
            width: "100%",
            height: "100%",
            flex_direction: "column",
            justify_content: "center",
            align_items: "center",
            gap: 28.0,
            padding: 72.0,
            bg_color: bg
        });

        let bar = root.add_box(#{
            width: 96.0,
            height: 8.0,
            border_radius: 4.0,
            bg_color: accent
        });
        bar.fade_in(0.25);

        let num = root.add_text(#{
            content: shown.to_string(),
            size: 168.0,
            color: "#f8fafc",
            weight: "bold"
        });
        if last {
            num.pop_in(0.45);
        } else {
            num.fly_in_up(0.35, 0.05, 48.0);
        }

        let cap = root.add_text(#{
            content: label,
            size: 36.0,
            color: accent,
            weight: "bold"
        });
        cap.fade_in(0.3, 0.12);

        i += 1;
    }

    movie
});
