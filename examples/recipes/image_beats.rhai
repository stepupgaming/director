// L3 Recipe: Image beats (media props + Ken Burns)
//
// Full-bleed image cards with captions. Paths come from props so hosts can
// swap media without editing the recipe graph.
//
// Default media is repo-relative (run from workspace root / examples tests cwd):
//   examples/tests/heart_eyes_official_frame1.jpg
//   examples/assets/star.svg
//
// CLI:
//   director-engine compositions examples/recipes/image_beats.rhai --json
//   director-engine preview examples/recipes/image_beats.rhai
//   director-engine render examples/recipes/image_beats.rhai --composition image_beats \
//     --props '{"images":["path/a.jpg","path/b.jpg"],"captions":["One","Two"]}' -o out.mp4

register_composition("image_beats", #{
    width: 1080,
    height: 1920,
    fps: 30,
    duration: 9.0,
    default_props: #{
        title: "Still frames that move",
        accent: "#fbbf24",
        bg: "#020617",
        hold: 2.8,
        images: [
            "examples/tests/heart_eyes_official_frame1.jpg",
            "examples/assets/star.svg"
        ],
        captions: [
            "Lead with a still",
            "Let motion do the rest"
        ]
    }
}, |props| {
    let movie = new_director(1080, 1920, 30);
    let accent = props["accent"];
    let bg = props["bg"];
    let title = props["title"];
    let images = props["images"];
    let captions = props["captions"];
    let hold = props["hold"];

    // Title card
    let s0 = movie.add_scene(2.4);
    let open = s0.add_box(#{
        width: "100%",
        height: "100%",
        flex_direction: "column",
        justify_content: "center",
        align_items: "center",
        gap: 20.0,
        padding: 48.0,
        bg_color: bg
    });
    let k = open.add_text(#{
        content: "IMAGE BEATS",
        size: 26.0,
        color: accent,
        weight: "bold"
    });
    k.fade_in(0.3);
    let h = open.add_text(#{
        content: title,
        size: 52.0,
        color: "#f8fafc",
        weight: "bold"
    });
    h.fly_in_up(0.5, 0.12);

    let n = images.len();
    let i = 0;
    while i < n {
        let path = images[i];
        let caption = if i < captions.len() {
            captions[i]
        } else {
            ""
        };

        let scene = movie.add_scene(hold);
        let root = scene.add_box(#{
            width: "100%",
            height: "100%",
            bg_color: bg,
            justify_content: "center",
            align_items: "center"
        });

        // Full-bleed media plate
        let plate = root.add_box(#{
            width: "100%",
            height: "100%",
            position: "absolute",
            left: 0.0,
            top: 0.0,
            justify_content: "center",
            align_items: "center",
            bg_color: "#0f172a"
        });

        let img = plate.add_image(path, #{
            width: "100%",
            height: "100%",
            object_fit: "cover"
        });
        // Alternate Ken Burns direction per beat
        if i % 2 == 0 {
            img.ken_burns(hold, 1.12, 22.0, -14.0);
        } else {
            img.ken_burns(hold, 1.1, -18.0, 12.0);
        }
        img.fade_in(0.35);

        if caption != "" {
            let bar = root.add_box(#{
                position: "absolute",
                left: 40.0,
                right: 40.0,
                bottom: 120.0,
                padding: 28.0,
                border_radius: 20.0,
                bg_color: "#020617cc",
                justify_content: "center",
                align_items: "flex_start"
            });
            bar.fly_in_up(0.4, 0.15, 40.0);
            let t = bar.add_text(#{
                content: caption,
                size: 36.0,
                color: "#f8fafc",
                weight: "bold"
            });
            t.typewriter(0.7, 0.25);
        }

        i += 1;
    }

    // End
    let s_end = movie.add_scene(2.0);
    let end = s_end.add_box(#{
        width: "100%",
        height: "100%",
        flex_direction: "column",
        justify_content: "center",
        align_items: "center",
        gap: 16.0,
        bg_color: bg
    });
    let done = end.add_text(#{
        content: "Cut. Caption. Ship.",
        size: 44.0,
        color: accent,
        weight: "bold"
    });
    done.pop_in(0.45);

    movie
});
