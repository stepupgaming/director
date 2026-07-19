// Style Policy Check demo
//
// Configures a strict production StylePolicy, builds a scene that violates
// it, calls compile(), and inspects the emitted violations.

let movie = new_director(1080, 1920, 30);
let scene = movie.add_scene(2.0);
let root = scene.add_box(#{ width: "100%", height: "100%" });

// Create a text node with a font size that exceeds the strict policy limit.
let text = root.add_text(#{
    content: "This text is way too large for the strict policy",
    size: 250.0,
    color: "#FFFFFF"
});

// Configure a strict StylePolicy.
let policy = style_policy(#{
    typography: #{
        max_font_size: 120.0,
        max_text_area_ratio: 0.2,
        safe_zone_margin: 32.0,
        weight: 1.0,
    },
    motion: #{
        max_pixels_per_second: 500.0,
        max_rotation_velocity: 180.0,
        max_scale_velocity: 2.0,
        avg_motion_displacement_threshold: 0.0,
        weight: 1.0,
    },
    transitions: #{
        min_seconds_between_transitions: 2.0,
        max_transitions_per_minute: 6.0,
        min_transition_duration: 0.2,
        max_transition_duration: 1.0,
        weight: 1.0,
    },
    audio: #{
        target_loudness_db: -14.0,
        loudness_tolerance_db: 1.0,
        max_true_peak_db: -1.0,
        min_loudness_range: 4.0,
        weight: 1.0,
    },
});
movie.set_style_policy(policy);

// Compile the scene. This scores the scene against the policy and stores
// the result for later inspection.
movie.compile();

// Inspect the compile-time score and violations.
let score = movie.style_policy_score();
print("Style policy total score: " + score.total_score);
print("Dimension scores:");
for ds in score.dimension_scores {
    print("  - " + ds.name + ": " + ds.score);
}

let violations = movie.style_policy_violations();
print("Violations found: " + violations.len);
for violation in violations {
    print("  - [" + violation.severity + "] " + violation.rule + ": " + violation.description);
}

if violations.len == 0 {
    throw "expected at least one style policy violation (font size too large)";
}

movie
