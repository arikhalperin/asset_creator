//! The extraction engine against the fixtures in tests/fixtures/extract
//! (regenerate with `UPDATE_FIXTURES=1 cargo test --test extract_fixtures -- --ignored`),
//! plus release benchmarks.

use std::path::PathBuf;

use game_asset_creator::document::PixelRect;
use game_asset_creator::extract::detect::{self, DetectParams};
use game_asset_creator::extract::keying::{self, KeyParams};
use game_asset_creator::extract::segment::{Config, Engine, Prompt};
use game_asset_creator::extract::{grabcut, models, sam, tight_crop};
use image::{GrayImage, RgbaImage};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/extract")
        .join(name)
}

fn load(name: &str) -> RgbaImage {
    image::open(fixture(name)).unwrap().into_rgba8()
}

fn json(name: &str) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(fixture(name)).unwrap()).unwrap()
}

fn truth_boxes(name: &str) -> Vec<(String, PixelRect)> {
    json(name)["sprites"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            let n = |k: &str| s[k].as_u64().unwrap() as u32;
            (
                s["name"].as_str().unwrap().to_owned(),
                PixelRect::new(n("x"), n("y"), n("w"), n("h")),
            )
        })
        .collect()
}

/// Proposals for a sheet the way Auto-Detect makes them.
fn detect_sheet(image: &RgbaImage) -> Vec<detect::Proposal> {
    let keyed = if image.pixels().any(|p| p[3] < 255) {
        image.clone()
    } else {
        keying::key(image, &KeyParams::auto(image))
    };
    detect::detect(&keyed, &DetectParams::default())
}

fn assert_bounds(sheet: &str, truth: &str, slack: u32) {
    let image = load(sheet);
    let found = detect_sheet(&image);
    let truth = truth_boxes(truth);
    assert_eq!(
        found.len(),
        truth.len(),
        "{sheet}: one proposal per sprite: {:?}",
        found.iter().map(|p| p.rect).collect::<Vec<_>>()
    );
    for (name, want) in &truth {
        let close = |a: u32, b: u32| a.abs_diff(b) <= slack;
        let hit = found.iter().find(|p| {
            close(p.rect.x, want.x)
                && close(p.rect.y, want.y)
                && close(p.rect.right(), want.right())
                && close(p.rect.bottom(), want.bottom())
        });
        assert!(hit.is_some(), "{sheet}: {name} {want:?} not found");
    }
}

#[test]
fn auto_detect_finds_every_sprite_with_exact_bounds() {
    assert_bounds("sheet_flat.png", "sheet_flat.json", 0);
    assert_bounds("sheet_transparent.png", "sheet_transparent.json", 0);
}

#[test]
fn auto_detect_on_a_near_flat_background_is_within_one_pixel() {
    assert_bounds("sheet_noisy.png", "sheet_noisy.json", 1);
}

#[test]
fn remove_background_clears_background_and_keeps_sprites() {
    let coverage = image::open(fixture("sheet_coverage.png"))
        .unwrap()
        .into_luma8();
    for sheet in ["sheet_flat.png", "sheet_noisy.png"] {
        let image = load(sheet);
        let params = KeyParams::auto(&image);
        assert!(
            params
                .color
                .iter()
                .zip([255u8, 0, 255])
                .all(|(a, b)| a.abs_diff(b) <= 3),
            "{sheet}: auto color {:?}",
            params.color
        );
        let keyed = keying::key(&image, &params);
        let (mut edges, mut decontaminated) = (0, 0);
        for (x, y, c) in coverage.enumerate_pixels() {
            let (before, after) = (image.get_pixel(x, y), keyed.get_pixel(x, y));
            match c[0] {
                0 => assert_eq!(after[3], 0, "{sheet}: background ({x}, {y}) stays"),
                255 => assert_eq!(after, before, "{sheet}: interior ({x}, {y}) changed"),
                _ => {
                    assert!(after[3] > 0, "{sheet}: edge ({x}, {y}) vanished");
                    if after[3] < 255 {
                        edges += 1;
                        // Magenta removed: green rises or red/blue fall.
                        if after[1] >= before[1] && (after[0] <= before[0] || after[2] <= before[2])
                        {
                            decontaminated += 1;
                        }
                    }
                }
            }
        }
        assert!(edges >= 100, "{sheet}: soft edge pixels {edges}");
        assert_eq!(
            decontaminated, edges,
            "{sheet}: background color removed from edges"
        );
    }
}

fn iou(mask: &GrayImage, truth: &GrayImage) -> f32 {
    let (mut i, mut u) = (0u32, 0u32);
    for (a, b) in mask.pixels().zip(truth.pixels()) {
        let (a, b) = (a[0] >= 128, b[0] >= 128);
        i += (a && b) as u32;
        u += (a || b) as u32;
    }
    i as f32 / u as f32
}

