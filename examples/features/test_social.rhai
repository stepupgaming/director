let movie = new_director(1080, 1920, 30);
let scene = movie.add_scene(2.0);

scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0A0A12"
});

scene.add_social_handle_overlay(#{
    platforms: [
        #{ platform: "X", handle: "@directorapp", color: "#1DA1F2" },
        #{ platform: "IG", handle: "@directorapp", color: "#E4405F" }
    ],
    position: "bottom_left",
    bg_color: "#1A1A2E",
    text_color: "#FFFFFF",
    margin: 100.0
});

movie
