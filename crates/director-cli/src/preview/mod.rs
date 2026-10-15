//! Native preview endpoints. The embedded player is an unchanged optional client.
mod worker;

use crate::input::Input;
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tracing::{error, info};
use worker::Worker;

const PLAYER_HTML: &str = include_str!("player.html");

#[derive(Debug, Default, Deserialize)]
struct FrameQuery {
    t: Option<f64>,
    frame: Option<u64>,
}

pub struct PreviewArgs {
    pub input: Input,
    pub gpu: bool,
    pub bind: SocketAddr,
    pub open_browser: bool,
}

pub async fn run_preview(args: PreviewArgs) -> Result<(), String> {
    let worker = tokio::task::spawn_blocking(move || Worker::start(args.input, args.gpu))
        .await
        .map_err(|e| format!("native worker initialization: {e}"))??;
    let app = router(worker);
    let listener = tokio::net::TcpListener::bind(args.bind)
        .await
        .map_err(|e| format!("bind {}: {e}", args.bind))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let url = format!("http://{addr}/");
    eprintln!("Preview: {url}");
    if args.open_browser {
        let _ = open::that(&url);
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("server error: {e}"))
}

fn router(worker: Worker) -> Router {
    Router::new()
        .route("/", get(|| async { Html(PLAYER_HTML) }))
        .route("/api/meta", get(api_meta))
        .route("/api/frame", get(api_frame))
        .route("/api/inspect", get(api_inspect))
        .route("/api/audio", get(api_audio))
        .route("/api/reload", post(api_reload))
        .layer(CorsLayer::permissive())
        .with_state(worker)
}

async fn api_meta(State(worker): State<Worker>) -> Result<Json<serde_json::Value>, AppError> {
    worker
        .request(|session| session.metadata())
        .await
        .map(Json)
        .map_err(AppError)
}

async fn api_frame(
    State(worker): State<Worker>,
    Query(query): Query<FrameQuery>,
) -> Result<Response, AppError> {
    let png = worker
        .request(move |session| {
            let fps = session
                .loaded
                .movie
                .director
                .lock()
                .map_err(|_| "Director lock poisoned")?
                .fps;
            let time = query.frame.map_or(query.t.unwrap_or(0.0), |frame| {
                director_core::timeline::frame_to_time(frame, fps)
            });
            session.frame(time)
        })
        .await
        .map_err(AppError)?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        png,
    )
        .into_response())
}

async fn api_inspect(
    State(worker): State<Worker>,
    Query(query): Query<FrameQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    worker
        .request(move |session| {
            let fps = session
                .loaded
                .movie
                .director
                .lock()
                .map_err(|_| "Director lock poisoned")?
                .fps;
            let time = query.frame.map_or(query.t.unwrap_or(0.0), |frame| {
                director_core::timeline::frame_to_time(frame, fps)
            });
            session.loaded.inspect(time)
        })
        .await
        .map(Json)
        .map_err(AppError)
}

