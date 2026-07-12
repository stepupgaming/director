// HTTP Fetching Showcase
//
// Demonstrates Director's ability to pull live data into videos:
// - fetch_text(url): download raw text
// - fetch_json(url): download JSON and convert to Rhai map

let movie = new_director(1920, 1080, 30);

// ============================================================
// SCENE 1: Title + fetch_json demo (4.0s)
// ============================================================
let s1 = movie.add_scene(4.0);

s1.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0a0a1a",
    justify_content: "center",
    align_items: "center",
    flex_direction: "column"
});

s1.add_text(#{
    content: "HTTP FETCHING",
    size: 80.0,
    color: "#ffffff",
    weight: 800,
    margin_bottom: 20.0
});

// Fetch a JSON placeholder post (very reliable demo API)
let post = fetch_json("https://jsonplaceholder.typicode.com/posts/1");

s1.add_text(#{
    content: post.title,
    size: 28.0,
    color: "#a0a0ff",
    max_width: 1200.0,
    align: "center",
    line_height: 1.5
});

s1.add_text(#{
    content: "User ID: " + post.userId,
    size: 20.0,
    color: "#666688",
    margin_top: 10.0
});

// ============================================================
// SCENE 2: fetch_json demo — live exchange rates (4.0s)
// ============================================================
let s2 = movie.add_scene(4.0);

s2.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#0f0f23",
    justify_content: "center",
    align_items: "center",
    flex_direction: "column"
});

s2.add_text(#{
    content: "LIVE DATA",
    size: 64.0,
    color: "#ffffff",
    weight: 800,
    margin_bottom: 30.0
});

// Fetch current USD exchange rates (reliable, no auth needed)
let rates = fetch_json("https://api.exchangerate-api.com/v4/latest/USD");

let eur_rate = rates.rates.EUR;
let gbp_rate = rates.rates.GBP;
let jpy_rate = rates.rates.JPY;

s2.add_text(#{
    content: "1 USD =",
    size: 36.0,
    color: "#8888aa",
    margin_bottom: 10.0
});

s2.add_text(#{
    content: eur_rate + " EUR",
    size: 48.0,
    color: "#00ff88",
    margin_bottom: 8.0
});

s2.add_text(#{
    content: gbp_rate + " GBP",
    size: 48.0,
    color: "#4ecdc4",
    margin_bottom: 8.0
});

s2.add_text(#{
    content: jpy_rate + " JPY",
    size: 48.0,
    color: "#ffe66d"
});

// ============================================================
// SCENE 3: fetch_json — parsed nested structure demo (4.0s)
// ============================================================
let s3 = movie.add_scene(4.0);

s3.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e",
    justify_content: "center",
    align_items: "center",
    flex_direction: "column"
});

s3.add_text(#{
    content: "NESTED JSON",
    size: 64.0,
    color: "#ffffff",
    weight: 800,
    margin_bottom: 20.0
});

// Fetch a user profile with nested address object
let user = fetch_json("https://jsonplaceholder.typicode.com/users/1");

let name = user.name;
let email = user.email;
let city = user.address.city;

s3.add_text(#{
    content: name,
    size: 40.0,
    color: "#cccccc",
    margin_bottom: 10.0
});

s3.add_text(#{
    content: email,
    size: 24.0,
    color: "#8888aa",
    margin_bottom: 10.0
});

s3.add_text(#{
    content: city,
    size: 32.0,
    color: "#ff6b6b"
});

// ============================================================
// Transitions
// ============================================================
movie.add_transition(s1, s2, "fade", 1.0, "ease_in_out");
movie.add_transition(s2, s3, "slide_left", 1.0, "ease_in_out");

movie
