// Caption system proof - bright background, visible text + pill styling
let movie = new_director(1080, 1920, 30);
let scene = movie.add_scene(4.0);

// Gradient background
let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_gradient: ["#667eea", "#764ba2"],
    flex_direction: "column",
    justify_content: "center",
    align_items: "center"
});

// Title
root.add_text(#{
    content: "Caption System Demo",
    size: 48.0,
    color: "#ffffff",
    weight: "bold",
    align: "center"
});

// Subtitle
root.add_text(#{
    content: "SRT parsing + background pill styling",
    size: 28.0,
    color: "#ddddff",
    align: "center"
});

// Captions at bottom with pill background
let caps = root.add_caption(#{
    source: `
1
00:00:00,500 --> 00:00:02,500
Welcome to Director!

2
00:00:02,500 --> 00:00:04,000
Captions with background styling`,
    font_size: 56.0,
    color: "#FFFFFF",
    background_color: "#1a1a2eCC",
    caption_position: "bottom",
    position: "absolute",
    bottom: 0,
    left: 0,
    width: "100%",
    align: "center",
    bottom_margin: 60.0,
    background_padding: 16.0
});

movie
