use std::sync::Arc;

use anyhow::Result;
use image::imageops::FilterType;
use image::{DynamicImage, GrayImage, RgbImage};
use imageproc::gradients::sobel_gradients;
use imageproc::template_matching::{MatchTemplateMethod, match_template};

/// A detected match position in the screenshot (pixel coordinates, at original scale).
#[derive(Debug, Clone)]
pub struct TemplateMatch {
    pub x: u32,
    pub y: u32,
    pub score: f32,
}

/// Pre-computed reference image for template matching.
/// Stores per-channel grayscale images for color-aware matching,
/// plus a Sobel edge channel for structural matching.
pub struct PreparedRef {
    pub channels: [GrayImage; 3], // R, G, B
    pub edge: GrayImage,          // Sobel edge channel
    pub width: u32,
    pub height: u32,
}

pub const MATCH_THRESHOLD: f32 = 0.98;

/// Downscale factor for template matching (1 = full size, most accurate).
/// Using 1 (no downscale) because the reference images are small (~48x36)
/// and downscaling them further loses too much detail for reliable matching.
const SCALE_DOWN: u32 = 1;

/// Game viewport bounds (excluding UI: minimap, top bar, bottom toolbar, right panel).
/// Matches found within the cropped region are offset back to full screenshot coordinates.
const VIEWPORT_LEFT: u32 = 160;
const VIEWPORT_TOP: u32 = 60;
const VIEWPORT_RIGHT: u32 = 1860;
const VIEWPORT_BOTTOM: u32 = 1000;

/// Split an RGB image into 3 separate grayscale images (one per channel).
fn split_channels(rgb: &RgbImage) -> [GrayImage; 3] {
    let (w, h) = rgb.dimensions();
    let mut r = GrayImage::new(w, h);
    let mut g = GrayImage::new(w, h);
    let mut b = GrayImage::new(w, h);
    for (x, y, pixel) in rgb.enumerate_pixels() {
        r.put_pixel(x, y, image::Luma([pixel[0]]));
        g.put_pixel(x, y, image::Luma([pixel[1]]));
        b.put_pixel(x, y, image::Luma([pixel[2]]));
    }
    [r, g, b]
}

/// Compute Sobel edge magnitude image, normalized to u8.
fn compute_edges(gray: &GrayImage) -> GrayImage {
    let grad = sobel_gradients(gray);
    let (w, h) = grad.dimensions();
    let mut edges = GrayImage::new(w, h);
    // Find max for normalization
    let max_val = grad.pixels().map(|p| p.0[0]).max().unwrap_or(1).max(1);
    for (x, y, pixel) in grad.enumerate_pixels() {
        let normalized = (pixel.0[0] as f32 / max_val as f32 * 255.0) as u8;
        edges.put_pixel(x, y, image::Luma([normalized]));
    }
    edges
}

/// Pre-compute reference images for matching.
/// Call once at startup; the results are reused for every scan step.
pub fn prepare_reference_images(ref_images: &[Arc<DynamicImage>]) -> Vec<PreparedRef> {
    ref_images
        .iter()
        .filter_map(|img| {
            let ref_small_w = img.width() / SCALE_DOWN;
            let ref_small_h = img.height() / SCALE_DOWN;

            if ref_small_w < 10 || ref_small_h < 10 {
                tracing::warn!("reference image too small after downscale, skipping");
                return None;
            }

            let ref_small = img.resize_exact(ref_small_w, ref_small_h, FilterType::Triangle);
            let rgb = ref_small.to_rgb8();
            let channels = split_channels(&rgb);
            let gray = ref_small.to_luma8();
            let edge = compute_edges(&gray);
            Some(PreparedRef {
                width: ref_small_w,
                height: ref_small_h,
                channels,
                edge,
            })
        })
        .collect()
}

