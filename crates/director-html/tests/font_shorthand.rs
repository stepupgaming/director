use director_html::{compile, CompileOptions};

fn options() -> CompileOptions {
    CompileOptions {
        width: 64,
        height: 64,
        duration: 3.0,
        ..Default::default()
    }
}

fn source(style: &str) -> String {
    format!("<style>div{{{style}}}</style><div>11</div>")
}

#[test]
fn font_shorthand_compiles() {
    compile(&source("font:800 24px/1.2 Roboto"), options()).unwrap();
}

#[test]
fn normal_stretch_and_caps_compile() {
    compile(
        &source("font-stretch:normal;font-variant-caps:normal"),
        options(),
    )
    .unwrap();
}

#[test]
fn condensed_stretch_is_rejected() {
    let err = compile(&source("font-stretch:condensed"), options())
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("font-stretch other than normal unsupported"), "{err}");
}

#[test]
fn small_caps_is_rejected() {
    let err = compile(&source("font:small-caps 24px Roboto"), options())
        .err()
        .unwrap()
        .to_string();
    assert!(
        err.contains("font-variant-caps other than normal unsupported"),
        "{err}"
    );
}
