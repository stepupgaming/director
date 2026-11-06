//! One integration binary for `director-track`.

mod klt;
mod probe;
mod solve;

#[test]
fn every_test_file_is_a_module() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let source = std::fs::read_to_string(dir.join("main.rs")).unwrap();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "rs") && path.file_name().unwrap() != "main.rs" {
            let name = path.file_stem().unwrap().to_str().unwrap();
            assert!(source.contains(&format!("mod {name};")), "tests/{name}.rs is not declared in tests/main.rs");
        }
    }
}
