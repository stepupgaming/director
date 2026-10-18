//! Native preview endpoints. The embedded player is an unchanged optional client.
mod worker;

use crate::input::Input;
use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
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

async fn api_audio(State(worker): State<Worker>, headers: HeaderMap) -> Result<Response, AppError> {
    // One cached WAV per accepted revision. Ranges slice that body; they do not remix.
    let audio = worker
        .request(|session| session.audio())
        .await
        .map_err(AppError)?;
    let range = resolve_byte_range(headers.get(header::RANGE), audio.len());
    Ok(wav_response(audio, range))
}

enum ByteRange {
    Absent,
    Satisfied { start: usize, end: usize },
    Unsatisfiable,
}

fn wav_response(audio: Vec<u8>, range: ByteRange) -> Response {
    match range {
        ByteRange::Absent => (
            [
                (header::CONTENT_TYPE, "audio/wav"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            audio,
        )
            .into_response(),
        ByteRange::Satisfied { start, end } => {
            let body = audio[start..=end].to_vec();
            let total = audio.len();
            (
                StatusCode::PARTIAL_CONTENT,
                [
                    (header::CONTENT_TYPE, "audio/wav".to_string()),
                    (header::CACHE_CONTROL, "no-store".to_string()),
                    (header::ACCEPT_RANGES, "bytes".to_string()),
                    (header::CONTENT_LENGTH, body.len().to_string()),
                    (
                        header::CONTENT_RANGE,
                        format!("bytes {start}-{end}/{total}"),
                    ),
                ],
                body,
            )
                .into_response()
        }
        ByteRange::Unsatisfiable => {
            let total = audio.len();
            (
                StatusCode::RANGE_NOT_SATISFIABLE,
                [
                    (header::CACHE_CONTROL, "no-store".to_string()),
                    (header::ACCEPT_RANGES, "bytes".to_string()),
                    (header::CONTENT_LENGTH, "0".to_string()),
                    (header::CONTENT_RANGE, format!("bytes */{total}")),
                ],
                (),
            )
                .into_response()
        }
    }
}

/// Inclusive byte ranges. Syntax that is not a single `bytes` range is ignored.
/// A well-formed range that does not overlap the WAV is unsatisfiable.
fn resolve_byte_range(header: Option<&HeaderValue>, total: usize) -> ByteRange {
    let Some(text) = header.and_then(|value| value.to_str().ok()) else {
        return ByteRange::Absent;
    };
    let Some((unit, spec)) = text.trim().split_once('=') else {
        return ByteRange::Absent;
    };
    if !unit.trim().eq_ignore_ascii_case("bytes") {
        return ByteRange::Absent;
    }
    let spec = spec.trim();
    if spec.is_empty() || spec.contains(',') {
        return ByteRange::Absent;
    }
    if let Some(suffix) = spec.strip_prefix('-') {
        let Some(length) = parse_bound(suffix.trim()) else {
            return ByteRange::Absent;
        };
        if length == 0 || total == 0 {
            return ByteRange::Unsatisfiable;
        }
        let length = usize::try_from(length).unwrap_or(usize::MAX);
        return ByteRange::Satisfied {
            start: total.saturating_sub(length),
            end: total - 1,
        };
    }
    let Some((start_text, end_text)) = spec.split_once('-') else {
        return ByteRange::Absent;
    };
    let Some(start) = parse_bound(start_text.trim()) else {
        return ByteRange::Absent;
    };
    if start >= total as u128 {
        return ByteRange::Unsatisfiable;
    }
    let start = start as usize;
    let end_text = end_text.trim();
    if end_text.is_empty() {
        return ByteRange::Satisfied {
            start,
            end: total - 1,
        };
    }
    let Some(end) = parse_bound(end_text) else {
        return ByteRange::Absent;
    };
    if end < start as u128 {
        return ByteRange::Unsatisfiable;
    }
    ByteRange::Satisfied {
        start,
        end: usize::try_from(end).unwrap_or(usize::MAX).min(total - 1),
    }
}

fn parse_bound(text: &str) -> Option<u128> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(text.parse::<u128>().unwrap_or(u128::MAX))
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

    fn hdr<'a>(response: &'a Response, name: &header::HeaderName) -> &'a str {
        response.headers().get(name).unwrap().to_str().unwrap()
    }

    async fn ranged(app: &Router, range: &str) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/api/audio")
                    .header(header::RANGE, range)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    #[test]
    fn byte_ranges_are_inclusive_and_unsatisfiable_ranges_do_not_overlap() {
        assert!(matches!(resolve_byte_range(None, 10), ByteRange::Absent));
        assert!(matches!(
            resolve_byte_range(Some(&HeaderValue::from_static("bytes=0-3")), 10),
            ByteRange::Satisfied { start: 0, end: 3 }
        ));
        assert!(matches!(
            resolve_byte_range(Some(&HeaderValue::from_static("bytes=4-")), 10),
            ByteRange::Satisfied { start: 4, end: 9 }
        ));
        assert!(matches!(
            resolve_byte_range(Some(&HeaderValue::from_static("bytes=-2")), 10),
            ByteRange::Satisfied { start: 8, end: 9 }
        ));
        assert!(matches!(
            resolve_byte_range(Some(&HeaderValue::from_static("bytes=-100")), 10),
            ByteRange::Satisfied { start: 0, end: 9 }
        ));
        assert!(matches!(
            resolve_byte_range(Some(&HeaderValue::from_static("bytes=0-99")), 10),
            ByteRange::Satisfied { start: 0, end: 9 }
        ));
        assert!(matches!(
            resolve_byte_range(Some(&HeaderValue::from_static("bytes=10-")), 10),
            ByteRange::Unsatisfiable
        ));
        assert!(matches!(
            resolve_byte_range(Some(&HeaderValue::from_static("bytes=-0")), 10),
            ByteRange::Unsatisfiable
        ));
        assert!(matches!(
            resolve_byte_range(Some(&HeaderValue::from_static("bytes=8-3")), 10),
            ByteRange::Unsatisfiable
        ));
        assert!(matches!(
            resolve_byte_range(Some(&HeaderValue::from_static("items=0-1")), 10),
            ByteRange::Absent
        ));
        assert!(matches!(
            resolve_byte_range(Some(&HeaderValue::from_static("bytes=0-1,2-3")), 10),
            ByteRange::Absent
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn audio_ranges_slice_the_cached_revision_wav() {
        let directory = tempfile::tempdir().unwrap();
        let app = router(Worker::start(source(directory.path()), false).unwrap());
        let full = get(&app, "/api/audio").await;
        assert_eq!(full.status(), StatusCode::OK);
        assert!(full.headers().get(header::CONTENT_RANGE).is_none());
        let audio = bytes(full).await;
        assert_eq!(&audio[..4], b"RIFF");
        let total = audio.len();

        let head = ranged(&app, "bytes=0-43").await;
        assert_eq!(head.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(hdr(&head, &header::ACCEPT_RANGES), "bytes");
        assert_eq!(hdr(&head, &header::CONTENT_TYPE), "audio/wav");
        assert_eq!(hdr(&head, &header::CACHE_CONTROL), "no-store");
        assert_eq!(hdr(&head, &header::CONTENT_LENGTH), "44");
        assert_eq!(
            hdr(&head, &header::CONTENT_RANGE),
            format!("bytes 0-43/{total}")
        );
        assert_eq!(bytes(head).await, audio[..44]);

        let wave = ranged(&app, "bytes=8-11").await;
        assert_eq!(wave.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            hdr(&wave, &header::CONTENT_RANGE),
            format!("bytes 8-11/{total}")
        );
        assert_eq!(bytes(wave).await, b"WAVE");

        let open = ranged(&app, "bytes=44-").await;
        assert_eq!(open.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            hdr(&open, &header::CONTENT_RANGE),
            format!("bytes 44-{}/{total}", total - 1)
        );
        assert_eq!(
            hdr(&open, &header::CONTENT_LENGTH),
            (total - 44).to_string()
        );
        assert_eq!(bytes(open).await, audio[44..]);

        let suffix = ranged(&app, "bytes=-8").await;
        assert_eq!(suffix.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            hdr(&suffix, &header::CONTENT_RANGE),
            format!("bytes {}-{}/{total}", total - 8, total - 1)
        );
        assert_eq!(bytes(suffix).await, audio[total - 8..]);

        let wide_suffix = ranged(&app, &format!("bytes=-{}", total + 10)).await;
        assert_eq!(wide_suffix.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            hdr(&wide_suffix, &header::CONTENT_RANGE),
            format!("bytes 0-{}/{total}", total - 1)
        );
        assert_eq!(bytes(wide_suffix).await, audio);

        let clamped = ranged(&app, &format!("bytes=0-{}", total + 100)).await;
        assert_eq!(clamped.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            hdr(&clamped, &header::CONTENT_RANGE),
            format!("bytes 0-{}/{total}", total - 1)
        );
        assert_eq!(bytes(clamped).await, audio);

        let past = ranged(&app, &format!("bytes={total}-")).await;
        assert_eq!(past.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            hdr(&past, &header::CONTENT_RANGE),
            format!("bytes */{total}")
        );
        assert_eq!(hdr(&past, &header::ACCEPT_RANGES), "bytes");
        assert_eq!(hdr(&past, &header::CONTENT_LENGTH), "0");
        assert!(bytes(past).await.is_empty());

        let inverted = ranged(&app, "bytes=20-10").await;
        assert_eq!(inverted.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            hdr(&inverted, &header::CONTENT_RANGE),
            format!("bytes */{total}")
        );

        let again = get(&app, "/api/audio").await;
        assert_eq!(again.status(), StatusCode::OK);
        assert_eq!(bytes(again).await, audio);
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
