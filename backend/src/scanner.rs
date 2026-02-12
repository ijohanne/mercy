use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use tokio::time::{sleep, Duration};

use crate::browser::{self, GameBrowser};
use crate::config::Config;
use crate::detector::{self, PreparedRef};
use crate::state::{AppState, MercExchange, ScannerPhase};

#[derive(Debug, Serialize)]
struct ExchangeLogEntry {
    timestamp: String,
    kingdom: u32,
    x: u32,
    y: u32,
    confirmed: bool,
    stored: bool,
    initial_score: f32,
    calibration_score: Option<f32>,
    scan_pattern: String,
    scan_duration_secs: Option<f64>,
}

fn log_exchange(config: &Config, entry: &ExchangeLogEntry) {
    use std::fs::OpenOptions;
    use std::io::Write;

    let line = match serde_json::to_string(entry) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("failed to serialize exchange log entry: {e}");
            return;
        }
    };

    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config.exchange_log)
    {
        Ok(mut f) => {
            if let Err(e) = writeln!(f, "{line}") {
                tracing::warn!("failed to write to {}: {e}", config.exchange_log);
            }
        }
        Err(e) => {
            tracing::warn!("failed to open {}: {e}", config.exchange_log);
        }
    }
}

/// Game-coordinate step between scan positions.
/// Viewport covers ~34×33 game units (usable area at 25% zoom),
/// so step=25 gives ~25% overlap for reliable detection.
const SCAN_STEP: u32 = 25;

/// Launch browser and log in if not already done. Sets phase Idle → Preparing → Ready.
/// If a browser already exists, returns it without relaunching.
pub async fn prepare_browser(state: &AppState) -> Result<Arc<GameBrowser>> {
    // Fast path: browser already exists
    {
        let s = state.lock().await;
        if let Some(ref browser) = s.browser {
            return Ok(browser.clone());
        }
    }

    // Set phase to Preparing
    let config = {
        let mut s = state.lock().await;
        s.phase = ScannerPhase::Preparing;
        s.config.clone()
    };

    tracing::info!("launching browser");
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

    // Set phase to Ready
    {
        let mut s = state.lock().await;
        s.phase = ScannerPhase::Ready;
    }

    tracing::info!("browser ready");
    Ok(game)
}

/// Check whether the scan loop should continue. If paused, blocks until resumed.
/// Returns `true` for Scanning, `false` for anything else (stopped, idle, etc.).
async fn check_should_continue(state: &AppState) -> bool {
    loop {
        let (phase, notify) = {
            let s = state.lock().await;
            (s.phase, s.pause_notify.clone())
        };
        match phase {
            ScannerPhase::Scanning => return true,
            ScannerPhase::Paused => {
                tracing::info!("scanner paused, waiting for resume");
                notify.notified().await;
                // Re-check phase after wakeup
            }
            _ => return false,
        }
    }
}

pub async fn run_scan(state: AppState, ref_images: Arc<Vec<PreparedRef>>) -> Result<()> {
    let config = {
        let s = state.lock().await;
        s.config.clone()
    };

    let game = prepare_browser(&state).await?;

    // Set phase to Scanning
    {
        let mut s = state.lock().await;
        s.phase = ScannerPhase::Scanning;
    }

    tracing::info!("starting kingdom scan loop");

    let cooldown = chrono::Duration::minutes(2);

    loop {
        for &kingdom in &config.kingdoms {
            if !check_should_continue(&state).await {
                tracing::info!("scanner stopped");
                return Ok(());
            }

            // Update current kingdom
            {
                let mut s = state.lock().await;
                s.current_kingdom = Some(kingdom);
            }

            // Cooldown + re-verification logic
            let (last_scan, known_exchange) = {
                let s = state.lock().await;
                (s.last_scan_time(kingdom), s.exchange_for_kingdom(kingdom))
            };

            if let Some(last) = last_scan {
                let elapsed = Utc::now() - last;
                if elapsed < cooldown {
                    if let Some((ex, ey)) = known_exchange {
                        // Re-verify: navigate to known location, check if still there
                        tracing::info!("kingdom {kingdom}: re-verifying exchange at ({ex}, {ey})");
                        match verify_exchange(&game, kingdom, ex, ey, &ref_images, &config).await {
                            Ok(true) => {
                                tracing::info!("kingdom {kingdom}: exchange still present");
                                let mut s = state.lock().await;
                                s.refresh_exchange(kingdom, ex, ey);
                                let remaining = (cooldown - elapsed)
                                    .to_std()
                                    .unwrap_or_default();
                                drop(s);
                                sleep(remaining).await;
                                continue;
                            }
                            Ok(false) => {
                                tracing::info!("kingdom {kingdom}: exchange gone, removing");
                                let mut s = state.lock().await;
                                s.remove_exchange(kingdom);
                                // Fall through to full scan
                            }
                            Err(e) => {
                                tracing::warn!("kingdom {kingdom}: verify failed: {e:#}");
                                // Fall through to full scan
                            }
                        }
                    } else {
                        // No known exchange but recently scanned — wait out cooldown
                        tracing::info!("kingdom {kingdom}: cooldown active, waiting");
                        let remaining = (cooldown - elapsed)
                            .to_std()
                            .unwrap_or_default();
                        sleep(remaining).await;
                        // Fall through to full scan after cooldown
                    }
                }
            }

            // Full spiral scan
            tracing::info!("scanning kingdom {kingdom}");
            if let Err(e) = scan_kingdom(&game, &state, kingdom, &ref_images, &config).await {
                tracing::error!("error scanning kingdom {kingdom}: {e:#}");
            }

            {
                let mut s = state.lock().await;
                s.set_last_scan_time(kingdom);
            }
        }

        tracing::info!("completed scan pass, restarting");
    }
}

