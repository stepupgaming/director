//! Compiles the C++ shim against the Filament SDK named by FILAMENT_DIR
//! (default F:/director-spikes/filament) and links its /MD release libraries.

fn main() {
    let sdk = std::env::var("FILAMENT_DIR").unwrap_or_else(|_| "F:/director-spikes/filament".into());
    println!("cargo:rerun-if-changed=shim.cpp");
    println!("cargo:rerun-if-env-changed=FILAMENT_DIR");
    cc::Build::new()
        .cpp(true)
        .file("shim.cpp")
        .include("stub")
        .include(format!("{sdk}/include"))
        .flag("/std:c++20")
        .flag("/EHsc")
        .flag("/permissive-")
        .flag("/Zc:__cplusplus")
        .define("NOMINMAX", None)
        .define("WIN32_LEAN_AND_MEAN", None)
        .warnings(false)
        .compile("filament_shim");
    println!("cargo:rustc-link-search=native={sdk}/lib/x86_64/md");
    for lib in [
        "gltfio_core", "uberarchive", "uberzlib", "ktxreader", "image", "basis_transcoder",
        "dracodec", "meshoptimizer", "mikktspace", "stb", "camutils", "geometry",
        "filament", "backend", "bluevk", "bluegl", "filabridge", "filaflat", "ibl-lite",
        "shaders", "smol-v", "zstd", "perfetto", "abseil", "utils",
    ] {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    for lib in ["gdi32", "user32", "opengl32", "shlwapi", "advapi32", "ole32", "shell32"] {
        println!("cargo:rustc-link-lib={lib}");
    }
}
