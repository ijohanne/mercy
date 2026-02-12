use std::sync::Arc;

use anyhow::Result;
use image::{DynamicImage, GrayImage, RgbImage};
use image::imageops::FilterType;
use imageproc::gradients::sobel_gradients;
use imageproc::template_matching::{match_template, MatchTemplateMethod};

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
        if prepared.width >= screenshot_rgb.width()
            || prepared.height >= screenshot_rgb.height()
        {
            tracing::warn!(
                "reference image {}x{} is too large for screenshot {}x{}, skipping",
                prepared.width, prepared.height,
                screenshot_rgb.width(), screenshot_rgb.height()
            );
            continue;
        }

        tracing::debug!(
            "matching {}x{} template against {}x{} screenshot (RGBE 4-channel)",
            prepared.width, prepared.height,
            screenshot_rgb.width(), screenshot_rgb.height()
        );

        let matches = find_template_matches_rgbe(&screenshot_channels, &screenshot_edge, &prepared.channels, &prepared.edge, prepared.width, prepared.height)?;

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

/// Run template matching on 4 channels (R, G, B, Edge) and take the minimum
/// score at each position. This ensures ALL channels must match well —
/// a structurally similar but differently colored building will fail,
/// and a color-similar but structurally different building will also fail.
fn find_template_matches_rgbe(
    screenshot_channels: &[GrayImage; 3],
    screenshot_edge: &GrayImage,
    template_channels: &[GrayImage; 3],
    template_edge: &GrayImage,
    template_w: u32,
    template_h: u32,
) -> Result<Vec<TemplateMatch>> {
    // Run matching on R, G, B channels
    let mut results: Vec<_> = (0..3)
        .map(|ch| {
            match_template(
                &screenshot_channels[ch],
                &template_channels[ch],
                MatchTemplateMethod::CrossCorrelationNormalized,
            )
        })
        .collect();

    // Run matching on edge channel
    results.push(match_template(
        screenshot_edge,
        template_edge,
        MatchTemplateMethod::CrossCorrelationNormalized,
    ));

    let (w, h) = results[0].dimensions();

    let mut matches = Vec::new();
    let mut best_score: f32 = 0.0;

    for y in 0..h {
        for x in 0..w {
            // Minimum across all 4 channels: ALL must match
            let score = (0..4)
                .map(|ch| results[ch].get_pixel(x, y).0[0])
                .fold(f32::INFINITY, f32::min);

            if score > best_score {
                best_score = score;
            }
            if score >= MATCH_THRESHOLD {
                matches.push(TemplateMatch {
                    x: x + template_w / 2,
                    y: y + template_h / 2,
                    score,
                });
            }
        }
    }

    tracing::info!(
        "template {}x{}: best_score={:.4}, {} raw matches above {:.2} (RGBE 4-channel)",
        template_w, template_h, best_score, matches.len(), MATCH_THRESHOLD
    );

    // Sort by score descending
    matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    Ok(matches)
}

/// Find the single best match regardless of threshold (for calibration).
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
        if prepared.width >= screenshot_rgb.width()
            || prepared.height >= screenshot_rgb.height()
        {
            continue;
        }

        // Run matching on R, G, B channels
        let mut results: Vec<_> = (0..3)
            .map(|ch| {
                match_template(
                    &screenshot_channels[ch],
                    &prepared.channels[ch],
                    MatchTemplateMethod::CrossCorrelationNormalized,
                )
            })
            .collect();

        // Run matching on edge channel
        results.push(match_template(
            &screenshot_edge,
            &prepared.edge,
            MatchTemplateMethod::CrossCorrelationNormalized,
        ));

        let (w, h) = results[0].dimensions();
        for y in 0..h {
            for x in 0..w {
                let score = (0..4)
                    .map(|ch| results[ch].get_pixel(x, y).0[0])
                    .fold(f32::INFINITY, f32::min);

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
    matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

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
    let env_assets = std::env::var("MERCY_ASSETS_DIR").ok().map(std::path::PathBuf::from);

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