/// Navigate to known exchange coordinates, screenshot, and check if the exchange
/// is still visible near screen center (within ~80px, score >= 0.90).
async fn verify_exchange(
    game: &GameBrowser,
    kingdom: u32,
    x: u32,
    y: u32,
    ref_images: &[PreparedRef],
    _config: &Config,
) -> Result<bool> {
    game.navigate_to_coords(kingdom, x, y).await?;
    sleep(Duration::from_secs(2)).await;

    let screenshot_bytes = game
        .take_screenshot()
        .await
        .context("failed to take verification screenshot")?;

    let screenshot = image::load_from_memory(&screenshot_bytes)
        .context("failed to decode verification screenshot")?;

    match detector::find_best_match(&screenshot, ref_images) {
        Some(m) => {
            let err_x = (m.x as f64 - SCREEN_CENTER_X).abs();
            let err_y = (m.y as f64 - SCREEN_CENTER_Y).abs();
            let near_center = err_x < 80.0 && err_y < 80.0;
            let good_score = m.score >= 0.90;
            tracing::info!(
                "verify K:{kingdom} ({x},{y}): pixel ({},{}) score={:.4} err=({err_x:.0},{err_y:.0}) near={near_center} good={good_score}",
                m.x, m.y, m.score
            );
            Ok(near_center && good_score)
        }
        None => {
            tracing::info!("verify K:{kingdom} ({x},{y}): no match found");
            Ok(false)
        }
    }
}

struct DetectionResult {
    matches: Vec<detector::TemplateMatch>,
    nav_x: u32,
    nav_y: u32,
    step_index: usize,
}

