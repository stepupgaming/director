let movie = new_director(1080, 1920, 30);
let scene = movie.add_scene(2.0);

scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0A0A18"
});

scene.add_title_card(#{
    title: "Director Templates",
    subtitle: "Higher-level layout primitives",
    bg_color: "#0A0A18",
    title_color: "#FFFFFF",
    subtitle_color: "#A0A0B0",
    title_size: 96.0,
    subtitle_size: 48.0
});

movie
