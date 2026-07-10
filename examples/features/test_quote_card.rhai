let movie = new_director(1080, 1920, 30.0);
let scene = movie.add_scene(2.0);

scene.add_quote_card(#{
    quote: "The best way to predict the future is to create it.",
    author: "Peter Drucker",
    accent_color: "#FF3366",
    text_color: "#FFFFFF",
    label_color: "#A0A0B0",
    bg_color: "#1A1A2E",
    max_width: 800.0
});

movie