async fn scan_kingdom(
    game: &GameBrowser,
    state: &AppState,
    kingdom: u32,
    ref_images: &Arc<Vec<PreparedRef>>,
    config: &Config,
) -> Result<()> {
    let positions = match config.scan_pattern.as_str() {
        "single" => spiral_scan_positions(512, 512, SCAN_STEP, config.scan_rings.unwrap_or(4)),
        "multi" => multi_spiral_positions(SCAN_STEP, config.scan_rings.unwrap_or(4)),
        "wide" => wide_spiral_positions(config.scan_rings.unwrap_or(9)),
        "grid" => grid_scan_positions(),
        "known" => known_spiral_positions(config.known_locations_file.as_deref(), SCAN_STEP, config.scan_rings.unwrap_or(1)),
        _ => grid_scan_positions(),
    };
    let total = positions.len();
    tracing::info!(
        "scanning {total} positions in kingdom {kingdom} (pattern={})",
        config.scan_pattern
    );

    let scan_start = Instant::now();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DetectionResult>();

    for (i, &(gx, gy)) in positions.iter().enumerate() {
        // Check for detection result from previous step (non-blocking)
        if let Ok(det) = rx.try_recv() {
            let m = &det.matches[0];
            let scan_secs = scan_start.elapsed().as_secs_f64();
            tracing::info!(
                "async detection from step {}/{}: {} match(es), best pixel ({}, {}) score={:.4}",
                det.step_index + 1, total, det.matches.len(), m.x, m.y, m.score
            );
            match confirm_match(game, state, kingdom, m.x, m.y, det.nav_x, det.nav_y, m.score, Some(scan_secs), config, ref_images).await {
                Ok(true) => {
                    let elapsed = scan_start.elapsed();
                    tracing::info!("kingdom {kingdom} scan completed in {elapsed:.1?} (confirmed at step {}/{})", det.step_index + 1, total);
                    return Ok(());
                }
                Ok(false) => {
                    tracing::info!("match not confirmed at step {}/{}, resuming scan", det.step_index + 1, total);
                    // Drain any stale detections
                    while rx.try_recv().is_ok() {}
                }
                Err(e) => {
                    tracing::warn!("failed to confirm match at pixel ({}, {}): {e:#}", m.x, m.y);
                    while rx.try_recv().is_ok() {}
                }
            }
        }

        if !check_should_continue(state).await {
            return Ok(());
        }

        tracing::info!("step {}/{}: goto ({gx}, {gy})", i + 1, total);
        game.navigate_to_coords(kingdom, gx, gy).await?;

        // Take screenshot
        let screenshot_bytes = game
            .take_screenshot()
            .await
            .context("failed to take screenshot")?;

        if config.debug_screenshots {
            let scan_path = format!("debug_scan_k{kingdom}_s{:03}.png", i + 1);
            if let Err(e) = tokio::fs::write(&scan_path, &screenshot_bytes).await {
                tracing::warn!("failed to save {scan_path}: {e}");
            }
        }

        // Spawn detection in background (CPU-bound work overlaps with next navigation)
        let refs = ref_images.clone();
        let tx = tx.clone();
        tokio::task::spawn_blocking(move || {
            let screenshot = match image::load_from_memory(&screenshot_bytes) {
                Ok(img) => img,
                Err(e) => {
                    tracing::warn!("failed to decode screenshot in background: {e}");
                    return;
                }
            };

            let matches = match detector::find_matches(&screenshot, &refs) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("template matching failed in background: {e}");
                    return;
                }
            };

            if matches.is_empty() {
                tracing::info!("step {}/{total}: no matches (async)", i + 1);
                return;
            }

            tracing::info!(
                "step {}/{total}: found {} match(es) (async)",
                i + 1, matches.len()
            );

            let _ = tx.send(DetectionResult {
                matches,
                nav_x: gx,
                nav_y: gy,
                step_index: i,
            });
        });
    }

    // After loop: wait for final detection result
    drop(tx); // close sender so recv terminates
    if let Some(det) = rx.recv().await {
        let m = &det.matches[0];
        let scan_secs = scan_start.elapsed().as_secs_f64();
        tracing::info!(
            "final async detection from step {}/{}: best pixel ({}, {}) score={:.4}",
            det.step_index + 1, total, m.x, m.y, m.score
        );
        match confirm_match(game, state, kingdom, m.x, m.y, det.nav_x, det.nav_y, m.score, Some(scan_secs), config, ref_images).await {
            Ok(true) => {
                let elapsed = scan_start.elapsed();
                tracing::info!("kingdom {kingdom} scan completed in {elapsed:.1?} (confirmed at step {}/{})", det.step_index + 1, total);
                return Ok(());
            }
            Ok(false) => {
                tracing::info!("final match not confirmed at step {}/{}", det.step_index + 1, total);
            }
            Err(e) => {
                tracing::warn!("failed to confirm final match: {e:#}");
            }
        }
    }

    let elapsed = scan_start.elapsed();
    tracing::info!("kingdom {kingdom} scan completed in {elapsed:.1?} (no match found)");
    Ok(())
}

/// Screen center pixel coordinates (where navigated game coords appear).
/// Measured from the yellow crosshair square after goto in the 1920×1080
/// headless viewport.  The minimap, top bar, bottom toolbar and right-side
/// icons shift the center well away from (960, 540).
pub const SCREEN_CENTER_X: f64 = 760.0;
pub const SCREEN_CENTER_Y: f64 = 400.0;

/// Calibrated pixel-to-game-coordinate transform (25% zoom).
/// Forward: pixel_dx = PX_PER_GAME_X * game_dx
///          pixel_dy = TILT_Y * game_dx + PX_PER_GAME_Y * game_dy
/// Calibrated from K:111 buildings at (502,512) and (528,524).
const PX_PER_GAME_X: f64 = 49.40;
const PX_PER_GAME_Y: f64 = 28.32;
const TILT_Y: f64 = -1.50; // vertical pixel shift per game X unit

