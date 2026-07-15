// Smart Caption demo
// Renders a 4-second scene where a caption automatically avoids a tracked face.
// The face moves from the bottom-center to the top-center of the frame, so the
// solver switches the caption anchor mid-scene (top -> bottom).

let movie = new_director(1080, 1920, 30); // portrait (Shorts/Reels aspect)
let scene = movie.add_scene(4.0);

// Dark background with relative positioning for absolute children
let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e",
    position: "relative"
});

// Create a face entity that moves from the bottom-center to the top-center
// of the frame over the course of the shot.
let face_id = movie.create_entity("face", "mara");
movie.add_entity_sample(face_id, 0.0, 400.0, 1500.0, 200.0, 200.0, 1.0);
movie.add_entity_sample(face_id, 2.0, 400.0, 960.0, 200.0, 200.0, 1.0);
movie.add_entity_sample(face_id, 4.0, 400.0, 60.0, 200.0, 200.0, 1.0);

// Add a smart caption that avoids the tracked face.
// The solver chooses the actual anchor (top, bottom, left, or right)
// based on where the negative space is, so caption_position is ignored.
let smart_caps = root.add_smart_caption(#{
    source: `
1
00:00:00,500 --> 00:00:01,500
Welcome to Director!

2
00:00:01,500 --> 00:00:02,500
Smart captions avoid faces.

3
00:00:02,500 --> 00:00:04,000
The solver picks negative space.`,
    entity_refs: [face_id],
    font_size: 48.0,
    color: "#FFFFFF",
    pill_enabled: true,
    pill_color: "#000000AA",
    pill_padding_h: 12.0,
    pill_padding_v: 6.0,
    position: "absolute",
    bottom: 0,
    left: 0,
    width: "100%",
    align: "center",
    bottom_margin: 80.0,
    sample_rate: 10.0,
    // Low motion penalty so the caption freely switches between top and bottom
    // anchors as the face moves across the shot.
    motion_penalty: 0.0
});

movie
