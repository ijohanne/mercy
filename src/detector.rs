use std::sync::Arc;

use anyhow::Result;
use image::{DynamicImage, GrayImage};
use image::imageops::FilterType;
use imageproc::template_matching::{match_template, MatchTemplateMethod};

/// A detected match position in the screenshot (pixel coordinates, at original scale).
#[derive(Debug, Clone)]
pub struct TemplateMatch {
    pub x: u32,
    pub y: u32,
    pub score: f32,
}

const MATCH_THRESHOLD: f32 = 0.98;

/// Downscale factor for template matching (1 = full size, most accurate).
const SCALE_DOWN: u32 = 1;

/// Game viewport bounds (excluding UI: minimap, top bar, bottom toolbar, right panel).
/// Matches found within the cropped region are offset back to full screenshot coordinates.
const VIEWPORT_LEFT: u32 = 160;
const VIEWPORT_TOP: u32 = 60;
const VIEWPORT_RIGHT: u32 = 1860;
const VIEWPORT_BOTTOM: u32 = 1000;

/// Find all locations in the screenshot that match any of the reference images
/// above the confidence threshold. Images are downscaled for performance.
/// Only searches within the game viewport area (excluding UI elements).
pub fn find_matches(
    screenshot: &DynamicImage,
    ref_images: &[Arc<DynamicImage>],
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
    let screenshot_gray = screenshot_small.to_luma8();

    let mut all_matches = Vec::new();

    for ref_img in ref_images {
        // Downscale reference image too
        let ref_small_w = ref_img.width() / SCALE_DOWN;
        let ref_small_h = ref_img.height() / SCALE_DOWN;

        // Skip tiny references after downscale
        if ref_small_w < 10 || ref_small_h < 10 {
            tracing::warn!("reference image too small after downscale, skipping");
            continue;
        }

        let ref_small = ref_img.resize_exact(ref_small_w, ref_small_h, FilterType::Triangle);
        let ref_gray = ref_small.to_luma8();

        // Skip if reference is larger than screenshot
        if ref_gray.width() >= screenshot_gray.width()
            || ref_gray.height() >= screenshot_gray.height()
        {
            tracing::warn!(
                "reference image {}x{} is too large for screenshot {}x{}, skipping",
                ref_gray.width(),
                ref_gray.height(),
                screenshot_gray.width(),
                screenshot_gray.height()
            );
            continue;
        }

        tracing::debug!(
            "matching {}x{} template against {}x{} screenshot",
            ref_gray.width(), ref_gray.height(),
            screenshot_gray.width(), screenshot_gray.height()
        );

        let matches = find_template_matches(&screenshot_gray, &ref_gray)?;

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

fn find_template_matches(
    screenshot: &GrayImage,
    template: &GrayImage,
) -> Result<Vec<TemplateMatch>> {
    let result = match_template(
        screenshot,
        template,
        MatchTemplateMethod::CrossCorrelationNormalized,
    );

    let mut matches = Vec::new();
    let mut best_score: f32 = 0.0;
    let (w, h) = result.dimensions();

    for y in 0..h {
        for x in 0..w {
            let score = result.get_pixel(x, y).0[0];
            if score > best_score {
                best_score = score;
            }
            if score >= MATCH_THRESHOLD {
                matches.push(TemplateMatch {
                    x: x + template.width() / 2,
                    y: y + template.height() / 2,
                    score,
                });
            }
        }
    }

    tracing::info!(
        "template {}x{}: best_score={:.4}, {} raw matches above {:.2}",
        template.width(), template.height(), best_score, matches.len(), MATCH_THRESHOLD
    );

    // Sort by score descending
    matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    Ok(matches)
}

/// Find the single best match regardless of threshold (for calibration).
pub fn find_best_match(
    screenshot: &DynamicImage,
    ref_images: &[Arc<DynamicImage>],
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
    let screenshot_gray = screenshot_small.to_luma8();

    let mut best: Option<TemplateMatch> = None;

    for ref_img in ref_images {
        let ref_small_w = ref_img.width() / SCALE_DOWN;
        let ref_small_h = ref_img.height() / SCALE_DOWN;
        if ref_small_w < 10 || ref_small_h < 10 {
            continue;
        }
        let ref_small = ref_img.resize_exact(ref_small_w, ref_small_h, FilterType::Triangle);
        let ref_gray = ref_small.to_luma8();
        if ref_gray.width() >= screenshot_gray.width()
            || ref_gray.height() >= screenshot_gray.height()
        {
            continue;
        }

        let result = match_template(
            &screenshot_gray,
            &ref_gray,
            MatchTemplateMethod::CrossCorrelationNormalized,
        );
        let (w, h) = result.dimensions();
        for y in 0..h {
            for x in 0..w {
                let score = result.get_pixel(x, y).0[0];
                let dominated = best.as_ref().is_some_and(|b| score <= b.score);
                if !dominated {
                    best = Some(TemplateMatch {
                        x: (x + ref_gray.width() / 2) * SCALE_DOWN + VIEWPORT_LEFT,
                        y: (y + ref_gray.height() / 2) * SCALE_DOWN + VIEWPORT_TOP,
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
pub fn load_reference_images() -> Result<Vec<Arc<DynamicImage>>> {
    let env_assets = std::env::var("MERCY_ASSETS_DIR").ok().map(std::path::PathBuf::from);

    let bin_share = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent()?.parent().map(|p| p.join("share/mercy")));

    let filenames = [
        "test_building_ref.png",
    ];

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