/// Convert a pixel offset from screen center to approximate game coordinate offset.
/// Returns (delta_x, delta_y) in game coordinate units.
pub fn pixel_to_game_offset(pixel_x: u32, pixel_y: u32) -> (i32, i32) {
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
    initial_score: f32,
    scan_duration_secs: Option<f64>,
    config: &Config,
    ref_images: &[PreparedRef],
) -> Result<bool> {
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

    if config.debug_screenshots {
        let goto_path = format!("debug_goto_k{kingdom}_{est_x}_{est_y}.png");
        if let Err(e) = tokio::fs::write(&goto_path, &goto_bytes).await {
            tracing::warn!("failed to save {goto_path}: {e}");
        } else {
            tracing::info!("saved goto screenshot: {goto_path}");
        }
    }

    // Calibration: re-run template matching on goto screenshot to refine position
    let goto_img = image::load_from_memory(&goto_bytes)
        .context("failed to decode goto screenshot")?;
    let calibration = detector::find_best_match(&goto_img, ref_images);

    // Refine coordinates using calibration offset (accounts for sprite height)
    let (refined_x, refined_y, click_x, click_y) = if let Some(ref gm) = calibration {
        let err_x = gm.x as f64 - SCREEN_CENTER_X;
        let err_y = gm.y as f64 - SCREEN_CENTER_Y;
        tracing::info!(
            "CALIBRATION: building at pixel ({}, {}), score={:.4}, error from center: ({err_x:.0}, {err_y:.0})",
            gm.x, gm.y, gm.score
        );

        // The calibration error tells us how far the building is from where we
        // expected it. Convert that pixel offset to game coordinate correction.
        let (corr_dx, corr_dy) = pixel_to_game_offset(gm.x, gm.y);
        let rx = (est_x as i32 + corr_dx).clamp(0, 1023) as u32;
        let ry = (est_y as i32 + corr_dy).clamp(0, 1023) as u32;
        tracing::info!("refined coords: K:{kingdom} X:{rx} Y:{ry} (correction: {corr_dx}, {corr_dy})");

        (rx, ry, gm.x as f64, gm.y as f64)
    } else {
        tracing::info!("CALIBRATION: no match in goto screenshot, using estimate");
        (est_x, est_y, SCREEN_CENTER_X, SCREEN_CENTER_Y)
    };

    let cal_score = calibration.as_ref().map(|gm| gm.score);

    // Step 4: Click at the detected building position
    tracing::info!("clicking at ({click_x:.0}, {click_y:.0})");
    game.click_at_cdp_full(click_x, click_y).await?;
    sleep(Duration::from_secs(2)).await;

    // Step 5: Screenshot the popup
    let popup_bytes = game
        .take_screenshot()
        .await
        .context("failed to take popup screenshot")?;

    if config.debug_screenshots {
        let popup_path = format!("debug_popup_k{kingdom}_{refined_x}_{refined_y}.png");
        if let Err(e) = tokio::fs::write(&popup_path, &popup_bytes).await {
            tracing::warn!("failed to save {popup_path}: {e}");
        } else {
            tracing::info!("saved popup screenshot: {popup_path}");
        }
    }

    // Try to read popup text via DOM
    let popup_text = game.read_popup_text().await?;
    tracing::info!("popup text result: {:?}", popup_text);

    let screenshot = Some(popup_bytes.to_vec());

    let confirmed = if let Some(ref text) = popup_text {
        if let Some((k, x, y)) = browser::parse_popup_coords(text) {
            tracing::info!("found coordinates in popup: K:{k} X:{x} Y:{y}");

            let exchange = MercExchange {
                kingdom: k,
                x,
                y,
                found_at: Utc::now(),
                scan_duration_secs,
                confirmed: true,
                screenshot_png: screenshot,
            };

            let mut s = state.lock().await;
            let stored = s.add_exchange(exchange);
            if stored {
                tracing::info!("added exchange K:{k} X:{x} Y:{y} confirmed (total: {})", s.exchanges.len());
            } else {
                tracing::debug!("duplicate or full, skipping K:{k} X:{x} Y:{y}");
            }
            drop(s);

            log_exchange(config, &ExchangeLogEntry {
                timestamp: Utc::now().to_rfc3339(),
                kingdom: k,
                x,
                y,
                confirmed: true,
                stored,
                initial_score,
                calibration_score: cal_score,
                scan_pattern: config.scan_pattern.clone(),
                scan_duration_secs,
            });

            true
        } else {
            tracing::info!("popup text has no coords, not confirmed: {text}");

            log_exchange(config, &ExchangeLogEntry {
                timestamp: Utc::now().to_rfc3339(),
                kingdom,
                x: refined_x,
                y: refined_y,
                confirmed: false,
                stored: false,
                initial_score,
                calibration_score: cal_score,
                scan_pattern: config.scan_pattern.clone(),
                scan_duration_secs,
            });

            false
        }
    } else {
        // No popup text — check if calibration was strong and near center
        let cal_confirmed = calibration.as_ref().is_some_and(|gm| {
            let err_x = (gm.x as f64 - SCREEN_CENTER_X).abs();
            let err_y = (gm.y as f64 - SCREEN_CENTER_Y).abs();
            gm.score >= 0.90 && err_x < 80.0 && err_y < 80.0
        });

        if cal_confirmed {
            tracing::info!("no popup but strong calibration match, storing refined estimate");
            let exchange = MercExchange {
                kingdom,
                x: refined_x,
                y: refined_y,
                found_at: Utc::now(),
                scan_duration_secs,
                confirmed: false,
                screenshot_png: screenshot,
            };

            let mut s = state.lock().await;
            let stored = s.add_exchange(exchange);
            if stored {
                tracing::info!("added exchange K:{kingdom} X:{refined_x} Y:{refined_y} (estimate, total: {})", s.exchanges.len());
            } else {
                tracing::debug!("duplicate or full, skipping K:{kingdom} X:{refined_x} Y:{refined_y}");
            }
            drop(s);

            log_exchange(config, &ExchangeLogEntry {
                timestamp: Utc::now().to_rfc3339(),
                kingdom,
                x: refined_x,
                y: refined_y,
                confirmed: false,
                stored,
                initial_score,
                calibration_score: cal_score,
                scan_pattern: config.scan_pattern.clone(),
                scan_duration_secs,
            });

            true
        } else {
            tracing::info!("no popup and weak/no calibration, not confirmed");

            log_exchange(config, &ExchangeLogEntry {
                timestamp: Utc::now().to_rfc3339(),
                kingdom,
                x: refined_x,
                y: refined_y,
                confirmed: false,
                stored: false,
                initial_score,
                calibration_score: cal_score,
                scan_pattern: config.scan_pattern.clone(),
                scan_duration_secs,
            });

            false
        }
    };

    // Close popup
    game.send_canvas_escape().await;
    sleep(Duration::from_millis(500)).await;

    Ok(confirmed)
}

