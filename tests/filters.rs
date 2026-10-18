//! Golden-image regression tests for every filter, plus the release preview
//! benchmark. Run `UPDATE_GOLDEN=1 cargo test --test filters` to rewrite goldens.

use std::path::PathBuf;

use game_asset_creator::document::PixelRect;
use game_asset_creator::filters::{self, ParamValue, Params};
use image::{Rgba, RgbaImage};

/// A deterministic synthetic test image: gradients, shapes, edges, and alpha.
fn synthetic(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
        let mut p = [
            (fx * 255.0) as u8,
            (fy * 255.0) as u8,
            ((1.0 - fx) * 200.0) as u8,
            255,
        ];
        // A bright disc.
        let (dx, dy) = (fx - 0.3, fy - 0.4);
        if dx * dx + dy * dy < 0.04 {
            p = [250, 240, 40, 255];
        }
        // A dark bar with a hard edge.
        if (0.6..0.75).contains(&fx) {
            p = [20, 30, 60, 255];
        }
        // Speckles (for the median).
        if (x * 7 + y * 13) % 37 == 0 {
            p = [255, 255, 255, 255];
        }
        // A semi-transparent band.
        if fy > 0.8 {
            p[3] = ((x * 5) % 256) as u8;
        }
        Rgba(p)
    })
}

/// Fixed, non-default parameters exercising each filter.
fn golden_params(id: &str) -> Params {
    let filter = filters::find(id).expect("registered");
    let mut p = Params::defaults(&filter.params());
    let mut set = |k: &'static str, v: ParamValue| p.set(k, v);
    match id {
        "brightness_contrast" => {
            set("brightness", ParamValue::Int(20));
            set("contrast", ParamValue::Int(40));
        }
        "levels" => {
            set("rgb.in_black", ParamValue::Int(30));
            set("rgb.gamma", ParamValue::Float(1.4));
            set("red.out_white", ParamValue::Int(200));
        }
        "curves" => {
            set(
                "rgb.curve",
                ParamValue::Curve(vec![
                    (0.0, 0.0),
                    (80.0, 120.0),
                    (200.0, 210.0),
                    (255.0, 255.0),
                ]),
            );
            set(
                "green.curve",
                ParamValue::Curve(vec![(0.0, 30.0), (255.0, 220.0)]),
            );
        }
        "hue_saturation" => {
            set("hue", ParamValue::Int(45));
            set("saturation", ParamValue::Int(-30));
            set("lightness", ParamValue::Int(10));
        }
        "color_balance" => {
            set("shadows.yellow_blue", ParamValue::Int(40));
            set("midtones.cyan_red", ParamValue::Int(30));
            set("highlights.magenta_green", ParamValue::Int(-25));
        }
        "posterize" => set("levels", ParamValue::Int(5)),
        "gaussian_blur" => set("radius", ParamValue::Float(3.5)),
        "motion_blur" => {
            set("angle", ParamValue::Int(30));
            set("distance", ParamValue::Int(9));
        }
        "unsharp_mask" => {
            set("amount", ParamValue::Int(150));
            set("radius", ParamValue::Float(1.5));
            set("threshold", ParamValue::Int(4));
        }
        "add_noise" => {
            set("amount", ParamValue::Int(25));
            set("distribution", ParamValue::Choice(1));
            set("seed", ParamValue::Seed(20261018));
        }
        "median" => set("radius", ParamValue::Int(2)),
        _ => {}
    }
    p
}

fn golden_path(id: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{id}.png"))
}

#[test]
fn every_filter_matches_its_golden_image() {
    let input = synthetic(64, 48);
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut failures = Vec::new();
    for filter in filters::registry() {
        let id = filter.id();
        let params = golden_params(id);
        let output =
            filters::render_filter(&input, None, PixelRect::new(0, 0, 64, 48), *filter, &params);
        let path = golden_path(id);
        if update {
            output.save(&path).expect("write golden");
            continue;
        }
        match image::open(&path) {
            Ok(golden) => {
                if golden.into_rgba8() != output {
                    failures.push(format!("{id}: output differs from {}", path.display()));
                }
            }
            Err(e) => failures.push(format!("{id}: missing golden {} ({e})", path.display())),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn goldens_differ_from_the_input() {
    // Guards against a filter silently doing nothing.
    let input = synthetic(64, 48);
    for filter in filters::registry() {
        let output = filters::render_filter(
            &input,
            None,
            PixelRect::new(0, 0, 64, 48),
            *filter,
            &golden_params(filter.id()),
        );
        assert_ne!(output, input, "{} changed nothing", filter.id());
    }
}

/// Large settings from the filters spec.
fn large_params(id: &str) -> Option<Params> {
    let filter = filters::find(id)?;
    let mut p = Params::defaults(&filter.params());
    match id {
        "gaussian_blur" => p.set("radius", ParamValue::Float(50.0)),
        "motion_blur" => p.set("distance", ParamValue::Int(100)),
        "unsharp_mask" => p.set("radius", ParamValue::Float(50.0)),
        "median" => p.set("radius", ParamValue::Int(10)),
        _ => return None,
    }
    Some(p)
}

/// Release benchmark: `cargo test --release --test filters -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_previews_1080p() {
    let input = synthetic(1920, 1080);
    let rect = PixelRect::new(0, 0, 1920, 1080);
    let mut worst = 0.0f64;
    let mut report = Vec::new();
    for filter in filters::registry() {
        let mut runs = vec![("default", Params::defaults(&filter.params()))];
        if let Some(p) = large_params(filter.id()) {
            runs.push(("large", p));
        }
        for (label, params) in runs {
            // Warm up once, then time the median of three runs.
            let _ = filters::render_filter(&input, None, rect, *filter, &params);
            let mut times: Vec<f64> = (0..3)
                .map(|_| {
                    let start = std::time::Instant::now();
                    let _ = filters::render_filter(&input, None, rect, *filter, &params);
                    start.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            times.sort_by(f64::total_cmp);
            let ms = times[1];
            worst = worst.max(ms);
            report.push(format!("{:<22} {:<8} {:>7.2} ms", filter.id(), label, ms));
        }
    }
    println!("{}", report.join("\n"));
    if !cfg!(debug_assertions) {
        assert!(worst < 100.0, "slowest preview {worst:.2} ms");
    }
}

#[test]
fn noise_is_keyed_to_absolute_positions() {
    // The same seed gives a pixel the same noise whatever area is filtered.
    let input = synthetic(64, 48);
    let noise = filters::find("add_noise").unwrap();
    let params = golden_params("add_noise");
    let full = filters::render_filter(&input, None, PixelRect::new(0, 0, 64, 48), noise, &params);
    let part = filters::render_filter(&input, None, PixelRect::new(17, 9, 30, 20), noise, &params);
    for (x, y, p) in part.enumerate_pixels() {
        assert_eq!(
            p,
            full.get_pixel(17 + x, 9 + y),
            "at ({}, {})",
            17 + x,
            9 + y
        );
    }
}
