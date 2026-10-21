//! Pattern detection and seamless tiles on the brick/stone fixtures.

use std::path::PathBuf;

use game_asset_creator::pattern::{self, SEAMLESS_RATIO};
use image::RgbaImage;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/patterns")
}

fn load(file: &str) -> RgbaImage {
    image::open(dir().join(file)).unwrap().into_rgba8()
}

fn fixtures() -> Vec<(String, (u32, u32))> {
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir().join("patterns.json")).unwrap())
            .unwrap();
    meta["images"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            let t = i["tile"].as_array().unwrap();
            (
                i["file"].as_str().unwrap().to_owned(),
                (t[0].as_u64().unwrap() as u32, t[1].as_u64().unwrap() as u32),
            )
        })
        .collect()
}

fn crop(img: &RgbaImage, x: u32, y: u32, w: u32, h: u32) -> RgbaImage {
    image::imageops::crop_imm(img, x, y, w, h).to_image()
}

#[test]
fn detector_recovers_the_known_tile_size_wherever_the_region_starts() {
    let all = fixtures();
    assert!(all.len() >= 15);
    for (file, tile) in all {
        let img = load(&file);
        for (dx, dy) in [(0, 0), (7, 3), (tile.0 / 2, tile.1 / 3), (tile.0 - 1, 1)] {
            let region = crop(&img, dx, dy, img.width() - dx, img.height() - dy);
            let d = pattern::detect(&region);
            assert!(d.found, "{file} at {dx},{dy}");
            assert_eq!(d.period, tile, "{file} at {dx},{dy}");
            assert!(d.confidence >= pattern::MIN_CONFIDENCE);
            let t = d.tile;
            assert!(t.right() <= region.width() && t.bottom() <= region.height());
            let tile_img = crop(&region, t.x, t.y, t.width, t.height);
            let ratio = pattern::seam_ratio(&tile_img);
            assert!(
                ratio <= SEAMLESS_RATIO,
                "{file} at {dx},{dy}: detected tile seam {ratio:.2}"
            );
        }
    }
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) % n.max(1) as u64) as u32
    }
}

/// Random regions with at least 2.05 repeats per axis: always the exact
/// tile; with fewer than two repeats on an axis: never a pattern.
#[test]
fn random_regions_find_the_tile_only_with_two_repeats() {
    let mut rng = Rng(77);
    // More regions with GAC_PATTERN_SWEEP=<n> (12 per image by default).
    let count: u32 = std::env::var("GAC_PATTERN_SWEEP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    for (file, (tw, th)) in fixtures() {
        let img = load(&file);
        let (w, h) = img.dimensions();
        for _ in 0..count {
            let rw =
                (2.05 * tw as f32).ceil() as u32 + rng.below(w - (2.05 * tw as f32).ceil() as u32);
            let rh =
                (2.05 * th as f32).ceil() as u32 + rng.below(h - (2.05 * th as f32).ceil() as u32);
            let (x, y) = (rng.below(w - rw + 1), rng.below(h - rh + 1));
            let d = pattern::detect(&crop(&img, x, y, rw, rh));
            assert_eq!(d.period, (tw, th), "{file} {x},{y} {rw}x{rh}");
            assert!(d.found);
        }
        for i in 0..count {
            // One axis (alternating) below two repeats.
            let short = |t: u32, n: u32| {
                (t as f32 * (1.1 + 0.85 * ((i % 12) as f32 / 11.0))) as u32 % n.max(1)
            };
            let (mut rw, mut rh) = (w, h);
            if i % 2 == 0 {
                rw = short(tw, w).max(8);
            } else {
                rh = short(th, h).max(8);
            }
            let (x, y) = (rng.below(w - rw + 1), rng.below(h - rh + 1));
            let d = pattern::detect(&crop(&img, x, y, rw, rh));
            assert!(!d.found, "{file} {x},{y} {rw}x{rh}: {d:?}");
            assert_eq!(
                d.tile,
                game_asset_creator::document::PixelRect::new(0, 0, rw, rh)
            );
        }
    }
}

/// Regions just under two repeats (2t − 1, 2t − 2 on one axis) never report
/// a pattern; exactly two repeats report the tile.
#[test]
fn just_under_two_repeats_is_no_pattern() {
    for (file, (tw, th)) in fixtures() {
        let img = load(&file);
        let (w, h) = img.dimensions();
        // Every start offset with GAC_PATTERN_SWEEP set.
        let offsets: Vec<u32> = if std::env::var_os("GAC_PATTERN_SWEEP").is_some() {
            (0..tw).collect()
        } else {
            vec![0, 1, tw / 3, tw - 1]
        };
        for x in offsets {
            for (rw, rh, found) in [
                (2 * tw - 1, h, false),
                (2 * tw - 2, h, false),
                (w, 2 * th - 1, false),
                (w, 2 * th - 2, false),
                (2 * tw, 2 * th, true),
            ] {
                let (x, y) = (x.min(w - rw), (x / 2).min(h - rh));
                let d = pattern::detect(&crop(&img, x, y, rw, rh));
                assert_eq!(d.found, found, "{file} {x},{y} {rw}x{rh}: {d:?}");
                if found {
                    assert_eq!(d.period, (tw, th), "{file} {x},{y} {rw}x{rh}");
                }
            }
        }
    }
}