/// Generate 9 interleaved spirals in a 3×3 grid covering the full map.
/// Interleaves by ring level so broad coverage comes first.
fn multi_spiral_positions(step: u32, max_rings: u32) -> Vec<(u32, u32)> {
    let centers: [(u32, u32); 9] = [
        (512, 512), // center
        (150, 150), // NW
        (874, 150), // NE
        (150, 874), // SW
        (874, 874), // SE
        (512, 150), // N
        (150, 512), // W
        (874, 512), // E
        (512, 874), // S
    ];

    // Generate per-center spiral positions grouped by ring
    let per_center: Vec<Vec<Vec<(u32, u32)>>> = centers
        .iter()
        .map(|&(cx, cy)| {
            let mut rings = Vec::new();
            // Ring 0 = center point
            rings.push(vec![(cx, cy)]);
            for r in 1..=max_rings {
                rings.push(spiral_ring_positions(cx, cy, step, r));
            }
            rings
        })
        .collect();

    let mut seen = HashSet::new();
    let mut positions = Vec::new();

    // Interleave: for each ring level, emit all 9 centers' ring
    for ring in 0..=max_rings as usize {
        for center_rings in &per_center {
            if ring < center_rings.len() {
                for &pos in &center_rings[ring] {
                    if seen.insert(pos) {
                        positions.push(pos);
                    }
                }
            }
        }
    }

    positions
}

/// Single spiral at center with step=50 for wider coverage.
fn wide_spiral_positions(max_rings: u32) -> Vec<(u32, u32)> {
    spiral_scan_positions(512, 512, 50, max_rings)
}

