//! Integration test for the `director-cli inspect-deps` subcommand.

use std::io::Write;
use std::process::Command;

#[test]
fn inspect_deps_emits_dependency_graph_json() {
    let script = r#"
        let movie = new_director(640, 360, 30);
        let scene = movie.add_scene(2.5);
        let root = scene.add_box(#{ width: "100%", height: "100%" });
        let inner = root.add_box(#{ width: 100.0, height: 100.0 });
        movie
    "#;

    let dir = std::env::temp_dir();
    let unique = format!(
        "director_cli_inspect_deps_test_{}_{}.rhai",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    );
    let path = dir.join(unique);
    {
        let mut file = std::fs::File::create(&path).expect("create temp script");
        file.write_all(script.as_bytes()).expect("write temp script");
    }

    let output = Command::new(env!("CARGO_BIN_EXE_director-cli"))
        .arg("inspect-deps")
        .arg(&path)
        .output()
        .expect("failed to run director-cli inspect-deps");

    assert!(
        output.status.success(),
        "director-cli inspect-deps failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json = String::from_utf8(output.stdout).expect("stdout is valid UTF-8");
    let _ = std::fs::remove_file(&path);

    let parsed: serde_json::Value = serde_json::from_str(&json).expect("stdout is valid JSON");
    assert!(parsed.get("edges").is_some(), "dependency graph JSON should contain edges");
    assert!(parsed.get("node_inputs").is_some(), "dependency graph JSON should contain node_inputs");
}