/// IoU ≥ 0.9, one 8-connected region, no holes or islands over 0.5 %.
fn assert_clean(name: &str, mask: &GrayImage, truth: &GrayImage) {
    let score = iou(mask, truth);
    assert!(score >= 0.90, "{name}: IoU {score:.3}");
    let (w, h) = mask.dimensions();
    let regions = detect::label(w, h, |x, y| mask.get_pixel(x, y)[0] >= 128, true);
    assert_eq!(regions.sizes.len(), 2, "{name}: one region");
    let area = regions.sizes[1];
    let holes = detect::label(w, h, |x, y| mask.get_pixel(x, y)[0] < 128, false);
    let mut border = vec![false; holes.sizes.len()];
    for x in 0..w {
        border[holes.label[x as usize] as usize] = true;
        border[holes.label[((h - 1) * w + x) as usize] as usize] = true;
    }
    for y in 0..h {
        border[holes.label[(y * w) as usize] as usize] = true;
        border[holes.label[(y * w + w - 1) as usize] as usize] = true;
    }
    for (l, &size) in holes.sizes.iter().enumerate().skip(1) {
        assert!(
            border[l] || size as f32 <= area as f32 * 0.005,
            "{name}: hole of {size} px"
        );
    }
}

fn photos() -> Vec<(String, RgbaImage, GrayImage, (f32, f32))> {
    json("photos.json")["photos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let name = p["photo"].as_str().unwrap().to_owned();
            let truth = image::open(fixture(p["mask"].as_str().unwrap()))
                .unwrap()
                .into_luma8();
            let click = (
                p["click"][0].as_f64().unwrap() as f32,
                p["click"][1].as_f64().unwrap() as f32,
            );
            (name.clone(), load(&name), truth, click)
        })
        .collect()
}

#[test]
fn grabcut_click_returns_a_clean_mask_on_every_photo() {
    for (name, photo, truth, click) in photos() {
        let mask = grabcut::segment(&photo, &Prompt::click(click));
        assert_clean(&format!("{name} (GrabCut)"), &mask, &truth);
    }
}

/// MobileSAM, when ONNX Runtime and the models are installed (otherwise the
/// test reports that it skipped).
#[test]
fn mobilesam_click_returns_a_clean_mask_on_every_photo() {
    let config = Config::default();
    let Ok(env) = sam::runtime(&config.runtime_paths) else {
        eprintln!("skipped: ONNX Runtime not installed");
        return;
    };
    if !models::present(&config.model_dir) {
        eprintln!(
            "skipped: MobileSAM models not in {}",
            config.model_dir.display()
        );
        return;
    }
    let mut model = sam::SamModel::load(env, &config.model_dir).unwrap();
    for (name, photo, truth, click) in photos() {
        let embedding = model.embed(&photo).unwrap();
        let mask = model.decode(&embedding, &Prompt::click(click)).unwrap();
        assert_clean(&format!("{name} (MobileSAM)"), &mask, &truth);
    }
    let _ = Engine::Sam;
}

#[test]
fn every_extraction_is_a_tight_transparent_crop() {
    let image = load("sheet_flat.png");
    let keyed = keying::key(&image, &KeyParams::auto(&image));
    let dir = tempfile::tempdir().unwrap();
    for (i, p) in detect::detect(&keyed, &DetectParams::default())
        .iter()
        .enumerate()
    {
        let mut full = GrayImage::new(image.width(), image.height());
        image::imageops::replace(&mut full, &p.mask, p.rect.x as i64, p.rect.y as i64);
        let (asset, rect) = tight_crop(&keyed, &full).unwrap();
        assert_eq!(rect, p.rect);
        assert_tight(&asset);
        let path = dir.path().join(format!("{i}.png"));
        asset.save(&path).unwrap();
        let back = image::open(&path).unwrap();
        assert_eq!(back.color(), image::ColorType::Rgba8);
        assert_eq!(back.into_rgba8(), asset, "PNG round trip");
    }
    // A GrabCut extraction from a photo.
    let (_, photo, _, click) = photos().remove(0);
    let mask = grabcut::segment(&photo, &Prompt::click(click));
    let (asset, _) = tight_crop(&photo, &mask).unwrap();
    assert_tight(&asset);
    assert!(
        asset.pixels().any(|p| p[3] == 0),
        "outside the object is transparent"
    );
}

fn assert_tight(asset: &RgbaImage) {
    let (w, h) = asset.dimensions();
    let opaque = |x: u32, y: u32| asset.get_pixel(x, y)[3] > 0;
    assert!((0..w).any(|x| opaque(x, 0)) && (0..w).any(|x| opaque(x, h - 1)));
    assert!((0..h).any(|y| opaque(0, y)) && (0..h).any(|y| opaque(w - 1, y)));
}