async fn api_audio(State(worker): State<Worker>) -> Result<Response, AppError> {
    let audio = worker
        .request(|session| session.audio())
        .await
        .map_err(AppError)?;
    Ok((
        [
            (header::CONTENT_TYPE, "audio/wav"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        audio,
    )
        .into_response())
}

async fn api_reload(State(worker): State<Worker>) -> Result<Json<serde_json::Value>, AppError> {
    worker
        .request(|session| {
            session.reload()?;
            session.metadata()
        })
        .await
        .map(Json)
        .map_err(AppError)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
    info!("Preview server shutting down");
}

struct AppError(String);
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        error!("preview error: {}", self.0);
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":self.0})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;

    async fn get(app: &Router, uri: &str) -> Response {
        app.clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn bytes(response: Response) -> Vec<u8> {
        to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec()
    }

    fn source(directory: &std::path::Path) -> Input {
        std::fs::write(
            directory.join("style.css"),
            "body{margin:0}#box{width:32px;height:32px;background:red}",
        )
        .unwrap();
        let input = directory.join("scene.html");
        std::fs::write(&input, "<link rel='stylesheet' href='style.css'><main data-composition-id='main' data-width='32' data-height='32' data-duration='.6'><div id='box'></div></main>").unwrap();
        Input::new(input)
    }

    #[tokio::test]
    async fn native_endpoints_reload_dependencies_and_preserve_the_accepted_scene_on_error() {
        let directory = tempfile::tempdir().unwrap();
        let app = router(Worker::start(source(directory.path()), false).unwrap());
        let initial = bytes(get(&app, "/api/frame?frame=0").await).await;
        assert_eq!(
            image::load_from_memory(&initial)
                .unwrap()
                .to_rgba8()
                .get_pixel(16, 16)
                .0,
            [255, 0, 0, 255]
        );
        let inspection: serde_json::Value =
            serde_json::from_slice(&bytes(get(&app, "/api/inspect?t=.1").await).await).unwrap();
        assert!(inspection["element_ids"].get("box").is_some());
        std::fs::write(
            directory.path().join("style.css"),
            "#box{perspective:400px}",
        )
        .unwrap();
        let failed: serde_json::Value =
            serde_json::from_slice(&bytes(get(&app, "/api/meta").await).await).unwrap();
        assert_eq!(failed["revision"], 1);
        assert!(failed["reload_error"]
            .as_str()
            .unwrap()
            .contains("perspective"));
        assert_eq!(bytes(get(&app, "/api/frame?t=.1").await).await, initial);
        let rejected = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/reload")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        std::fs::write(
            directory.path().join("style.css"),
            "body{margin:0}#box{width:32px;height:32px;background:blue}",
        )
        .unwrap();
        let corrected = bytes(get(&app, "/api/frame?t=.1").await).await;
        assert_eq!(
            image::load_from_memory(&corrected)
                .unwrap()
                .to_rgba8()
                .get_pixel(16, 16)
                .0,
            [0, 0, 255, 255]
        );
        let metadata: serde_json::Value =
            serde_json::from_slice(&bytes(get(&app, "/api/meta").await).await).unwrap();
        assert_eq!(metadata["renderer"], "cpu");
        assert_eq!(metadata["revision"], 2);
        assert!(metadata["reload_error"].is_null());
        assert_eq!(
            get(&app, "/api/frame?t=NaN").await.status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn worker_serializes_requests_and_audio_uses_the_export_clock() {
        let directory = tempfile::tempdir().unwrap();
        let worker = Worker::start(source(directory.path()), false).unwrap();
        let (first, second) = tokio::join!(
            worker.request(|_| Ok(std::thread::current().id())),
            worker.request(|_| Ok(std::thread::current().id())),
        );
        assert_eq!(first.unwrap(), second.unwrap());
        let app = router(worker);
        let audio = bytes(get(&app, "/api/audio").await).await;
        assert_eq!(&audio[..4], b"RIFF");
        assert_eq!(audio.len(), 44 + 18 * 1600 * 2 * 4);
        let decoded = director_core::audio::load_audio_bytes(&audio, 48000).unwrap();
        assert_eq!(decoded.len(), 18 * 1600 * 2);
        assert!(decoded.iter().all(|s| *s == 0.0));
        assert_eq!(audio, bytes(get(&app, "/api/audio").await).await);
    }

    #[cfg(feature = "vulkan")]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires a live Vulkan device; run explicitly"]
    async fn gpu_preview_worker_keeps_the_context_on_its_owner_thread() {
        let directory = tempfile::tempdir().unwrap();
        let worker = Worker::start(source(directory.path()), true).unwrap();
        worker
            .request(|session| {
                let mut director = session
                    .loaded
                    .movie
                    .director
                    .lock()
                    .map_err(|_| "Director lock poisoned")?;
                director.samples_per_frame = 4;
                director.shutter_angle = 180.0;
                Ok(())
            })
            .await
            .unwrap();
        let app = router(worker);
        let metadata: serde_json::Value =
            serde_json::from_slice(&bytes(get(&app, "/api/meta").await).await).unwrap();
        assert_eq!(metadata["renderer"], "vulkan");
        let (first, second) =
            tokio::join!(get(&app, "/api/frame?t=.1"), get(&app, "/api/frame?t=.4"));
        let first = bytes(first).await;
        assert_eq!(first, bytes(second).await);
        assert_eq!(
            image::load_from_memory(&first)
                .unwrap()
                .to_rgba8()
                .get_pixel(16, 16)
                .0,
            [255, 0, 0, 255]
        );
    }
}
