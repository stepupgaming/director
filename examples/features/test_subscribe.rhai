let movie = new_director(1080, 1920, 30);
let scene = movie.add_scene(2.0);

scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0A0A12"
});

scene.add_subscribe_button(#{
    channel_name: "Director Tutorials",
    sub_count: "125K",
    button_text: "SUBSCRIBE",
    position: "bottom_right",
    bg_color: "#1A1A2E",
    button_color: "#FF3366",
    text_color: "#FFFFFF"
});

movie