/// Regions the first review found reporting made-up periods.
#[test]
fn partial_repeats_of_brick_report_no_pattern() {
    for (file, x, y, w, h) in [
        ("brick_synthetic.png", 3, 2, 60, 48),
        ("brick_synthetic.png", 0, 0, 60, 96),
        ("brick_grey.png", 0, 0, 180, 90),
        ("brick_grey.png", 0, 0, 360, 90),
        ("brick_red.png", 0, 0, 240, 240),
        ("brick_red.png", 85, 3, 318, 200),
        ("brick_red.png", 7, 40, 319, 240),
    ] {
        let d = pattern::detect(&crop(&load(file), x, y, w, h));
        assert!(!d.found, "{file} {w}x{h}: {d:?}");
    }
    let flat = RgbaImage::from_pixel(80, 60, image::Rgba([120, 80, 40, 255]));
    assert!(!pattern::detect(&flat).found, "flat colour");
}

/// A tile with transparent holes repeats like any other.
#[test]
fn transparent_patterns_are_detected_and_keep_alpha() {
    let tile = crop(&load("brick_synthetic.png"), 0, 0, 40, 32);
    let holes = RgbaImage::from_fn(40, 32, |x, y| {
        let mut p = *tile.get_pixel(x, y);
        if (x / 5 + y / 4) % 3 == 0 {
            p[3] = 0;
        }
        p
    });
    let region = crop(&pattern::repeat(&holes, 5, 4), 7, 5, 180, 110);
    let d = pattern::detect(&region);
    assert_eq!(d.period, (40, 32));
    let t = d.tile;
    let cut = crop(&region, t.x, t.y, t.width, t.height);
    assert!(cut.pixels().any(|p| p[3] == 0));
    let s = pattern::make_seamless(&cut, pattern::DEFAULT_BLEND);
    assert!(s.pixels().any(|p| p[3] == 0), "alpha kept");
}

#[test]
fn non_repeating_images_report_no_pattern() {
    for file in ["stone_source.png", "brick_red_source.png"] {
        // Whole textures repeat only once.
        let d = pattern::detect(&load(file));
        assert!(!d.found, "{file}: {d:?}");
    }
}

#[test]
fn make_seamless_closes_the_seam_of_arbitrary_crops() {
    let mut worst_raw = f32::INFINITY;
    let mut worst = 0.0f32;
    for file in [
        "brick_red_source.png",
        "brick_grey_source.png",
        "cobblestone_source.png",
        "stone_source.png",
    ] {
        let src = load(file);
        for (x, y, w, h) in [
            (13, 9, 97, 61),
            (40, 30, 64, 64),
            (5, 50, 120, 90),
            (70, 3, 48, 100),
        ] {
            let w = w.min(src.width() - x);
            let h = h.min(src.height() - y);
            let raw = crop(&src, x, y, w, h);
            let r = pattern::seam_ratio(&raw);
            worst_raw = worst_raw.min(r);
            assert!(r > SEAMLESS_RATIO, "{file} {x},{y},{w}x{h}: raw {r:.2}");
            for blend in [0.05, pattern::DEFAULT_BLEND, 0.5] {
                let s = pattern::make_seamless(&raw, blend);
                assert_eq!(s.dimensions(), raw.dimensions());
                let r = pattern::seam_ratio(&s);
                worst = worst.max(r);
                assert!(r <= SEAMLESS_RATIO, "{file} {x},{y} blend {blend}: {r:.2}");
            }
        }
    }
    eprintln!("lowest raw ratio {worst_raw:.2}, highest seamless ratio {worst:.2}");
}

#[test]
fn repeated_tile_matches_at_every_offset() {
    let t = crop(&load("brick_synthetic.png"), 0, 0, 40, 32);
    let r = pattern::repeat(&t, 4, 3);
    assert_eq!(r.dimensions(), (160, 96));
    for (x, y, p) in r.enumerate_pixels() {
        assert_eq!(p, t.get_pixel(x % 40, y % 32));
    }
}

