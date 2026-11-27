// Caption system demo
// Renders a 4-second scene with SRT captions appearing at specific times

let movie = new_director(1080, 1920, 30); // portrait (Shorts/Reels aspect)
let scene = movie.add_scene(4.0);

// Dark background with relative positioning for absolute children
let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e",
    position: "relative"
});

// Some content text
root.add_text(#{
    content: "Director Caption Demo",
    size: 36.0,
    color: "#e0e0e0",
    position: "absolute",
    top: 100, left: 0, width: "100%",
    align: "center"
});

// Add captions via SRT source
let caps = root.add_caption(#{
    source: `
1
00:00:00,500 --> 00:00:01,500
Welcome to Director!

2
00:00:01,500 --> 00:00:02,500
This is a caption demo.

3
00:00:02,500 --> 00:00:04,000
SRT subtitles with style!`,
    font_size: 48.0,
    color: "#FFFFFF",
    pill_enabled: true,
    pill_color: "#000000AA",
    caption_position: "bottom",
    bottom_margin: 80.0,
    pill_padding_h: 12.0,
    pill_padding_v: 6.0
});

movie
