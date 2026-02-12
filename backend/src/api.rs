use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::detector::{self, PreparedRef};
use crate::scanner;
use crate::state::{AppState, ScannerPhase};

pub fn router(state: AppState, ref_images: Arc<Vec<PreparedRef>>) -> Router {
    Router::new()
        .route("/start", post(start_scan))
        .route("/stop", post(stop_scan))
        .route("/pause", post(pause_scan))
        .route("/prepare", post(prepare_session))
        .route("/logout", post(logout_session))
        .route("/status", get(get_status))
        .route("/exchanges", get(get_exchanges))
        .route("/exchanges/{index}/screenshot", get(get_exchange_screenshot))
        .route("/screenshot", get(get_screenshot))
        .route("/goto", get(goto_coords))
        .route("/detect", get(detect_match))
        .with_state(ApiState {
            app: state,
            ref_images,
        })
}

#[derive(Clone)]
struct ApiState {
    app: AppState,
    ref_images: Arc<Vec<PreparedRef>>,
}

fn check_auth(headers: &HeaderMap, expected_token: &str) -> Result<(), StatusCode> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if let Some(token) = auth.strip_prefix("Bearer ")
        && token == expected_token
    {
        return Ok(());
    }

    Err(StatusCode::UNAUTHORIZED)
}

async fn start_scan(
    State(api): State<ApiState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let token = {
        let state = api.app.lock().await;
        state.config.auth_token.clone()
    };
    check_auth(&headers, &token)?;

    let mut state = api.app.lock().await;

    match state.phase {
        ScannerPhase::Paused => {
            // Resume: set phase to Scanning and wake the paused scanner
            state.phase = ScannerPhase::Scanning;
            state.pause_notify.notify_one();
            Ok(Json(json!({"status": "resumed"})))
        }
        ScannerPhase::Idle | ScannerPhase::Ready => {
            // Stop existing scanner handle if any
            if let Some(handle) = state.scanner_handle.take() {
                handle.abort();
            }

            // Clear exchanges and start fresh
            state.exchanges.clear();
            state.current_kingdom = None;

            let app_state = api.app.clone();
            let ref_images = api.ref_images.clone();
            let handle = tokio::spawn(async move {
                if let Err(e) = scanner::run_scan(app_state.clone(), ref_images).await {
                    tracing::error!("scanner error: {e:#}");
                    let mut state = app_state.lock().await;
                    state.phase = ScannerPhase::Idle;
                }
            });

            state.scanner_handle = Some(handle);

            Ok(Json(json!({"status": "started"})))
        }
        ScannerPhase::Scanning | ScannerPhase::Preparing => {
            Err(StatusCode::CONFLICT)
        }
    }
}

async fn stop_scan(
    State(api): State<ApiState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let token = {
        let state = api.app.lock().await;
        state.config.auth_token.clone()
    };
    check_auth(&headers, &token)?;

    let mut state = api.app.lock().await;

    if let Some(handle) = state.scanner_handle.take() {
        handle.abort();
    }

    // Wake any paused waiter so it can exit
    state.pause_notify.notify_one();

    // Keep browser alive: Ready if browser exists, Idle otherwise
    state.phase = if state.browser.is_some() {
        ScannerPhase::Ready
    } else {
        ScannerPhase::Idle
    };

    Ok(Json(json!({"status": "stopped"})))
}

async fn pause_scan(
    State(api): State<ApiState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let token = {
        let state = api.app.lock().await;
        state.config.auth_token.clone()
    };
    check_auth(&headers, &token)?;

    let mut state = api.app.lock().await;

    match state.phase {
        ScannerPhase::Scanning => {
            state.phase = ScannerPhase::Paused;
            Ok(Json(json!({"status": "paused"})))
        }
        ScannerPhase::Paused => {
            // Idempotent
            Ok(Json(json!({"status": "paused"})))
        }
        _ => Err(StatusCode::CONFLICT),
    }
}

async fn prepare_session(
    State(api): State<ApiState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let token = {
        let state = api.app.lock().await;
        state.config.auth_token.clone()
    };
    check_auth(&headers, &token)?;

    let state = api.app.lock().await;

    match state.phase {
        ScannerPhase::Idle => {
            drop(state);

            let app_state = api.app.clone();
            tokio::spawn(async move {
                if let Err(e) = scanner::prepare_browser(&app_state).await {
                    tracing::error!("prepare failed: {e:#}");
                    let mut s = app_state.lock().await;
                    s.phase = ScannerPhase::Idle;
                }
            });

            Ok(Json(json!({"status": "preparing"})))
        }
        ScannerPhase::Ready | ScannerPhase::Paused => {
            Ok(Json(json!({"status": "ready"})))
        }
        ScannerPhase::Preparing | ScannerPhase::Scanning => {
            Err(StatusCode::CONFLICT)
        }
    }
}

async fn logout_session(
    State(api): State<ApiState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let token = {
        let state = api.app.lock().await;
        state.config.auth_token.clone()
    };
    check_auth(&headers, &token)?;

    let mut state = api.app.lock().await;

    // Abort scanner if running
    if let Some(handle) = state.scanner_handle.take() {
        handle.abort();
    }

    // Wake any paused waiter so it can exit
    state.pause_notify.notify_one();

    // Drop browser (kills Chromium)
    state.browser = None;
    state.phase = ScannerPhase::Idle;

    Ok(Json(json!({"status": "logged_out"})))
}

