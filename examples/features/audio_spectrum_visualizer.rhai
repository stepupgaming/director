// Audio-Reactive Spectrum Visualizer
// Uses the FFT analyzer to drive bar heights in real time from audio.

let movie = new_director(1920, 1080, 30);
let scene = movie.add_scene(5.0);

// Add audio track (use a WAV file from the smoke test assets)
let music = movie.add_audio(".director-smoke/gemmy_music.wav");
let track_id = music.id();

// Dark background
scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#050510",
    justify_content: "center",
    align_items: "center"
});

// Title
scene.add_text(#{
    content: "SPECTRUM VISUALIZER",
    size: 48.0,
    color: "#ffffff",
    weight: 800,
    position: "absolute",
    top: 80.0,
    width: "100%",
    justify_content: "center"
});

// Audio-reactive spectrum bars — bar heights are driven by FFT data
scene.add_waveform_visualizer(#{
    track_id: track_id,
    bars: 48,
    bar_color: "#00ffaa",
    gap: 3.0,
    max_height: 70.0,
    min_height: 2.0,
    position: "center",
    smoothing: 0.55,
    border_radius: 4.0
});

movie
