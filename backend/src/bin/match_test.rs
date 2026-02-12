use std::sync::Arc;

use mercy::detector;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: match_test <reference.png> <screenshot.png> [screenshot2.png ...]");
        std::process::exit(1);
    }

    let ref_path = &args[1];
    let ref_img = Arc::new(image::open(ref_path).unwrap_or_else(|e| {
        eprintln!("Failed to load reference image {ref_path}: {e}");
        std::process::exit(1);
    }));
    println!(
        "Reference: {} ({}x{})",
        ref_path,
        ref_img.width(),
        ref_img.height()
    );

    let prepared = detector::prepare_reference_images(&[ref_img]);
    println!(
        "Prepared: {}x{} RGB per-channel",
        prepared[0].width,
        prepared[0].height
    );
    println!("Threshold: {:.4}", detector::MATCH_THRESHOLD);
    println!();

    for screenshot_path in &args[2..] {
        let screenshot = match image::open(screenshot_path) {
            Ok(img) => img,
            Err(e) => {
                eprintln!("Failed to load {screenshot_path}: {e}");
                continue;
            }
        };

        let best = detector::find_best_match(&screenshot, &prepared);
        let matches = detector::find_matches(&screenshot, &prepared).unwrap_or_default();

        match best {
            Some(m) => {
                let status = if m.score >= detector::MATCH_THRESHOLD {
                    "MATCH"
                } else {
                    "no match"
                };
                println!(
                    "{screenshot_path}: {status} score={:.4} pixel=({}, {}) above_threshold={}",
                    m.score,
                    m.x,
                    m.y,
                    matches.len()
                );
            }
            None => {
                println!("{screenshot_path}: no correlation result");
            }
        }
    }
}
