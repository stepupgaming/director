// FX overlay track for the Director reel, rendered on black and screen-blended in reel.html.
// Scene lengths follow the beat grid of music.wav, so this clip shares the reel clock.
let movie = new_director(1920, 1080, 30);
let music = movie.add_audio("examples/html/reel/assets/music.wav");
let track = music.id();

let s0 = movie.add_scene(3.808);
s0.add_box(#{ width: 1920.0, height: 1080.0, bg_color: "#000000" });

// Eye shot: viewfinder locks on (Lottie trim paths).
let s1 = movie.add_scene(1.904);
let b1 = s1.add_box(#{ width: 1920.0, height: 1080.0, bg_color: "#000000" });
let finder = b1.add_lottie("examples/html/reel/assets/finder.json", #{ width: 900.0, height: 900.0, speed: 1.0, "loop": false, position: "absolute", left: 450.0, top: 60.0 });

let s2 = movie.add_scene(15.209);
s2.add_box(#{ width: 1920.0, height: 1080.0, bg_color: "#000000" });

// Monolith and end card: an equalizer line and a glow driven by the music's bass, mids and highs.
let s3 = movie.add_scene(7.679);
let b3 = s3.add_box(#{ width: 1920.0, height: 1080.0, bg_color: "#000000" });
let glow = b3.add_box(#{ width: 360.0, height: 360.0, position: "absolute", left: 340.0, top: 240.0, border_radius: 180.0, bg_color: "#ff6a1f" });
glow.set_pivot(0.5, 0.5);
glow.set_blur(90.0);
glow.bind_audio(track, "bass", "scale");
let row = b3.add_box(#{ width: 1920.0, height: 200.0, position: "absolute", left: 0.0, top: 720.0, flex_direction: "row", justify_content: "center", align_items: "flex_end" });
let bands = ["bass", "mids", "highs"];
for i in 0..64 {
    let d = (i - 31.5).abs() / 32.0;
    let h = 6.0 + 34.0 * (1.0 - d * d);
    let bar = row.add_box(#{ width: 4.0, height: h, margin: 5.0, border_radius: 2.0, bg_color: if d < 0.35 { "#ffb070" } else { "#ff7a33" } });
    bar.set_pivot(0.5, 1.0);
    bar.bind_audio(track, bands[(i * 7) % 3], "scale_y");
}
movie
