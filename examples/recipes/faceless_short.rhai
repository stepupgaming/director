// L3 Recipe: Faceless Short
//
// Vertical kinetic explainer (1080×1920). No face, no VO dependency —
// typography + color + L1 motion carry the arc.
//
// Props (shallow merge over defaults):
//   title, subtitle, beats (array of strings), accent, bg, end_line
//
// CLI:
//   director-engine compositions examples/recipes/faceless_short.rhai --json
//   director-engine preview examples/recipes/faceless_short.rhai
//   director-engine render  examples/recipes/faceless_short.rhai --composition faceless_short -o out.mp4
//   director-engine still   examples/recipes/faceless_short.rhai --composition faceless_short --props '{"title":"X"}' -t 1 -o frame.png

register_composition("faceless_short", #{
    width: 1080,
    height: 1920,
    fps: 30,
    duration: 14.0,
    default_props: #{
        title: "Three things that compound",
        subtitle: "A 15-second faceless short",
        accent: "#38bdf8",
        bg: "#050816",
        end_line: "Ship the next one.",
        beats: [
            "Clarity beats volume",
            "Systems beat motivation",
            "Reps beat talent"
        ]
    }
}, |props| {
    let movie = new_director(1080, 1920, 30);
    let accent = props["accent"];
    let bg = props["bg"];
    let title = props["title"];
    let subtitle = props["subtitle"];
    let end_line = props["end_line"];
    let beats = props["beats"];

    // ----- Cold open -----
    let s0 = movie.add_scene(3.2);
    let open = s0.add_box(#{
        width: "100%",
        height: "100%",
        flex_direction: "column",
        justify_content: "center",
        align_items: "center",
        gap: 28.0,
        padding: 64.0,
        bg_color: bg
    });

    let kicker = open.add_text(#{
        content: "FACELLESS",
        size: 28.0,
        color: accent,
        weight: "bold"
    });
    kicker.fade_in(0.35);

    let headline = open.add_text(#{
        content: title,
        size: 64.0,
        color: "#f8fafc",
        weight: "bold"
    });
    headline.fly_in_up(0.55, 0.15);

    let sub = open.add_text(#{
        content: subtitle,
        size: 30.0,
        color: "#94a3b8"
    });
    sub.fade_in(0.4, 0.45);

    // ----- Beat cards -----
    let beat_count = beats.len();
    let i = 0;
    while i < beat_count {
        let beat_text = beats[i];
        let n = i + 1;
        let label = "#" + n.to_string() + "  of  " + beat_count.to_string();

        let scene = movie.add_scene(2.8);
        let root = scene.add_box(#{
            width: "100%",
            height: "100%",
            flex_direction: "column",
            justify_content: "center",
            align_items: "stretch",
            gap: 32.0,
            padding: 72.0,
            bg_color: bg
        });

        let idx = root.add_text(#{
            content: label,
            size: 26.0,
            color: accent,
            weight: "bold"
        });
        idx.fade_in(0.25);

        let card = root.add_box(#{
            width: "100%",
            padding: 40.0,
            border_radius: 28.0,
            bg_color: "#0f172a",
            justify_content: "center",
            align_items: "flex_start"
        });
        card.fly_in_left(0.45, 0.08, 90.0);

        let body = card.add_text(#{
            content: beat_text,
            size: 48.0,
            color: "#f1f5f9",
            weight: "bold"
        });
        body.pop_in(0.4, 0.2);

        i += 1;
    }

    // ----- End card -----
    let s_end = movie.add_scene(2.6);
    let end_root = s_end.add_box(#{
        width: "100%",
        height: "100%",
        flex_direction: "column",
        justify_content: "center",
        align_items: "center",
        gap: 24.0,
        padding: 64.0,
        bg_color: bg
    });

    let bar = end_root.add_box(#{
        width: 120.0,
        height: 6.0,
        border_radius: 4.0,
        bg_color: accent
    });
    bar.fade_in(0.3);

    let closer = end_root.add_text(#{
        content: end_line,
        size: 52.0,
        color: "#ffffff",
        weight: "bold"
    });
    closer.pop_in(0.5, 0.1);
    closer.pulse(0.45, 1.1, 1.06);

    movie
});
