use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use image::DynamicImage;
use tokio::time::{sleep, Duration};

use crate::browser::{self, GameBrowser};
use crate::detector;
use crate::state::{AppState, MercExchange};

/// Max spiral rings from center (limits scan to inner kingdom area).
const MAX_SCAN_RINGS: u32 = 6;

pub async fn run_scan(state: AppState, ref_images: Arc<Vec<Arc<DynamicImage>>>) -> Result<()> {
    let config = {
        let s = state.lock().await;
        s.config.clone()
    };

    tracing::info!("starting scanner, launching browser");
    let game = Arc::new(
        GameBrowser::launch(&config)
            .await
            .context("failed to launch browser")?,
    );

    // Store browser in state so the API can take screenshots
    {
        let mut s = state.lock().await;
        s.browser = Some(game.clone());
    }

    tracing::info!("logging in");
    game.login(&config.tb_email, &config.tb_password)
        .await
        .context("login failed")?;

    tracing::info!("login complete, starting kingdom scan loop");

    loop {
        for &kingdom in &config.kingdoms {
            // Check if we should stop
            {
                let s = state.lock().await;
                if !s.running {
                    tracing::info!("scanner stopped by user");
                    return Ok(());
                }
                if s.is_full() {
                    tracing::info!("all exchanges found, stopping");
                    // Drop s before re-acquiring to avoid deadlock
                    drop(s);
                    let mut s = state.lock().await;
                    s.running = false;
                    return Ok(());
                }
            }

            // Update current kingdom
            {
                let mut s = state.lock().await;
                s.current_kingdom = Some(kingdom);
            }

            tracing::info!("scanning kingdom {kingdom}");

            if let Err(e) = scan_kingdom(&game, &state, kingdom, &ref_images, &config.search_target).await {
                tracing::error!("error scanning kingdom {kingdom}: {e:#}");
                // Continue to next kingdom on error
            }
        }

        tracing::info!("completed scan pass, restarting");
    }
}

async fn scan_kingdom(
    game: &GameBrowser,
    state: &AppState,
    kingdom: u32,
    ref_images: &[Arc<DynamicImage>],
    search_target: &str,
) -> Result<()> {
    // Navigate to kingdom center and track our known game position
    let (nav_x, nav_y): (u32, u32) = (512, 512);
    if let Err(e) = game.go_to_kingdom(kingdom).await {
        tracing::warn!("failed to navigate to kingdom {kingdom}: {e:#}, scanning current view");
    }

    sleep(Duration::from_secs(2)).await;

    // Drag-based spiral scan.
    // Each step drags the map by DRAG_PX pixels (roughly one viewport width at 25% zoom).
    const DRAG_PX: i32 = 800;

    let steps = spiral_drag_steps(MAX_SCAN_RINGS);
    tracing::info!("scanning {} positions in kingdom {kingdom}", steps.len());

    for (i, (dx, dy)) in steps.iter().enumerate() {
        // Check if we should stop
        {
            let s = state.lock().await;
            if !s.running || s.is_full() {
                return Ok(());
            }
        }

        // First position (0,0) = current view, no drag needed
        if *dx != 0 || *dy != 0 {
            tracing::info!("step {}/{}: dragging ({}, {})", i + 1, steps.len(), dx, dy);
            if let Err(e) = game.drag_map(dx * DRAG_PX, dy * DRAG_PX).await {
                tracing::warn!("drag failed at step {}: {e:#}", i + 1);
            }
            sleep(Duration::from_secs(1)).await;
        } else {
            tracing::info!("step {}/{}: scanning current view", i + 1, steps.len());
        }

        // Take screenshot and run detection
        let screenshot_bytes = game
            .take_screenshot()
            .await
            .context("failed to take screenshot")?;

        let screenshot = image::load_from_memory(&screenshot_bytes)
            .context("failed to decode screenshot")?;

        // Save every screenshot for debugging
        let scan_path = format!("debug_scan_k{kingdom}_s{:03}.png", i + 1);
        if let Err(e) = tokio::fs::write(&scan_path, &screenshot_bytes).await {
            tracing::warn!("failed to save {scan_path}: {e}");
        }

        let matches = detector::find_matches(&screenshot, ref_images)
            .context("template matching failed")?;

        if matches.is_empty() {
            tracing::info!("step {}/{}: no matches", i + 1, steps.len());
            continue;
        }

        tracing::info!(
            "found {} potential match(es) at step {}/{}",
            matches.len(), i + 1, steps.len()
        );

        // Only one target expected per kingdom - confirm best match and stop
        let m = &matches[0];
        tracing::info!("best match: pixel ({}, {}) score={:.4}", m.x, m.y, m.score);
        if let Err(e) = confirm_match(game, state, kingdom, m.x, m.y, nav_x, nav_y, search_target, ref_images).await {
            tracing::warn!("failed to confirm match at pixel ({}, {}): {e:#}", m.x, m.y);
        }
        tracing::info!("match found at step {}/{}, done with kingdom {kingdom}", i + 1, steps.len());
        return Ok(());
    }

    Ok(())
}