fn time_ms(f: impl FnOnce()) -> f64 {
    let start = std::time::Instant::now();
    f();
    start.elapsed().as_secs_f64() * 1000.0
}

/// Release benchmark: `cargo test --release --test extract -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_extraction() {
    // Remove Background preview through the filter path on 1920×1080.
    let photo = image::imageops::resize(
        &load("sheet_noisy.png"),
        1920,
        1080,
        image::imageops::FilterType::Nearest,
    );
    let params = keying::RemoveBackground::params_for(&KeyParams::auto(&photo));
    let rect = PixelRect::new(0, 0, 1920, 1080);
    let _ = game_asset_creator::filters::render_filter(
        &photo,
        None,
        rect,
        &keying::RemoveBackground,
        &params,
    );
    let key_ms = time_ms(|| {
        game_asset_creator::filters::render_filter(
            &photo,
            None,
            rect,
            &keying::RemoveBackground,
            &params,
        );
    });
    // Auto-Detect on 2048×2048 (a 4×5 tiling of the transparent sheet).
    let tile = load("sheet_transparent.png");
    let mut big = RgbaImage::new(2048, 2048);
    for ty in 0..6 {
        for tx in 0..4 {
            image::imageops::replace(&mut big, &tile, tx * 520, ty * 400);
        }
    }
    let detect_ms = time_ms(|| {
        detect::detect(&big, &DetectParams::default());
    });
    // GrabCut on 1920×1080.
    let (_, small, _, click) = photos().remove(0);
    let hd = image::imageops::resize(&small, 1920, 1080, image::imageops::FilterType::Triangle);
    let hd_click = (click.0 * 3.0, click.1 * 2.25);
    let grabcut_ms = time_ms(|| {
        grabcut::segment(&hd, &Prompt::click(hd_click));
    });
    println!("remove background {key_ms:.1} ms, auto-detect {detect_ms:.1} ms, grabcut {grabcut_ms:.0} ms");
    // MobileSAM when installed.
    let config = Config::default();
    let mut sam_line = "MobileSAM not installed".to_owned();
    let mut sam_times = None;
    if let (Ok(env), true) = (
        sam::runtime(&config.runtime_paths),
        models::present(&config.model_dir),
    ) {
        let mut model = sam::SamModel::load(env, &config.model_dir).unwrap();
        let mut embedding = None;
        let first = time_ms(|| {
            let e = model.embed(&hd).unwrap();
            model.decode(&e, &Prompt::click(hd_click)).unwrap();
            embedding = Some(e);
        });
        let embedding = embedding.unwrap();
        let later = time_ms(|| {
            model
                .decode(&embedding, &Prompt::click((hd_click.0 + 30.0, hd_click.1)))
                .unwrap();
        });
        sam_line = format!("MobileSAM first {first:.0} ms, later {later:.0} ms");
        sam_times = Some((first, later));
    }
    println!("{sam_line}");
    if !cfg!(debug_assertions) {
        assert!(key_ms < 150.0, "remove background {key_ms:.1} ms");
        assert!(detect_ms < 250.0, "auto-detect {detect_ms:.1} ms");
        assert!(grabcut_ms < 2000.0, "grabcut {grabcut_ms:.0} ms");
        if let Some((first, later)) = sam_times {
            assert!(first < 2000.0 && later < 200.0, "{sam_line}");
        }
    }
}

// --- Whole-video extraction (tests/fixtures/video/walk.mp4) ---------------

fn clusters_of(clip: &str, every: u64) -> Vec<game_asset_creator::extract::video::Cluster> {
    use game_asset_creator::extract::video::{self, VideoExtractSpec};
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/video")
        .join(clip);
    let spec = VideoExtractSpec {
        path,
        from: 0,
        to: 199,
        every,
        detect: DetectParams::default(),
    };
    let none = std::sync::atomic::AtomicBool::new(false);
    video::run(&spec, &std::sync::atomic::AtomicU64::new(0), &none, &|| {}).unwrap()
}

fn walk_clusters() -> Vec<game_asset_creator::extract::video::Cluster> {
    clusters_of("walk.mp4", 1)
}

/// Flat, symmetric static shapes and a thin-limbed figure under H.264
/// (crf 18): every static shape stays one asset, the figure has exactly its
/// 4 poses, with every frame and with every second frame.
#[test]
fn compressed_flat_shapes_and_thin_limbs_dedup_correctly() {
    for every in [1, 2] {
        let clusters = clusters_of("shapes.mp4", every);
        let summary: Vec<usize> = clusters.iter().map(|c| c.poses.len()).collect();
        assert_eq!(clusters.len(), 4, "every {every}: {summary:?}");
        assert_eq!(
            clusters.iter().filter(|c| !c.is_animation()).count(),
            3,
            "every {every}: {summary:?}"
        );
        let figure = clusters.iter().find(|c| c.is_animation()).unwrap();
        assert_eq!(figure.poses.len(), 4, "every {every}: {summary:?}");
        assert!(clusters
            .iter()
            .all(|c| c.frames.len() == (200 / every) as usize));
    }
    let first: Vec<u64> = clusters_of("shapes.mp4", 1)
        .into_iter()
        .find(|c| c.is_animation())
        .unwrap()
        .poses
        .iter()
        .map(|p| p.frame)
        .collect();
    assert_eq!(first, vec![0, 4, 8, 12]);
}

