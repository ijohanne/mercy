use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::scanner;
use crate::state::AppState;

pub fn router(state: AppState, ref_images: Arc<Vec<Arc<DynamicImage>>>) -> Router {
    Router::new()
        .route("/start", post(start_scan))
        .route("/stop", post(stop_scan))
        .route("/status", get(get_status))
        .route("/exchanges", get(get_exchanges))
        .route("/screenshot", get(get_screenshot))
        .route("/goto", get(goto_coords))
        .with_state(ApiState {
            app: state,
            ref_images,
        })
}

#[derive(Clone)]
struct ApiState {
    app: AppState,
    ref_images: Arc<Vec<Arc<DynamicImage>>>,
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

    // Stop existing scanner if running
    if let Some(handle) = state.scanner_handle.take() {
        handle.abort();
    }

    // Clear exchanges and start fresh
    state.exchanges.clear();
    state.running = true;
    state.current_kingdom = None;

    let app_state = api.app.clone();
    let ref_images = api.ref_images.clone();
    let handle = tokio::spawn(async move {
        if let Err(e) = scanner::run_scan(app_state.clone(), ref_images).await {
            tracing::error!("scanner error: {e:#}");
            let mut state = app_state.lock().await;
            state.running = false;
        }
    });

    state.scanner_handle = Some(handle);

    Ok(Json(json!({"status": "started"})))
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
    state.running = false;

    if let Some(handle) = state.scanner_handle.take() {
        handle.abort();
    }

    Ok(Json(json!({"status": "stopped"})))
}

#[derive(Serialize)]
struct StatusResponse {
    running: bool,
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
        running: state.running,
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

    Ok(([(header::CONTENT_TYPE, "image/png")], png_bytes))
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

    Ok(([(header::CONTENT_TYPE, "image/png")], png_bytes))
}
