use std::fs;
use std::path::PathBuf;

use director_asr::store::{sha256_file, BARK_RS_COMMIT};
use director_asr::{Asset, AsrError, Graphs, Model, Status, Store};

/// A store asset holding `bytes`, with their real size and hash.
fn asset_for(bytes: &[u8]) -> (Asset, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.bin");
    fs::write(&source, bytes).unwrap();
    let sha256: &'static str = Box::leak(sha256_file(&source).unwrap().into_boxed_str());
    let asset = Asset {
        label: "test file".to_string(),
        path: PathBuf::from("set").join("file.bin"),
        url: String::new(),
        size: bytes.len() as u64,
        sha256,
        fetch: "fetch it".to_string(),
    };
    (asset, dir)
}

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
fn graph_sets_pin_the_bark_rs_commit() {
    for g in Graphs::ALL {
        assert_eq!(g.id().parse::<Graphs>().unwrap(), g);
        assert_eq!(serde_json::to_string(&g).unwrap(), format!("\"{}\"", g.id()));
        let assets = g.assets();
        let names: Vec<_> = assets.iter().map(|a| a.path.file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(names, ["encoder.onnx", "decoder.onnx", "joint.onnx", "manifest.json", "vocab.json"]);
        for a in &assets {
            assert!(a.url.contains(BARK_RS_COMMIT), "{}", a.url);
            assert!(a.path.starts_with(g.dir()));
            assert_eq!(a.sha256.len(), 64);
        }
    }
    assert_eq!(Graphs::default(), Graphs::Exact);
    assert_eq!(
        Graphs::Int8.assets()[0].url,
        format!("https://raw.githubusercontent.com/stepupgaming/bark-rs/{BARK_RS_COMMIT}/graphs-int8/encoder.onnx")
    );
    assert!("fp16".parse::<Graphs>().unwrap_err().to_string().contains("exact or int8"));
}

#[test]
fn status_and_verify_report_missing_and_partial_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    let bark = Model::Bark.asset();
    assert_eq!(store.path(&Model::BarkClean.asset()), dir.path().join("bark-3.0-clean").join("model.bark"));
    assert_eq!(store.status(&bark), Status::Missing);
    let err = store.model(Model::Bark).unwrap_err();
    assert!(matches!(err, AsrError::NotInstalled { .. }), "{err}");
    assert!(err.to_string().contains("director-engine asr fetch bark-3.0`"), "{err}");
    let err = store.graphs(Graphs::Int8).unwrap_err();
    assert!(err.to_string().contains("--graphs int8"), "{err}");

    let path = store.path(&bark);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"partial").unwrap();
    assert_eq!(store.status(&bark), Status::WrongSize(7));
    let err = store.verify(&bark).unwrap_err();
    assert!(err.to_string().contains("asr fetch bark-3.0"), "{err}");
}

#[test]
fn import_checks_the_hash_and_installs_the_file() {
    let (asset, src) = asset_for(b"bark graphs");
    let source = src.path().join("source.bin");
    let store_dir = tempfile::tempdir().unwrap();
    let store = Store::new(store_dir.path());
    let installed = store.import(&asset, &source).unwrap();
    assert_eq!(installed, store_dir.path().join("set").join("file.bin"));
    assert_eq!(fs::read(&installed).unwrap(), b"bark graphs");
    assert_eq!(store.verify(&asset).unwrap(), installed);
    // A second import replaces the file in place.
    store.import(&asset, &source).unwrap();
    assert!(!store_dir.path().join("set").join("file.bin.part").exists());

    fs::write(&source, b"bark graphz").unwrap();
    assert!(matches!(store.import(&asset, &source), Err(AsrError::Hash { .. })));
    fs::write(&source, b"short").unwrap();
    assert!(matches!(store.import(&asset, &source), Err(AsrError::Invalid(_))));
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
