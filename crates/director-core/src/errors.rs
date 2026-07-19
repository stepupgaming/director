use thiserror::Error;

#[derive(Error, Debug)]
pub enum RenderError {
    #[error("Asset not found: {0}")]
    AssetNotFound(String),
    #[error("Failed to create surface")]
    SurfaceFailure,
    #[error("Graphics error: {0}")]
    SkiaError(String),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
    #[error("Recursion depth limit exceeded")]
    RecursionLimit,
    /// Required bundled fonts missing — export is non-reproducible (Plan 06).
    #[error("Missing required bundled fonts: {0:?}. Run setup_assets.sh / CI font step.")]
    MissingRequiredFonts(Vec<String>),
}
