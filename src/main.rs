mod api;
mod browser;
mod config;
mod detector;
mod scanner;
mod state;

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::state::AppStateInner;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,chromiumoxide::conn=off,chromiumoxide::handler=off")),
        )
        .init();

    let config = Config::from_env().context("failed to load configuration")?;

    tracing::info!(
        "mercy starting, kingdoms: {:?}, listen: {}, target: {}",
        config.kingdoms,
        config.listen_addr,
        config.search_target,
    );

    // Load reference images once at startup
    let ref_images = detector::load_reference_images(&config.search_target)
        .context("failed to load reference images")?;
    let ref_images = Arc::new(ref_images);

    tracing::info!("loaded {} reference image(s)", ref_images.len());

    let state: crate::state::AppState = Arc::new(Mutex::new(AppStateInner::new(config.clone())));

    let app = api::router(state, ref_images).layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind(&config.listen_addr)
        .await
        .context(format!("failed to bind to {}", config.listen_addr))?;

    tracing::info!("listening on {}", config.listen_addr);

    axum::serve(listener, app)
        .await
        .context("server error")?;

    Ok(())
}