/// Find all locations in the screenshot that match any of the reference images
/// above the confidence threshold.
/// Only searches within the game viewport area (excluding UI elements).
pub fn find_matches(
    screenshot: &DynamicImage,
    ref_images: &[PreparedRef],
) -> Result<Vec<TemplateMatch>> {
    // Crop to game viewport to avoid matching on minimap/UI icons
    let viewport = screenshot.crop_imm(
        VIEWPORT_LEFT,
        VIEWPORT_TOP,
        VIEWPORT_RIGHT - VIEWPORT_LEFT,
        VIEWPORT_BOTTOM - VIEWPORT_TOP,
    );

    // Downscale for faster matching
    let small_w = viewport.width() / SCALE_DOWN;
    let small_h = viewport.height() / SCALE_DOWN;
    let screenshot_small = viewport.resize_exact(small_w, small_h, FilterType::Triangle);
    let screenshot_rgb = screenshot_small.to_rgb8();
    let screenshot_channels = split_channels(&screenshot_rgb);
    let screenshot_gray = screenshot_small.to_luma8();
    let screenshot_edge = compute_edges(&screenshot_gray);

    let mut all_matches = Vec::new();

    for prepared in ref_images {
        // Skip if reference is larger than screenshot
        if prepared.width >= screenshot_rgb.width() || prepared.height >= screenshot_rgb.height() {
            tracing::warn!(
                "reference image {}x{} is too large for screenshot {}x{}, skipping",
                prepared.width,
                prepared.height,
                screenshot_rgb.width(),
                screenshot_rgb.height()
            );
            continue;
        }

        tracing::debug!(
            "matching {}x{} template against {}x{} screenshot (RGBE 4-channel)",
            prepared.width,
            prepared.height,
            screenshot_rgb.width(),
            screenshot_rgb.height()
        );

        let matches = find_template_matches_rgbe(
            &screenshot_channels,
            &screenshot_edge,
            &prepared.channels,
            &prepared.edge,
            prepared.width,
            prepared.height,
        )?;

        // Scale match coordinates back to original size and offset to full screenshot
        let scaled: Vec<TemplateMatch> = matches
            .into_iter()
            .map(|m| TemplateMatch {
                x: m.x * SCALE_DOWN + VIEWPORT_LEFT,
                y: m.y * SCALE_DOWN + VIEWPORT_TOP,
                score: m.score,
            })
            .collect();

        all_matches.extend(scaled);
    }

    // Deduplicate nearby matches (within 40px at original scale)
    let deduped = deduplicate_matches(&mut all_matches, 40);

    Ok(deduped)
}

