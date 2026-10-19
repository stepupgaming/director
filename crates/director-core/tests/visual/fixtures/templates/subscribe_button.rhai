// @skip
let movie = new_director(640, 120, 30);
let scene = movie.add_scene(1.0);

scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0d0d14"
});

scene.add_subscribe_button(#{
    channel_name: "My Channel",
    sub_count: "1.2M",
    button_text: "SUBSCRIBE",
    position: "bottom_center",
    bg_color: "#1e1e2e",
    button_color: "#e63c3c",
    text_color: "#ffffff",
    width: 520.0,
    height: 64.0,
    border_radius: 32.0
});

movie
