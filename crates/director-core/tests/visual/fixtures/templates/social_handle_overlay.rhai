// @skip
let movie = new_director(400, 300, 30);
let scene = movie.add_scene(1.0);

scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e"
});

scene.add_social_handle_overlay(#{
    position: "bottom_left",
    bg_color: "#252538",
    text_color: "#ffffff",
    platforms: [
        #{ platform: "YouTube", handle: "@mychannel", color: "#e63946" },
        #{ platform: "Twitter", handle: "@myhandle", color: "#3a86ff" }
    ]
});

movie
