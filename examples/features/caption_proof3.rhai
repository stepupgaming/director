// Caption proof: white background, large red text on yellow pill
let movie = new_director(1080, 1920, 30.0);
let scene = movie.add_scene(4.0);

let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#FFFFFF",
    position: "relative"
});

root.add_text(#{
    content: "CAPTION TEST",
    size: 72.0,
    color: "#000000",
    weight: "bold",
    position: "absolute",
    top: 100, left: 0, width: "100%",
    align: "center"
});

let caps = root.add_caption(#{
    source: `
1
00:00:00,500 --> 00:00:04,000
THIS IS A CAPTION TEST`,
    font_size: 64.0,
    color: "#FF0000",
    background_color: "#FFDD00FF",
    caption_position: "bottom",
    position: "absolute",
    bottom: 200,
    left: 0,
    width: "100%",
    align: "center",
    background_padding: 20.0
});

movie