#[derive(Serialize)]
struct StatusResponse {
    phase: ScannerPhase,
    running: bool,
    paused: bool,
    current_kingdom: Option<u32>,
    exchanges_found: usize,
}

async fn get_status(
    State(api): State<ApiState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let state = api.app.lock().await;
    check_auth(&headers, &state.config.auth_token)?;

    Ok(Json(StatusResponse {
        phase: state.phase,
        running: state.phase == ScannerPhase::Scanning,
        paused: state.phase == ScannerPhase::Paused,
        current_kingdom: state.current_kingdom,
        exchanges_found: state.exchanges.len(),
    }))
}

async fn get_exchanges(
    State(api): State<ApiState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let state = api.app.lock().await;
    check_auth(&headers, &state.config.auth_token)?;

    Ok(Json(state.exchanges.clone()))
}

async fn get_exchange_screenshot(
    State(api): State<ApiState>,
    headers: HeaderMap,
    Path(index): Path<usize>,
) -> Result<impl IntoResponse, StatusCode> {
    let state = api.app.lock().await;
    check_auth(&headers, &state.config.auth_token)?;

    let exchange = state.exchanges.get(index).ok_or(StatusCode::NOT_FOUND)?;
    let png = exchange.screenshot_png.clone().ok_or(StatusCode::NOT_FOUND)?;
    let filename = format!("exchange_k{}_{}_{}.png", exchange.kingdom, exchange.x, exchange.y);

    Ok((
        [
            (header::CONTENT_TYPE, "image/png".to_owned()),
            (header::CONTENT_DISPOSITION, format!("inline; filename=\"{filename}\"")),
        ],
        png,
    ))
}

async fn get_screenshot(
    State(api): State<ApiState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let state = api.app.lock().await;
    check_auth(&headers, &state.config.auth_token)?;

    let browser = state.browser.clone().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    drop(state); // Release lock before async screenshot

    let png_bytes = browser
        .take_screenshot()
        .await
        .map_err(|e| {
            tracing::error!("screenshot failed: {e:#}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Store for detect to reuse
    api.app.lock().await.last_screenshot = Some(png_bytes.clone());

    Ok((
        [
            (header::CONTENT_TYPE, "image/png".to_owned()),
            (header::CONTENT_DISPOSITION, "inline; filename=\"screenshot.png\"".to_owned()),
        ],
        png_bytes,
    ))
}

#[derive(Deserialize)]
struct GotoParams {
    k: u32,
    x: u32,
    y: u32,
}

async fn goto_coords(
    State(api): State<ApiState>,
    headers: HeaderMap,
    Query(params): Query<GotoParams>,
) -> Result<impl IntoResponse, StatusCode> {
    let state = api.app.lock().await;
    check_auth(&headers, &state.config.auth_token)?;

    let browser = state.browser.clone().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    drop(state);

    browser
        .navigate_to_coords(params.k, params.x, params.y)
        .await
        .map_err(|e| {
            tracing::error!("goto failed: {e:#}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let png_bytes = browser
        .take_screenshot()
        .await
        .map_err(|e| {
            tracing::error!("screenshot failed: {e:#}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Store for detect to reuse
    api.app.lock().await.last_screenshot = Some(png_bytes.clone());

    let filename = format!("goto_k{}_{}_{}.png", params.k, params.x, params.y);
    Ok((
        [
            (header::CONTENT_TYPE, "image/png".to_owned()),
            (header::CONTENT_DISPOSITION, format!("inline; filename=\"{filename}\"")),
        ],
        png_bytes,
    ))
}

#[derive(Serialize)]
struct DetectResponse {
    found: bool,
    threshold: f32,
    pixel_x: Option<u32>,
    pixel_y: Option<u32>,
    score: Option<f32>,
    game_dx: Option<i32>,
    game_dy: Option<i32>,
}

async fn detect_match(
    State(api): State<ApiState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    let state = api.app.lock().await;
    check_auth(&headers, &state.config.auth_token)?;

    // Reuse the last screenshot from goto/refresh instead of taking a new one,
    // because the game view drifts after navigation.
    let png_bytes = state.last_screenshot.clone().ok_or_else(|| {
        tracing::error!("no screenshot available — use goto or refresh first");
        StatusCode::BAD_REQUEST
    })?;
    drop(state);

    let screenshot = image::load_from_memory(&png_bytes).map_err(|e| {
        tracing::error!("decode failed: {e:#}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let best = detector::find_best_match(&screenshot, &api.ref_images);

    // Lower threshold for manual testing — scanner uses MATCH_THRESHOLD
    const DETECT_THRESHOLD: f32 = 0.88;

    let resp = match best {
        Some(m) => {
            let (gdx, gdy) = scanner::pixel_to_game_offset(m.x, m.y);
            DetectResponse {
                found: m.score >= DETECT_THRESHOLD,
                threshold: DETECT_THRESHOLD,
                pixel_x: Some(m.x),
                pixel_y: Some(m.y),
                score: Some(m.score),
                game_dx: Some(gdx),
                game_dy: Some(gdy),
            }
        }
        None => DetectResponse {
            found: false,
            threshold: DETECT_THRESHOLD,
            pixel_x: None,
            pixel_y: None,
            score: None,
            game_dx: None,
            game_dy: None,
        },
    };

    Ok(Json(resp))
}