/// Fine black/white stripes under crf 28 with sub-pixel motion: the crate
/// stays one asset and the figure has exactly its 5 poses.
#[test]
fn fine_textures_under_heavy_compression_keep_the_pose_count() {
    for every in [1, 2, 3] {
        let clusters = clusters_of("textured.mp4", every);
        let summary: Vec<usize> = clusters.iter().map(|c| c.poses.len()).collect();
        assert_eq!(clusters.len(), 2, "every {every}: {summary:?}");
        assert_eq!(
            clusters.iter().filter(|c| !c.is_animation()).count(),
            1,
            "every {every}: {summary:?}"
        );
        assert_eq!(
            clusters
                .iter()
                .find(|c| c.is_animation())
                .unwrap()
                .poses
                .len(),
            5,
            "every {every}: {summary:?}"
        );
    }
}

/// Heavy compression (crf 28) with sub-pixel motion: still three static
/// assets and exactly four poses, for every frame and every second or third.
#[test]
fn heavy_compression_and_subpixel_motion_keep_the_pose_count() {
    for every in [1, 2, 3] {
        let clusters = clusters_of("moving.mp4", every);
        let summary: Vec<usize> = clusters.iter().map(|c| c.poses.len()).collect();
        assert_eq!(clusters.len(), 4, "every {every}: {summary:?}");
        assert_eq!(
            clusters.iter().filter(|c| !c.is_animation()).count(),
            3,
            "every {every}: {summary:?}"
        );
        assert_eq!(
            clusters
                .iter()
                .find(|c| c.is_animation())
                .unwrap()
                .poses
                .len(),
            4,
            "every {every}: {summary:?}"
        );
    }
}

#[test]
fn walk_clip_yields_one_potion_and_one_eight_pose_walk() {
    let clusters = walk_clusters();
    let summary: Vec<(usize, usize)> = clusters
        .iter()
        .map(|c| (c.poses.len(), c.frames.len()))
        .collect();
    assert_eq!(clusters.len(), 2, "clusters (poses, frames): {summary:?}");
    let potion = clusters
        .iter()
        .find(|c| !c.is_animation())
        .expect("a single-asset cluster");
    assert_eq!(
        potion.frames.len(),
        200,
        "the potion is seen in every frame"
    );
    let walk = clusters
        .iter()
        .find(|c| c.is_animation())
        .expect("an animation cluster");
    assert_eq!(walk.poses.len(), 8, "eight poses");
    // In walk order: pose k first appears at frame 3k (the clip shows a new
    // pose every 3 frames), the poses are pairwise distinct, and each is the
    // same appearance as the other frames of its 3-frame window.
    use game_asset_creator::extract::video::{detect_frame, same_appearance};
    for (i, a) in walk.poses.iter().enumerate() {
        for b in &walk.poses[i + 1..] {
            assert!(
                !same_appearance(a, b),
                "poses at frames {} and {} are the same",
                a.frame,
                b.frame
            );
        }
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video/walk.mp4");
    let mut src = game_asset_creator::video::VideoSource::open(&path).unwrap();
    for (k, pose) in walk.poses.iter().enumerate() {
        for t in [3 * k as u64 + 1, 3 * k as u64 + 2, 3 * k as u64 + 24] {
            let frame = src.frame(t).unwrap();
            let skeleton = detect_frame(t, &frame.image, &DetectParams::default())
                .into_iter()
                .max_by_key(|c| c.rect.x)
                .unwrap();
            assert!(same_appearance(pose, &skeleton), "pose {k} vs frame {t}");
        }
    }
    let first_frames: Vec<u64> = walk.poses.iter().map(|p| p.frame).collect();
    assert_eq!(first_frames, vec![0, 3, 6, 9, 12, 15, 18, 21]);
}

/// Release benchmark: `cargo test --release --test extract bench_video -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_video_extraction() {
    let start = std::time::Instant::now();
    let clusters = walk_clusters();
    let seconds = start.elapsed().as_secs_f64();
    println!(
        "whole-video extraction of walk.mp4 (200 frames): {seconds:.2} s, {} clusters",
        clusters.len()
    );
    if !cfg!(debug_assertions) {
        assert!(seconds < 10.0, "{seconds:.2} s");
    }
}
