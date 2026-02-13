use imageproc::template_matching::{MatchTemplateMethod, match_template};

fn main() {
    let screenshot = image::open("screenshot.png").expect("failed to open screenshot.png");
    println!("screenshot: {}x{}", screenshot.width(), screenshot.height());

    let ref_img =
        image::open("assets/test_building_ref.png").expect("failed to open test_building_ref.png");
    println!("reference: {}x{}", ref_img.width(), ref_img.height());

    // Crop to game viewport (matching detector.rs constants)
    let viewport = screenshot.crop_imm(160, 60, 1860 - 160, 1000 - 60);
    println!("viewport: {}x{}", viewport.width(), viewport.height());

    // No downscaling - use full resolution for accuracy
    let scale = 1u32;
    let ss_gray = viewport.to_luma8();
    let ref_gray = ref_img.to_luma8();

    println!(
        "downscaled: screenshot {}x{}, template {}x{}",
        ss_gray.width(),
        ss_gray.height(),
        ref_gray.width(),
        ref_gray.height()
    );

    // Test both methods
    for method in [
        (
            "CrossCorrelationNormalized",
            MatchTemplateMethod::CrossCorrelationNormalized,
        ),
        (
            "SumOfSquaredErrorsNormalized",
            MatchTemplateMethod::SumOfSquaredErrorsNormalized,
        ),
    ] {
        println!("\n=== {} ===", method.0);
        let result = match_template(&ss_gray, &ref_gray, method.1);
        let (w, h) = result.dimensions();

        let mut scores: Vec<f32> = Vec::new();
        for y in 0..h {
            for x in 0..w {
                scores.push(result.get_pixel(x, y).0[0]);
            }
        }
        scores.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let len = scores.len();
        println!(
            "  min={:.4} p5={:.4} p25={:.4} median={:.4} p75={:.4} p95={:.4} max={:.4}",
            scores[0],
            scores[len * 5 / 100],
            scores[len * 25 / 100],
            scores[len / 2],
            scores[len * 75 / 100],
            scores[len * 95 / 100],
            scores[len - 1],
        );

        // For NCC: count above various thresholds and show top positions
        if method.0.starts_with("Cross") {
            for thresh in [0.90, 0.95, 0.96, 0.97, 0.98, 0.99] {
                let count = scores.iter().filter(|&&s| s >= thresh).count();
                println!("  >= {:.2}: {} matches", thresh, count);
            }

            // Print top 5 match positions (in viewport coords, then offset to full screenshot)
            let mut positions: Vec<(u32, u32, f32)> = Vec::new();
            for y in 0..h {
                for x in 0..w {
                    let score = result.get_pixel(x, y).0[0];
                    if score >= 0.95 {
                        // Offset: scale back up and add viewport offset
                        let full_x = x * scale + ref_gray.width() / 2 * scale + 160;
                        let full_y = y * scale + ref_gray.height() / 2 * scale + 60;
                        positions.push((full_x, full_y, score));
                    }
                }
            }
            positions.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
            println!("  Top matches (full screenshot coords):");
            for (i, (x, y, s)) in positions.iter().take(10).enumerate() {
                println!("    #{}: ({}, {}) score={:.4}", i + 1, x, y, s);
            }
        } else {
            // For SSE: count below various thresholds (lower = better)
            for thresh in [0.01, 0.02, 0.05, 0.10, 0.15, 0.20] {
                let count = scores.iter().filter(|&&s| s <= thresh).count();
                println!("  <= {:.2}: {} matches", thresh, count);
            }
        }
    }
}
