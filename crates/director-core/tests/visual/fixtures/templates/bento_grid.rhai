// @snapshot: 0.0
let movie = new_director(640, 360, 30);
let scene = movie.add_scene(1.0);

scene.add_bento_grid(#{
    columns: 2,
    rows: 2,
    gap: 12.0,
    padding: 24.0,
    items: [
        #{ text: "Video", bg_color: "#334466", text_color: "#ffffff" },
        #{ text: "Audio", bg_color: "#443366", text_color: "#ffffff" },
        #{ text: "Layout", bg_color: "#336644", text_color: "#ffffff" },
        #{ text: "Export", bg_color: "#664433", text_color: "#ffffff" }
    ]
});

movie
