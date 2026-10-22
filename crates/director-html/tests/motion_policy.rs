use director_html::{compile, CompileOptions};

fn motion_rules(animation: &str) -> Vec<String> {
    let source = format!(
        "<style>body{{margin:0}}div{{width:20px;height:20px;background:red;{animation}}}@keyframes slide{{from{{transform:translateX(0)}}to{{transform:translateX(300px)}}}}@keyframes dash{{from{{transform:translateX(0)}}to{{transform:translateX(3000px)}}}}</style><div></div>"
    );
    let c = compile(
        &source,
        CompileOptions {
            width: 64,
            height: 64,
            duration: 3.0,
            ..Default::default()
        },
    )
    .unwrap();
    c.director
        .last_policy_score
        .unwrap()
        .violations
        .into_iter()
        .map(|v| v.rule)
        .filter(|rule| rule.starts_with("motion."))
        .collect()
}

#[test]
fn static_html_reports_low_motion() {
    let rules = motion_rules("");
    assert!(rules.contains(&"motion.avg_motion_displacement_threshold".into()), "{rules:?}");
}

#[test]
fn css_transform_animation_counts_as_motion() {
    let rules = motion_rules("animation:slide 3s linear both");
    assert!(rules.is_empty(), "{rules:?}");
}

#[test]
fn fast_css_transform_animation_breaks_the_speed_limit() {
    let rules = motion_rules("animation:dash 1s linear both");
    assert!(rules.contains(&"motion.max_pixels_per_second".into()), "{rules:?}");
}
