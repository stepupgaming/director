// L1 motion stdlib smoke script (returns Movie for legacy path).

let movie = new_director(1280, 720, 30);
let scene = movie.add_scene(4.0);

let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    flex_direction: "column",
    justify_content: "center",
    align_items: "center",
    gap: 24.0,
    bg_color: "#0b1020"
});

let a = root.add_text(#{ content: "fade + pop", size: 48.0, color: "#e2e8f0", weight: "bold" });
a.fade_in(0.4);
a.pop_in(0.5);

let b = root.add_text(#{ content: "fly in up", size: 36.0, color: "#38bdf8" });
b.fly_in_up(0.55, 0.2);

let c = root.add_text(#{ content: "pulse", size: 32.0, color: "#a78bfa" });
c.fade_in(0.3, 0.5);
c.pulse(0.4, 0.9);

movie