/// Run template matching on 4 channels (R, G, B, Edge) with cascading early exit.
/// Runs channels sequentially; if no pixel exceeds the threshold after a channel,
/// skips remaining channels (~4x speedup for the common "no match" case).
fn find_template_matches_rgbe(
    screenshot_channels: &[GrayImage; 3],
    screenshot_edge: &GrayImage,
    template_channels: &[GrayImage; 3],
    template_edge: &GrayImage,
    template_w: u32,
    template_h: u32,
) -> Result<Vec<TemplateMatch>> {
    let channel_names = ["R", "G", "B", "Edge"];

    // Channel 0: R — collect all candidates above threshold
    let r_result = match_template(
        &screenshot_channels[0],
        &template_channels[0],
        MatchTemplateMethod::CrossCorrelationNormalized,
    );
    let (w, h) = r_result.dimensions();

    let mut candidates: Vec<(u32, u32, f32)> = Vec::new();
    let mut best_score: f32 = 0.0;
    for y in 0..h {
        for x in 0..w {
            let score = r_result.get_pixel(x, y).0[0];
            if score > best_score {
                best_score = score;
            }
            if score >= MATCH_THRESHOLD {
                candidates.push((x, y, score));
            }
        }
    }

    if candidates.is_empty() {
        tracing::info!(
            "template {}x{}: early-exit after R (best={:.4}, 0 candidates)",
            template_w,
            template_h,
            best_score
        );
        return Ok(Vec::new());
    }

    tracing::info!(
        "template {}x{}: R pass: {} candidates (best={:.4})",
        template_w,
        template_h,
        candidates.len(),
        best_score
    );

    // Channels 1-3: G, B, Edge — filter candidates, early-exit if none survive
    let channel_sources: [(Option<usize>, bool); 3] = [
        (Some(1), false), // G channel
        (Some(2), false), // B channel
        (None, true),     // Edge channel
    ];

    for (ch_idx, &(rgb_idx, is_edge)) in channel_sources.iter().enumerate() {
        let result = if is_edge {
            match_template(
                screenshot_edge,
                template_edge,
                MatchTemplateMethod::CrossCorrelationNormalized,
            )
        } else {
            let i = rgb_idx.unwrap();
            match_template(
                &screenshot_channels[i],
                &template_channels[i],
                MatchTemplateMethod::CrossCorrelationNormalized,
            )
        };

        best_score = 0.0;
        for cand in &mut candidates {
            let ch_score = result.get_pixel(cand.0, cand.1).0[0];
            cand.2 = cand.2.min(ch_score); // min across all channels so far
            if cand.2 > best_score {
                best_score = cand.2;
            }
        }

        candidates.retain(|c| c.2 >= MATCH_THRESHOLD);

        let ch_name = channel_names[ch_idx + 1];
        if candidates.is_empty() {
            tracing::info!(
                "template {}x{}: early-exit after {} (best={:.4}, 0 candidates)",
                template_w,
                template_h,
                ch_name,
                best_score
            );
            return Ok(Vec::new());
        }

        tracing::info!(
            "template {}x{}: {} pass: {} candidates (best={:.4})",
            template_w,
            template_h,
            ch_name,
            candidates.len(),
            best_score
        );
    }

    let mut matches: Vec<TemplateMatch> = candidates
        .into_iter()
        .map(|(x, y, score)| TemplateMatch {
            x: x + template_w / 2,
            y: y + template_h / 2,
            score,
        })
        .collect();

    tracing::info!(
        "template {}x{}: best_score={:.4}, {} raw matches above {:.2} (RGBE 4-channel)",
        template_w,
        template_h,
        best_score,
        matches.len(),
        MATCH_THRESHOLD
    );

    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(matches)
}

/// Find the single best match regardless of threshold (for calibration).
/// Uses cascading channels: runs R first, tracks best position, then refines
/// with G/B/Edge. Skips remaining channels if best R score < threshold
/// (but still returns the best R-only score for diagnostic output).
pub fn find_best_match(
    screenshot: &DynamicImage,
    ref_images: &[PreparedRef],
) -> Option<TemplateMatch> {
    let viewport = screenshot.crop_imm(
        VIEWPORT_LEFT,
        VIEWPORT_TOP,
        VIEWPORT_RIGHT - VIEWPORT_LEFT,
        VIEWPORT_BOTTOM - VIEWPORT_TOP,
    );

    let small_w = viewport.width() / SCALE_DOWN;
    let small_h = viewport.height() / SCALE_DOWN;
    let screenshot_small = viewport.resize_exact(small_w, small_h, FilterType::Triangle);
    let screenshot_rgb = screenshot_small.to_rgb8();
    let screenshot_channels = split_channels(&screenshot_rgb);
    let screenshot_gray = screenshot_small.to_luma8();
    let screenshot_edge = compute_edges(&screenshot_gray);

    let mut best: Option<TemplateMatch> = None;

    for prepared in ref_images {
        if prepared.width >= screenshot_rgb.width() || prepared.height >= screenshot_rgb.height() {
            continue;
        }

        // Channel 0: R — find best position
        let r_result = match_template(
            &screenshot_channels[0],
            &prepared.channels[0],
            MatchTemplateMethod::CrossCorrelationNormalized,
        );
        let (w, h) = r_result.dimensions();

        let mut best_r_x = 0u32;
        let mut best_r_y = 0u32;
        let mut best_r_score: f32 = f32::NEG_INFINITY;
        for y in 0..h {
            for x in 0..w {
                let score = r_result.get_pixel(x, y).0[0];
                if score > best_r_score {
                    best_r_score = score;
                    best_r_x = x;
                    best_r_y = y;
                }
            }
        }

        if best_r_score < MATCH_THRESHOLD {
            // No point running more channels; return R-only score for diagnostics
            tracing::info!(
                "find_best_match: early-exit after R (best={:.4})",
                best_r_score
            );
            let dominated = best.as_ref().is_some_and(|b| best_r_score <= b.score);
            if !dominated {
                best = Some(TemplateMatch {
                    x: (best_r_x + prepared.width / 2) * SCALE_DOWN + VIEWPORT_LEFT,
                    y: (best_r_y + prepared.height / 2) * SCALE_DOWN + VIEWPORT_TOP,
                    score: best_r_score,
                });
            }
            continue;
        }

        // Remaining channels: G, B, Edge — full scan, min across all
        let g_result = match_template(
            &screenshot_channels[1],
            &prepared.channels[1],
            MatchTemplateMethod::CrossCorrelationNormalized,
        );
        let b_result = match_template(
            &screenshot_channels[2],
            &prepared.channels[2],
            MatchTemplateMethod::CrossCorrelationNormalized,
        );
        let e_result = match_template(
            &screenshot_edge,
            &prepared.edge,
            MatchTemplateMethod::CrossCorrelationNormalized,
        );

        for y in 0..h {
            for x in 0..w {
                let score = r_result.get_pixel(x, y).0[0]
                    .min(g_result.get_pixel(x, y).0[0])
                    .min(b_result.get_pixel(x, y).0[0])
                    .min(e_result.get_pixel(x, y).0[0]);

                let dominated = best.as_ref().is_some_and(|b| score <= b.score);
                if !dominated {
                    best = Some(TemplateMatch {
                        x: (x + prepared.width / 2) * SCALE_DOWN + VIEWPORT_LEFT,
                        y: (y + prepared.height / 2) * SCALE_DOWN + VIEWPORT_TOP,
                        score,
                    });
                }
            }
        }
    }

    best
}