/// Read known exchange locations from a CSV file (k,x,y per line) and generate
/// interleaved spirals around each. Falls back to grid_scan_positions if the
/// file is missing or empty.
fn known_spiral_positions(
    file_path: Option<&str>,
    step: u32,
    max_rings: u32,
) -> Vec<(u32, u32)> {
    let centers = match file_path {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(contents) => parse_known_locations(&contents),
            Err(e) => {
                tracing::warn!("failed to read known locations file {path}: {e}, falling back to grid");
                return grid_scan_positions();
            }
        },
        None => {
            tracing::warn!("no known locations file configured, falling back to grid");
            return grid_scan_positions();
        }
    };

    if centers.is_empty() {
        tracing::warn!("known locations file is empty, falling back to grid");
        return grid_scan_positions();
    }

    tracing::info!("loaded {} known locations, generating spirals with {max_rings} ring(s)", centers.len());

    // Generate per-center spiral positions grouped by ring
    let per_center: Vec<Vec<Vec<(u32, u32)>>> = centers
        .iter()
        .map(|&(cx, cy)| {
            let mut rings = Vec::new();
            rings.push(vec![(cx, cy)]);
            for r in 1..=max_rings {
                rings.push(spiral_ring_positions(cx, cy, step, r));
            }
            rings
        })
        .collect();

    let mut seen = HashSet::new();
    let mut positions = Vec::new();

    // Interleave by ring level: all centers' ring 0, then ring 1, etc.
    for ring in 0..=max_rings as usize {
        for center_rings in &per_center {
            if ring < center_rings.len() {
                for &pos in &center_rings[ring] {
                    if seen.insert(pos) {
                        positions.push(pos);
                    }
                }
            }
        }
    }

    positions
}

/// Parse known locations from CSV content (k,x,y per line).
/// The kingdom column is stored but currently ignored — all locations are visited.
/// Deduplicates on (x,y) preserving file order.
fn parse_known_locations(contents: &str) -> Vec<(u32, u32)> {
    let mut seen = HashSet::new();
    let mut centers = Vec::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(',').collect();
        // k,x,y (3 columns) or legacy x,y (2 columns)
        let (xi, yi) = match parts.len() {
            3 => (1, 2),
            2 => (0, 1),
            _ => {
                tracing::warn!("skipping invalid line: {line}");
                continue;
            }
        };
        let (x, y) = match (parts[xi].trim().parse::<u32>(), parts[yi].trim().parse::<u32>()) {
            (Ok(x), Ok(y)) => (x, y),
            _ => {
                tracing::warn!("skipping invalid line: {line}");
                continue;
            }
        };
        if seen.insert((x, y)) {
            centers.push((x, y));
        }
    }

    centers
}

/// Regular grid across the full map (30–970, step=30).
fn grid_scan_positions() -> Vec<(u32, u32)> {
    let step = 30u32;
    let mut positions = Vec::new();
    let mut y = 30;
    while y <= 970 {
        let mut x = 30;
        while x <= 970 {
            positions.push((x, y));
            x += step;
        }
        y += step;
    }
    positions
}

/// Generate positions for a single ring of a spiral (not including center).
fn spiral_ring_positions(cx: u32, cy: u32, step: u32, ring: u32) -> Vec<(u32, u32)> {
    let s = step as i32;
    let cx = cx as i32;
    let cy = cy as i32;
    let r = ring as i32;
    let mut positions = Vec::new();

    // Right edge (top to bottom)
    for j in -r..=r {
        push_clamped(&mut positions, cx + r * s, cy + j * s);
    }
    // Bottom edge (right to left, skip corner)
    for i in (-r..r).rev() {
        push_clamped(&mut positions, cx + i * s, cy + r * s);
    }
    // Left edge (bottom to top, skip corner)
    for j in (-r..r).rev() {
        push_clamped(&mut positions, cx - r * s, cy + j * s);
    }
    // Top edge (left to right, skip both corners)
    for i in (-r + 1)..r {
        push_clamped(&mut positions, cx + i * s, cy - r * s);
    }

    positions
}

/// Generate absolute game coordinates in spiral order around (cx, cy).
/// Returns Vec of (x, y) positions clamped to [0, 1023].
fn spiral_scan_positions(cx: u32, cy: u32, step: u32, max_rings: u32) -> Vec<(u32, u32)> {
    let s = step as i32;
    let cx = cx as i32;
    let cy = cy as i32;
    let mut positions = vec![(cx.clamp(0, 1023) as u32, cy.clamp(0, 1023) as u32)];

    for r in 1..=max_rings as i32 {
        // Right edge (top to bottom)
        for j in -r..=r {
            push_clamped(&mut positions, cx + r * s, cy + j * s);
        }
        // Bottom edge (right to left, skip corner)
        for i in (-r..r).rev() {
            push_clamped(&mut positions, cx + i * s, cy + r * s);
        }
        // Left edge (bottom to top, skip corner)
        for j in (-r..r).rev() {
            push_clamped(&mut positions, cx - r * s, cy + j * s);
        }
        // Top edge (left to right, skip both corners)
        for i in (-r + 1)..r {
            push_clamped(&mut positions, cx + i * s, cy - r * s);
        }
    }

    positions
}

