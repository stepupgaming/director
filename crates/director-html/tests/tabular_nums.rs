use director_html::{compile, CompileOptions};

fn options() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        ..Default::default()
    }
}

fn source(value: &str) -> String {
    format!("<style>div{{font-variant-numeric:{value}}}</style><div>11</div>")
}

#[test]
fn tabular_nums_compiles() {
    compile(&source("tabular-nums"), options()).unwrap();
}

#[test]
fn normal_adds_no_feature_and_compiles() {
    compile(&source("normal"), options()).unwrap();
}

#[test]
fn diagonal_fractions_is_rejected() {
    let err = compile(&source("diagonal-fractions"), options())
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("unsupported font-variant-numeric"), "{err}");
}
