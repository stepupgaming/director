// Verification & Repair demo
//
// Builds a scene with a caption that overlaps a tracked face, builds a render
// manifest, runs verify() to detect the issue, then runs repair() to mark the
// affected node dirty. The script demonstrates the machine-actionable
// verification/repair pipeline from Rhai.

let movie = new_director(1080, 1920, 30);
let scene = movie.add_scene(2.0);

let root = scene.add_box(#{
    width: "100%",
    height: "100%",
    bg_color: "#1a1a2e",
    position: "relative"
});

// Place a face in the top-center of the frame where the default caption block
// will overlap it, guaranteeing a caption-over-face verification issue.
let face_id = movie.create_entity("face", "mara");
movie.add_entity_sample(face_id, 0.0, 400.0, 0.0, 200.0, 200.0, 1.0);
movie.add_entity_sample(face_id, 2.0, 400.0, 0.0, 200.0, 200.0, 1.0);

// Add a caption that will overlap the face.
let caption = root.add_caption(#{
    source: `
1
00:00:00,000 --> 00:00:02,000
This caption overlaps the face.`,
    font_size: 48.0,
    color: "#FFFFFF",
    background_color: "#000000AA",
    caption_position: "top",
    position: "absolute",
    top: 0,
    left: 0,
    width: "100%",
    align: "center",
    top_margin: 20.0,
    background_padding: 12.0
});

// Build a render manifest at the middle of the scene.
let manifest = build_render_manifest(movie, 1.0);

// Inspect the full manifest as JSON before verifying.
let manifest_json = manifest.to_json();
print("Manifest JSON length: " + manifest_json.len());
// Print a short preview so the script demonstrates content inspection.
let preview_len = 200.min(manifest_json.len());
print(manifest_json.sub_string(0, preview_len));

// Verify the manifest. We expect a caption-over-face issue.
let all_issues = manifest.verify();
print("Verification issues found: " + all_issues.len());
for issue in all_issues {
    print("  - " + issue.issue_type + ": " + issue.description);
}

// Select only the caption-over-face issues for repair, leaving any other
// issues (safe-zone, static-stretch, etc.) untouched.
let caption_issues = [];
for issue in all_issues {
    if issue.issue_type == "caption_over_face" {
        caption_issues.push(issue);
    }
}
print("Selected for repair: " + caption_issues.len);

// Repair only the selected issues. This will mark the affected caption
// node dirty and, for SmartCaptionNodes, re-solve trajectories against
// current tracks.
let repair_result = manifest.repair(caption_issues);
print("Repair affected nodes: " + repair_result.affected_nodes.len);
print("Repair queued issues: " + repair_result.queued_issue_count);

// Also demonstrate the config-aware overload by re-running the same
// selective repair with a custom verification config.
let custom_config = #{
    caption_face_overlap_threshold: 0.5
};
let repair_result_config = manifest.repair(caption_issues, custom_config);
print("Config-aware repair affected nodes: " + repair_result_config.affected_nodes.len);
print("Config-aware repair queued issues: " + repair_result_config.queued_issue_count);

movie