fn push_clamped(positions: &mut Vec<(u32, u32)>, x: i32, y: i32) {
    positions.push((x.clamp(0, 1023) as u32, y.clamp(0, 1023) as u32));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spiral_scan_positions_center_first() {
        let positions = spiral_scan_positions(512, 512, 25, 1);
        assert_eq!(positions[0], (512, 512));
    }

    #[test]
    fn test_spiral_scan_positions_count() {
        // 1 center + 8r per ring: 1 + 8 + 16 + 24 + 32 = 81
        let positions = spiral_scan_positions(512, 512, 25, 4);
        assert_eq!(positions.len(), 81);
    }

    #[test]
    fn test_spiral_scan_positions_within_bounds() {
        let positions = spiral_scan_positions(512, 512, 25, 4);
        for &(x, y) in &positions {
            assert!(x <= 1023, "x={x} out of bounds");
            assert!(y <= 1023, "y={y} out of bounds");
        }
    }

    #[test]
    fn test_spiral_scan_positions_no_duplicates() {
        let positions = spiral_scan_positions(512, 512, 25, 4);
        let mut seen = std::collections::HashSet::new();
        for &pos in &positions {
            assert!(seen.insert(pos), "duplicate position: {pos:?}");
        }
    }

    #[test]
    fn test_spiral_scan_positions_clamped_near_edge() {
        // Center near corner — positions should clamp to [0, 1023]
        let positions = spiral_scan_positions(10, 10, 25, 2);
        for &(x, y) in &positions {
            assert!(x <= 1023, "x={x} out of bounds");
            assert!(y <= 1023, "y={y} out of bounds");
        }
    }

    // --- Multi-spiral tests ---

    #[test]
    fn test_multi_spiral_no_duplicates() {
        let positions = multi_spiral_positions(25, 4);
        let mut seen = std::collections::HashSet::new();
        for &pos in &positions {
            assert!(seen.insert(pos), "duplicate position: {pos:?}");
        }
    }

    #[test]
    fn test_multi_spiral_within_bounds() {
        let positions = multi_spiral_positions(25, 4);
        for &(x, y) in &positions {
            assert!(x <= 1023, "x={x} out of bounds");
            assert!(y <= 1023, "y={y} out of bounds");
        }
    }

    #[test]
    fn test_multi_spiral_starts_with_centers() {
        let positions = multi_spiral_positions(25, 4);
        // First 9 positions should be the 9 center points (ring 0 of each)
        assert_eq!(positions[0], (512, 512));
        assert_eq!(positions[1], (150, 150));
        assert_eq!(positions[2], (874, 150));
        assert_eq!(positions[3], (150, 874));
        assert_eq!(positions[4], (874, 874));
        assert_eq!(positions[5], (512, 150));
        assert_eq!(positions[6], (150, 512));
        assert_eq!(positions[7], (874, 512));
        assert_eq!(positions[8], (512, 874));
    }

    #[test]
    fn test_multi_spiral_count() {
        // 9 spirals × (1 + 8 + 16 + 24 + 32) = 9 × 81 = 729, minus deduplication
        let positions = multi_spiral_positions(25, 4);
        assert!(
            positions.len() >= 600 && positions.len() <= 729,
            "expected ~650 positions, got {}",
            positions.len()
        );
    }

    #[test]
    fn test_multi_spiral_interleaving() {
        // After the first 9 (ring 0), the next batch should be ring 1 of all 9 centers
        let positions = multi_spiral_positions(25, 2);
        // Ring 0: 9 centers = positions[0..9]
        // Ring 1 center spiral starts at position[9], should be near (512,512)
        let ring1_start = positions[9];
        let dx = (ring1_start.0 as i32 - 512).abs();
        let dy = (ring1_start.1 as i32 - 512).abs();
        assert!(dx <= 25 && dy <= 25, "ring 1 should start near center spiral: {ring1_start:?}");
    }

    // --- Wide spiral tests ---

    #[test]
    fn test_wide_spiral_count() {
        // step=50, 5 rings: 1 + 8 + 16 + 24 + 32 + 40 = 121
        let positions = wide_spiral_positions(5);
        assert_eq!(positions.len(), 121);
    }

    #[test]
    fn test_wide_spiral_within_bounds() {
        let positions = wide_spiral_positions(5);
        for &(x, y) in &positions {
            assert!(x <= 1023, "x={x} out of bounds");
            assert!(y <= 1023, "y={y} out of bounds");
        }
    }

    #[test]
    fn test_wide_spiral_no_duplicates() {
        let positions = wide_spiral_positions(5);
        let mut seen = std::collections::HashSet::new();
        for &pos in &positions {
            assert!(seen.insert(pos), "duplicate position: {pos:?}");
        }
    }

    // --- Grid tests ---

    #[test]
    fn test_grid_within_bounds() {
        let positions = grid_scan_positions();
        for &(x, y) in &positions {
            assert!(x >= 30 && x <= 970, "x={x} out of expected range");
            assert!(y >= 30 && y <= 970, "y={y} out of expected range");
        }
    }

    #[test]
    fn test_grid_no_duplicates() {
        let positions = grid_scan_positions();
        let mut seen = std::collections::HashSet::new();
        for &pos in &positions {
            assert!(seen.insert(pos), "duplicate position: {pos:?}");
        }
    }

    #[test]
    fn test_grid_count() {
        // (970 - 30) / 30 + 1 = 32.33 → 32 per axis → 32 × 32 = 1024
        let positions = grid_scan_positions();
        assert_eq!(positions.len(), 1024);
    }

    #[test]
    fn test_grid_uniform_spacing() {
        let positions = grid_scan_positions();
        // Check first row has uniform X spacing of 30
        let first_y = positions[0].1;
        let first_row: Vec<u32> = positions.iter()
            .filter(|p| p.1 == first_y)
            .map(|p| p.0)
            .collect();
        for w in first_row.windows(2) {
            assert_eq!(w[1] - w[0], 30, "expected uniform step of 30");
        }
    }

    // --- Known spiral tests ---

    #[test]
    fn test_known_spiral_with_locations() {
        let dir = std::env::temp_dir();
        let path = dir.join("mercy_test_known.csv");
        std::fs::write(&path, "111,100,200\n112,800,900\n111,100,200\n").unwrap();

        let positions = known_spiral_positions(path.to_str(), 25, 1);

        // First two positions should be the two unique centers (ring 0 interleaved)
        assert_eq!(positions[0], (100, 200));
        assert_eq!(positions[1], (800, 900));

        // No duplicates
        let mut seen = std::collections::HashSet::new();
        for &pos in &positions {
            assert!(seen.insert(pos), "duplicate position: {pos:?}");
        }

        // 2 centers × (1 + 8) = 18, minus any dedup from overlap
        assert!(positions.len() >= 10 && positions.len() <= 18,
            "expected 10-18 positions, got {}", positions.len());

        // All within bounds
        for &(x, y) in &positions {
            assert!(x <= 1023, "x={x} out of bounds");
            assert!(y <= 1023, "y={y} out of bounds");
        }

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_known_spiral_fallback() {
        let positions = known_spiral_positions(Some("/nonexistent/path/mercy_test.csv"), 25, 1);

        // Should fall back to grid
        assert_eq!(positions.len(), grid_scan_positions().len());
    }

    #[test]
    fn test_known_spiral_none_fallback() {
        let positions = known_spiral_positions(None, 25, 1);
        assert_eq!(positions.len(), grid_scan_positions().len());
    }

    #[test]
    fn test_parse_known_locations_kxy() {
        let contents = "111,100,200\n112,800,900\n111,100,200\n";
        let locs = parse_known_locations(contents);
        assert_eq!(locs, vec![(100, 200), (800, 900)]);
    }

    #[test]
    fn test_parse_known_locations_legacy_xy() {
        let contents = "100,200\n800,900\n";
        let locs = parse_known_locations(contents);
        assert_eq!(locs, vec![(100, 200), (800, 900)]);
    }

    #[test]
    fn test_parse_known_locations_comments_and_blanks() {
        let contents = "# header\n\n111,100,200\n  \n112,800,900\n";
        let locs = parse_known_locations(contents);
        assert_eq!(locs, vec![(100, 200), (800, 900)]);
    }
}