/// Screen center pixel coordinates (where navigated game coords appear).
const SCREEN_CENTER_X: f64 = 960.0;
const SCREEN_CENTER_Y: f64 = 540.0;

/// Calibrated pixel-to-game-coordinate transform (25% zoom).
/// Forward: pixel_dx = PX_PER_GAME_X * game_dx
///          pixel_dy = TILT_Y * game_dx + PX_PER_GAME_Y * game_dy
/// Calibrated from K:111 buildings at (502,512) and (528,524).
const PX_PER_GAME_X: f64 = 49.40;
const PX_PER_GAME_Y: f64 = 28.32;
const TILT_Y: f64 = -1.50; // vertical pixel shift per game X unit

/// Convert a pixel offset from screen center to approximate game coordinate offset.
/// Returns (delta_x, delta_y) in game coordinate units.
fn pixel_to_game_offset(pixel_x: u32, pixel_y: u32) -> (i32, i32) {
    let screen_dx = pixel_x as f64 - SCREEN_CENTER_X;
    let screen_dy = pixel_y as f64 - SCREEN_CENTER_Y;

    let game_dx = screen_dx / PX_PER_GAME_X;
    let game_dy = (screen_dy - TILT_Y * game_dx) / PX_PER_GAME_Y;

    (game_dx.round() as i32, game_dy.round() as i32)
}