fn deduplicate_matches(matches: &mut [TemplateMatch], min_distance: u32) -> Vec<TemplateMatch> {
    // Sort by score descending so we keep the best matches
    matches.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut result = Vec::new();

    for m in matches.iter() {
        let too_close = result.iter().any(|existing: &TemplateMatch| {
            let dx = m.x.abs_diff(existing.x);
            let dy = m.y.abs_diff(existing.y);
            dx < min_distance && dy < min_distance
        });

        if !too_close {
            result.push(m.clone());
        }
    }

    result
}

/// Load reference images from the assets directory.
/// Returns them as Arc<DynamicImage> for cheap sharing across scan iterations.
///
/// Search order for each image:
/// 1. `MERCY_ASSETS_DIR` env var (if set)
/// 2. Relative to CWD (e.g. `./assets/...`)
/// 3. Relative to the binary's `../share/mercy/` (Nix install layout)
pub fn load_reference_images(search_target: &str) -> Result<Vec<Arc<DynamicImage>>> {
    let env_assets = std::env::var("MERCY_ASSETS_DIR")
        .ok()
        .map(std::path::PathBuf::from);

    let bin_share = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent()?.parent().map(|p| p.join("share/mercy")));

    let base = search_target.to_lowercase().replace(' ', "_");
    let filenames = [format!("{base}_ref.png")];

    let mut images = Vec::new();

    for filename in &filenames {
        let asset_rel = std::path::Path::new("assets").join(filename);

        let candidates: Vec<std::path::PathBuf> = [
            env_assets.as_ref().map(|d| d.join(filename)),
            Some(asset_rel),
            bin_share.as_ref().map(|d| d.join("assets").join(filename)),
        ]
        .into_iter()
        .flatten()
        .collect();

        let mut loaded = false;
        for path in &candidates {
            if path.exists() {
                match image::open(path) {
                    Ok(img) => {
                        tracing::info!("loaded reference image: {}", path.display());
                        images.push(Arc::new(img));
                        loaded = true;
                        break;
                    }
                    Err(e) => {
                        tracing::warn!("failed to decode {}: {e}", path.display());
                    }
                }
            }
        }

        if !loaded {
            tracing::warn!("reference image {filename} not found in any search path");
        }
    }

    if images.is_empty() {
        anyhow::bail!("no reference images could be loaded");
    }

    Ok(images)
}
