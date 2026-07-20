//! Live preview HTTP server: scrubbable frames for a loaded script/composition.
//!
//! Endpoints:
//! - `GET /` — minimal web player
//! - `GET /api/meta` — width/height/fps/duration + composition list
//! - `GET /api/frame?t=<secs>` or `?frame=<n>` — PNG frame
//! - `POST /api/reload` — re-eval script (same path/opts)

use crate::run::{eval_script, resolve_movie, ResolveOpts};
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use director_core::scripting::MovieHandle;
use director_core::systems::renderer::render_frame;
use director_core::video_wrapper::RenderMode;
use serde::{Deserialize, Serialize};
use skia_safe::{AlphaType, ColorSpace, ColorType, EncodedImageFormat};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tracing::{error, info};

const PLAYER_HTML: &str = include_str!("player.html");

#[derive(Clone)]
struct AppState {
    inner: Arc<Mutex<Session>>,
}

struct Session {
    script: PathBuf,
    composition: Option<String>,
    props: Option<String>,
    movie: MovieHandle,
    /// Registered composition names at last load (empty for legacy scripts).
    composition_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FrameQuery {
    t: Option<f64>,
    frame: Option<u64>,
}

#[derive(Debug, Serialize)]
struct MetaResponse {
    width: i32,
    height: i32,
    fps: f64,
    duration: f64,
    composition: Option<String>,
    compositions: Vec<String>,
    script: String,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

pub struct PreviewArgs {
    pub script: PathBuf,
    pub composition: Option<String>,
    pub props: Option<String>,
    pub bind: SocketAddr,
    pub open_browser: bool,
}

/// Load script, start HTTP server, block until Ctrl-C.
pub async fn run_preview(args: PreviewArgs) -> Result<(), String> {
    let session = load_session(
        &args.script,
        args.composition.as_deref(),
        args.props.as_deref(),
    )?;
    let state = AppState {
        inner: Arc::new(Mutex::new(session)),
    };

    let app = Router::new()
        .route("/", get(player_page))
        .route("/api/meta", get(api_meta))
        .route("/api/frame", get(api_frame))
        .route("/api/reload", post(api_reload))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(args.bind)
        .await
        .map_err(|e| format!("bind {}: {}", args.bind, e))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {}", e))?;

    let url = format!("http://{}/", addr);
    info!("Preview server listening on {}", url);
    eprintln!("Preview: {}", url);

    if args.open_browser {
        let _ = open::that(&url);
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("server error: {}", e))?;
    Ok(())
}

fn load_session(
    script: &std::path::Path,
    composition: Option<&str>,
    props: Option<&str>,
) -> Result<Session, String> {
    let evaluated = eval_script(script, RenderMode::Preview)?;
    let names = evaluated.registry.names();
    let movie = resolve_movie(
        &evaluated,
        ResolveOpts {
            composition,
            props_spec: props,
        },
    )?;
    Ok(Session {
        script: script.to_path_buf(),
        composition: composition.map(|s| s.to_string()),
        props: props.map(|s| s.to_string()),
        movie,
        composition_names: names,
    })
}

async fn player_page() -> Html<&'static str> {
    Html(PLAYER_HTML)
}

async fn api_meta(State(state): State<AppState>) -> Result<Json<MetaResponse>, AppError> {
    let session = state.inner.lock().map_err(|_| AppError::lock())?;
    let director = session
        .movie
        .director
        .lock()
        .map_err(|_| AppError::lock())?;
    let duration = director.timeline_duration().max(0.0);
    Ok(Json(MetaResponse {
        width: director.width,
        height: director.height,
        fps: director.fps,
        duration: if duration > 0.0 { duration } else { 5.0 },
        composition: session.composition.clone(),
        compositions: session.composition_names.clone(),
        script: session.script.display().to_string(),
    }))
}

async fn api_frame(
    State(state): State<AppState>,
    Query(q): Query<FrameQuery>,
) -> Result<Response, AppError> {
    let png = {
        let session = state.inner.lock().map_err(|_| AppError::lock())?;
        let mut director = session
            .movie
            .director
            .lock()
            .map_err(|_| AppError::lock())?;

        let time = if let Some(frame) = q.frame {
            director_core::timeline::frame_to_time(frame, director.fps)
        } else {
            q.t.unwrap_or(0.0).max(0.0)
        };

        render_png(&mut director, time)?
    };

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "no-store")],
        png,
    )
        .into_response())
}

async fn api_reload(State(state): State<AppState>) -> Result<Json<MetaResponse>, AppError> {
    let (script, composition, props) = {
        let session = state.inner.lock().map_err(|_| AppError::lock())?;
        (
            session.script.clone(),
            session.composition.clone(),
            session.props.clone(),
        )
    };

    // Reload off the lock so concurrent frame requests aren't blocked during eval.
    let new_session = tokio::task::spawn_blocking(move || {
        load_session(
            &script,
            composition.as_deref(),
            props.as_deref(),
        )
    })
    .await
    .map_err(|e| AppError::msg(format!("reload join: {}", e)))?
    .map_err(AppError::msg)?;

    {
        let mut session = state.inner.lock().map_err(|_| AppError::lock())?;
        *session = new_session;
    }

    api_meta(State(state)).await
}

fn render_png(director: &mut director_core::Director, time: f64) -> Result<Vec<u8>, AppError> {
    let width = director.width;
    let height = director.height;
    let info = skia_safe::ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        AlphaType::Premul,
        Some(ColorSpace::new_srgb()),
    );
    let mut surface = skia_safe::surfaces::raster(&info, None, None)
        .ok_or_else(|| AppError::msg("failed to create Skia surface"))?;

    render_frame(director, time, surface.canvas())
        .map_err(|e| AppError::msg(format!("render error at t={}: {}", time, e)))?;

    let image = surface.image_snapshot();
    let data = image
        .encode(None, EncodedImageFormat::PNG, 85)
        .ok_or_else(|| AppError::msg("PNG encode failed"))?;
    Ok(data.as_bytes().to_vec())
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

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    info!("Preview server shutting down");
}

struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    fn lock() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "session lock poisoned".into(),
        }
    }

    fn msg(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        error!("preview error: {}", self.message);
        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}
