//! Default output naming for CLI renders. Script evaluation lives in
//! `director_host::script`.

pub use director_host::script::eval_script;
use std::path::Path;

/// Default video output path: next to script, extension from alpha flag.
/// When a composition name is set, insert `_<name>` before the extension.
pub fn default_video_output(
    script: &Path,
    composition: Option<&str>,
    alpha: bool,
) -> std::path::PathBuf {
    let ext = if alpha { "webm" } else { "mp4" };
    let mut p = script.to_path_buf();
    if let Some(name) = composition {
        let stem = p
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "out".into());
        p.set_file_name(format!("{}_{}.{}", stem, sanitize_name(name), ext));
    } else {
        p.set_extension(ext);
    }
    p
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn default_output_suffixes_composition() {
        let p = default_video_output(Path::new("foo/bar.rhai"), Some("intro"), false);
        assert_eq!(p, PathBuf::from("foo/bar_intro.mp4"));
    }

    #[test]
    fn default_output_legacy() {
        let p = default_video_output(Path::new("foo/bar.rhai"), None, true);
        assert_eq!(p, PathBuf::from("foo/bar.webm"));
    }
}
