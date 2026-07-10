let movie = new_director(1080, 1920, 30.0);
let scene = movie.add_scene(2.0);

scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0A0A12"
});

scene.add_bento_grid(#{
    columns: 2,
    rows: 3,
    gap: 16.0,
    padding: 24.0,
    items: [
        #{ text: "Feature One", text_color: "#FFFFFF", bg_color: "#FF6B6B", border_radius: 16.0 },
        #{ text: "Feature Two", text_color: "#FFFFFF", bg_color: "#4ECDC4", border_radius: 16.0 },
        #{ text: "Feature Three", text_color: "#FFFFFF", bg_color: "#FFE66D", border_radius: 16.0 },
        #{ text: "Feature Four", text_color: "#FFFFFF", bg_color: "#A29BFE", border_radius: 16.0 },
        #{ text: "Feature Five", text_color: "#FFFFFF", bg_color: "#FF8B5E", border_radius: 16.0 },
        #{ text: "Feature Six", text_color: "#FFFFFF", bg_color: "#5E8BFF", border_radius: 16.0 }
    ]
});

movie
