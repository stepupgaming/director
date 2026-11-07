use std::fs;

use director_asr::store::sha256_file;
use director_asr::{AsrError, Model, Status, Store};

#[test]
fn model_ids_round_trip_and_unknown_names_fail() {
    for m in Model::ALL {
        assert_eq!(m.id().parse::<Model>().unwrap(), m);
        assert_eq!(m.to_string(), m.id());
        assert_eq!(serde_json::to_string(&m).unwrap(), format!("\"{}\"", m.id()));
        assert_eq!(m.license(), "CC-BY-NC-4.0");
        assert_eq!(m.file().sha256.len(), 64);
    }
    let err = "bark-9".parse::<Model>().unwrap_err();
    assert!(matches!(err, AsrError::UnknownModel(_)));
    assert!(err.to_string().contains("bark-3.0-pro"));
}

#[test]
fn urls_pin_the_revision() {
    assert_eq!(
        Model::Bark.url(),
        "https://huggingface.co/ConwayResearch/Underdog-Bark-3.0/resolve/e7a641c0fdd4cdf1818dde62a98c26e11d65c061/model.bark"
    );
    assert!(Model::BarkPro.url().ends_with("/model.safetensors"));
}

#[test]
fn status_and_verify_report_missing_and_partial_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    assert_eq!(store.path(Model::BarkClean), dir.path().join("bark-3.0-clean").join("model.bark"));
    assert_eq!(store.status(Model::Bark), Status::Missing);
    assert!(matches!(store.verify(Model::Bark), Err(AsrError::NotInstalled { model: Model::Bark })));

    let path = store.path(Model::Bark);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"partial").unwrap();
    assert_eq!(store.status(Model::Bark), Status::WrongSize(7));
    let err = store.verify(Model::Bark).unwrap_err();
    assert!(err.to_string().contains("asr fetch bark-3.0"), "{err}");
}

#[test]
fn sha256_matches_the_standard_vector() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("abc");
    fs::write(&path, b"abc").unwrap();
    assert_eq!(sha256_file(&path).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
}

#[test]
fn default_root_ends_in_director_asr() {
    if std::env::var_os("DIRECTOR_ASR_CACHE").is_some() {
        return;
    }
    if let Some(root) = Store::default_root() {
        assert!(root.ends_with("director/asr") || root.ends_with("director\\asr"), "{}", root.display());
    }
}
