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

let d = root.add_text(#{ content: "typewriter", size: 30.0, color: "#f472b6" });
d.typewriter(0.9, 0.3);

let e = root.add_box(#{ width: 220.0, height: 80.0, bg_color: "#1e293b", border_radius: 12.0 });
e.ken_burns(3.5, 1.1, 16.0, -10.0);
e.pulse_on_beat(100.0, 2.0, 1.05);

movie