#[allow(clippy::too_many_arguments)]
async fn confirm_match(
    game: &GameBrowser,
    state: &AppState,
    kingdom: u32,
    pixel_x: u32,
    pixel_y: u32,
    nav_x: u32,
    nav_y: u32,
    _search_target: &str,
    ref_images: &[Arc<DynamicImage>],
) -> Result<()> {
    // Step 1: Estimate game coordinates from pixel position
    let (gdx, gdy) = pixel_to_game_offset(pixel_x, pixel_y);
    let est_x = (nav_x as i32 + gdx).clamp(0, 1023) as u32;
    let est_y = (nav_y as i32 + gdy).clamp(0, 1023) as u32;

    tracing::info!(
        "match at pixel ({pixel_x}, {pixel_y}), offset from center: ({}, {}), estimated game coords: K:{kingdom} X:{est_x} Y:{est_y}",
        pixel_x as i32 - SCREEN_CENTER_X as i32,
        pixel_y as i32 - SCREEN_CENTER_Y as i32,
    );

    // Step 2: Navigate to the estimated coordinates (centers the target on screen)
    tracing::info!("navigating to estimated coords K:{kingdom} X:{est_x} Y:{est_y}");
    game.navigate_to_coords(kingdom, est_x, est_y).await?;
    sleep(Duration::from_secs(2)).await;

    // Step 3: Screenshot after navigation (target should be near center)
    let goto_bytes = game
        .take_screenshot()
        .await
        .context("failed to take goto screenshot")?;

    let goto_path = format!("debug_goto_k{kingdom}_{est_x}_{est_y}.png");
    if let Err(e) = tokio::fs::write(&goto_path, &goto_bytes).await {
        tracing::warn!("failed to save {goto_path}: {e}");
    } else {
        tracing::info!("saved goto screenshot: {goto_path}");
    }

    // Calibration: re-run template matching on goto screenshot to measure error
    let goto_img = image::load_from_memory(&goto_bytes)
        .context("failed to decode goto screenshot")?;
    if let Some(gm) = detector::find_best_match(&goto_img, ref_images) {
        let err_x = gm.x as f64 - SCREEN_CENTER_X;
        let err_y = gm.y as f64 - SCREEN_CENTER_Y;
        tracing::info!(
            "CALIBRATION: building at pixel ({}, {}), score={:.4}, error from center: ({err_x}, {err_y})",
            gm.x, gm.y, gm.score
        );
    } else {
        tracing::info!("CALIBRATION: no match at all in goto screenshot");
    }

    // Step 4: Click at screen center (where the target should now be)
    tracing::info!("clicking at screen center ({}, {})", SCREEN_CENTER_X, SCREEN_CENTER_Y);
    game.click_at_cdp_full(SCREEN_CENTER_X, SCREEN_CENTER_Y).await?;
    sleep(Duration::from_secs(2)).await;

    // Step 5: Screenshot the popup
    let popup_bytes = game
        .take_screenshot()
        .await
        .context("failed to take popup screenshot")?;

    let popup_path = format!("debug_popup_k{kingdom}_{est_x}_{est_y}.png");
    if let Err(e) = tokio::fs::write(&popup_path, &popup_bytes).await {
        tracing::warn!("failed to save {popup_path}: {e}");
    } else {
        tracing::info!("saved popup screenshot: {popup_path}");
    }

    // Try to read popup text via DOM
    let popup_text = game.read_popup_text().await?;
    tracing::info!("popup text result: {:?}", popup_text);

    if let Some(ref text) = popup_text {
        if let Some((k, x, y)) = browser::parse_popup_coords(text) {
            tracing::info!("found coordinates in popup: K:{k} X:{x} Y:{y}");

            let exchange = MercExchange {
                kingdom: k,
                x,
                y,
                found_at: Utc::now(),
            };

            let mut s = state.lock().await;
            if s.add_exchange(exchange) {
                tracing::info!("added exchange K:{k} X:{x} Y:{y} (total: {})", s.exchanges.len());
            } else {
                tracing::debug!("duplicate or full, skipping K:{k} X:{x} Y:{y}");
            }
        } else {
            tracing::info!("popup text has no coords: {text}");
        }
    } else {
        tracing::info!("no DOM text found in popup (expected for Unity canvas)");
    }

    // Close popup
    game.send_canvas_escape().await;
    sleep(Duration::from_millis(500)).await;

    Ok(())
}

/// Generate relative drag steps for a spiral scan pattern.
/// Returns Vec of (dx, dy) where each is -1, 0, or 1 indicating the drag direction
/// for that step. First entry is always (0, 0) = scan current position.
fn spiral_drag_steps(max_rings: u32) -> Vec<(i32, i32)> {
    let mut steps = Vec::new();
    steps.push((0, 0)); // Start: scan current position

    for ring in 1..=max_rings {
        // Move right one step to start this ring
        steps.push((1, 0));
        // Move up (ring*2 - 1) steps
        for _ in 0..(ring * 2 - 1) {
            steps.push((0, -1));
        }
        // Move left (ring*2) steps
        for _ in 0..(ring * 2) {
            steps.push((-1, 0));
        }
        // Move down (ring*2) steps
        for _ in 0..(ring * 2) {
            steps.push((0, 1));
        }
        // Move right (ring*2) steps
        for _ in 0..(ring * 2) {
            steps.push((1, 0));
        }
        // Move up 1 step to close the ring (position for next ring)
        // Only if not the last ring
        if ring < max_rings {
            steps.push((0, -1));
        }
    }

    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spiral_drag_steps_center() {
        let steps = spiral_drag_steps(1);
        assert_eq!(steps[0], (0, 0)); // Start at center
        assert!(steps.len() > 1);
    }

    #[test]
    fn test_spiral_drag_steps_count() {
        // Ring 1: 1 right + 1 up + 2 left + 2 down + 2 right = 8 steps
        let steps = spiral_drag_steps(1);
        assert_eq!(steps.len(), 1 + 8); // center + ring 1

        // Ring 2 adds: 1 right + 3 up + 4 left + 4 down + 4 right = 16
        // Plus 1 closing step for ring 1
        let steps = spiral_drag_steps(2);
        assert_eq!(steps.len(), 1 + 8 + 1 + 16); // center + ring1 + close + ring2
    }
}