#[test]
#[ignore]
fn bench_pattern() {
    let img = load("brick_red.png");
    let found = pattern::detect(&img).tile;
    let tile = crop(&img, found.x, found.y, found.width, found.height);
    let region = crop(&pattern::repeat(&tile, 7, 13), 5, 9, 1024, 1024);
    let t = std::time::Instant::now();
    let d = pattern::detect(&region);
    let detect = t.elapsed();
    assert_eq!(d.period, (160, 80));
    let tile = crop(&load("stone_source.png"), 0, 0, 256, 256);
    let tile = image::imageops::resize(&tile, 512, 512, image::imageops::FilterType::Triangle);
    let t = std::time::Instant::now();
    let s = pattern::make_seamless(&tile, pattern::DEFAULT_BLEND);
    let seamless = t.elapsed();
    assert!(pattern::seam_ratio(&s) <= SEAMLESS_RATIO);
    let odd = crop(&pattern::repeat(&tile, 9, 9), 0, 0, 1025, 1025);
    let t = std::time::Instant::now();
    let _ = pattern::detect(&odd);
    let odd_ms = t.elapsed().as_secs_f64() * 1e3;
    eprintln!("pattern detection 1025x1025: {odd_ms:.0} ms");
    eprintln!(
        "pattern detection 1024x1024: {:.0} ms, make seamless 512x512: {:.0} ms",
        detect.as_secs_f64() * 1e3,
        seamless.as_secs_f64() * 1e3
    );
}

/// Random crops: Make Seamless always closes the seam; the raw ratios are
/// reported (crops lined up with a regular brick grid can already be nearly
/// seamless).
#[test]
fn survey_of_random_crops() {
    let mut s = 12345u64;
    let mut rnd = |n: u32| {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((s >> 33) % n as u64) as u32
    };
    for file in [
        "brick_red_source.png",
        "brick_grey_source.png",
        "cobblestone_source.png",
        "stone_source.png",
    ] {
        let src = load(file);
        let mut raw = Vec::new();
        let mut worst = 0.0f32;
        let mut found = 0u32;
        for _ in 0..200 {
            let w = 48 + rnd(81.min(src.width() - 60));
            let h = 48 + rnd(81.min(src.height() - 60));
            let x = rnd(src.width() - w + 1);
            let y = rnd(src.height() - h + 1);
            let c = crop(&src, x, y, w, h);
            raw.push(pattern::seam_ratio(&c));
            found += pattern::detect(&c).found as u32;
            let r = pattern::seam_ratio(&pattern::make_seamless(&c, pattern::DEFAULT_BLEND));
            assert!(r <= SEAMLESS_RATIO, "{file} {x},{y},{w}x{h}: {r:.2}");
            worst = worst.max(r);
        }
        raw.sort_by(f32::total_cmp);
        eprintln!(
            "{file}: raw min {:.2}, median {:.2}, below 1.5: {}/200; seamless worst {worst:.2}; pattern found in {found}/200",
            raw[0],
            raw[100],
            raw.iter().filter(|r| **r < 1.5).count()
        );
    }
}

#[test]
#[ignore]
fn probe_shift_ratios() {
    fn lum(img: &RgbaImage) -> (Vec<f32>, usize, usize) {
        let v: Vec<f32> = img
            .pixels()
            .map(|p| {
                let a = p[3] as f32 / 255.0;
                (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) * a
                    + 128.0 * (1.0 - a)
            })
            .collect();
        let m = v.iter().sum::<f32>() / v.len() as f32;
        (
            v.iter().map(|x| x - m).collect(),
            img.width() as usize,
            img.height() as usize,
        )
    }
    fn ratio(img: &RgbaImage, lag: usize, vertical: bool) -> f32 {
        let (l, w, h) = lum(img);
        let s = (l.iter().map(|v| v * v).sum::<f32>() / l.len() as f32).sqrt();
        let mut sum = 0.0;
        let mut n = 0;
        for y in 0..h {
            for x in 0..w {
                let (x2, y2) = if vertical { (x, y + lag) } else { (x + lag, y) };
                if x2 < w && y2 < h {
                    sum += (l[y * w + x] - l[y2 * w + x2]).abs();
                    n += 1;
                }
            }
        }
        sum / n as f32 / (1.128 * s * std::f32::consts::SQRT_2)
    }
    let mut worst_true = 0.0f32;
    for (file, (tw, th)) in fixtures() {
        let img = load(&file);
        let rx = ratio(&img, tw as usize, false);
        let ry = ratio(&img, th as usize, true);
        worst_true = worst_true.max(rx).max(ry);
        eprintln!("{file}: true x {rx:.3} y {ry:.3}");
    }
    eprintln!("worst true {worst_true:.3}");
    let b = load("brick_synthetic.png");
    eprintln!(
        "synthetic one course y16 full {:.3}, crop {:.3}",
        ratio(&b, 16, true),
        ratio(&crop(&b, 0, 104, 200, 42), 16, true)
    );
    for (f, lag, v) in [
        ("brick_red.png", 80usize, false),
        ("brick_red.png", 40, true),
        ("brick_grey.png", 60, false),
        ("brick_grey.png", 30, true),
        ("cobblestone.png", 36, true),
    ] {
        eprintln!(
            "{f} half-period lag {lag} vertical={v}: {:.3}",
            ratio(&load(f), lag, v)
        );
    }
}
