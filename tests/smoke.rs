//! Headless, offline smoke test of the main workflow (run by
//! scripts/smoke.sh and CI): open → edit → extract → export for an image,
//! and video → Extract Assets → review → library → sprite sheet.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use game_asset_creator::extract::models::Source;
use game_asset_creator::extract::segment::Config;
use game_asset_creator::filters::ParamValue;
use game_asset_creator::library::pack::{ExportFormat, SheetOptions};
use image::{Rgba, RgbaImage};

mod common;
use common::Harness;

fn video(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/video")
        .join(name)
}

fn wait_until(h: &mut Harness, what: &str, mut done: impl FnMut(&Harness) -> bool) {
    let start = Instant::now();
    while !done(h) {
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
        h.frame(vec![]);
    }
}

/// Offline: no models (GrabCut only), nothing downloaded.
fn offline(h: &mut Harness, dir: &Path) {
    h.app.set_segment_config(Config {
        runtime_paths: Vec::new(),
        model_dir: dir.join("no-models"),
        source: Source::Directory(dir.join("no-models")),
    });
}

/// A small sprite sheet: three sprites on a flat magenta background.
fn sheet() -> RgbaImage {
    RgbaImage::from_fn(240, 120, |x, y| {
        let sprite = (x % 80).abs_diff(40) < 20 && y.abs_diff(60) < 30;
        if sprite {
            Rgba([(x / 80 * 90) as u8, 180, 60, 255])
        } else {
            Rgba([255, 0, 255, 255])
        }
    })
}

#[test]
fn image_open_edit_extract_export() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let mut h = Harness::new();
    h.app.open_library(&library);
    offline(&mut h, dir.path());
    // Open.
    let file = dir.path().join("sheet.png");
    sheet().save(&file).unwrap();
    let id = h.app.open_path(&file).expect("opened");
    h.frames(2);
    // Edit: a filter, one undo step.
    assert!(h.app.open_filter("hue_saturation"));
    h.app.set_filter_param("saturation", ParamValue::Int(20));
    assert!(h.app.filter_ok());
    assert_eq!(
        h.app.state.document(id).unwrap().undo_stack().undo_label(),
        Some("Hue/Saturation/Lightness")
    );
    // Extract: Remove Background, then Auto-Detect into the library.
    assert!(h.app.open_remove_background());
    h.app.wait_extraction_dialogs();
    assert!(h.app.remove_background_ok());
    // (From the canvas: a new "Background removed" layer on top.)
    let doc = &h.app.state.document(id).unwrap().document;
    assert_eq!(doc.layer_count(), 2);
    let keyed = doc.layer(doc.active_layer()).unwrap();
    assert_eq!(keyed.name, "Background removed");
    assert_eq!(keyed.pixels.get_pixel(0, 0)[3], 0, "background removed");
    assert!(h.app.open_auto_detect());
    h.app.wait_extraction_dialogs();
    assert_eq!(h.app.auto_detect_add(), 3, "three sprites");
    // Export: the image as PNG, the project, and the assets.
    let png = dir.path().join("out.png");
    assert!(h.app.export_to(&png));
    assert_eq!(
        image::open(&png).unwrap().into_rgba8().dimensions(),
        (240, 120)
    );
    let project = dir.path().join("sheet.gac");
    assert!(h.app.save_to(&project));
    assert!(project.exists());
    let ids: Vec<u64> = h.app.state.assets.assets().iter().map(|a| a.id).collect();
    let folder = dir.path().join("assets");
    let written = h
        .app
        .export_assets(&ids, ExportFormat::Pngs, &folder)
        .unwrap();
    assert_eq!(written.len(), 3);
    for f in written {
        image::open(f).unwrap();
    }
}

#[test]
fn video_extract_review_and_sprite_sheet() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(&dir.path().join("library"));
    offline(&mut h, dir.path());
    assert_eq!(h.app.open_path(&video("walk.mp4")), None);
    let id = *h.app.video_tabs().last().unwrap();
    wait_until(&mut h, "first frame", |h| {
        h.app.video(id).unwrap().displayed().is_some()
    });
    assert!(h.app.open_video_assets(id));
    assert!(h.app.start_video_assets());
    wait_until(&mut h, "review", |h| !h.app.review_tabs().is_empty());
    let review = *h.app.review_tabs().last().unwrap();
    let (singles, animations) = h.app.review_add_kept(review);
    assert!(singles + animations > 0, "assets added from the video");
    let ids: Vec<u64> = h.app.state.assets.assets().iter().map(|a| a.id).collect();
    let sheet = dir.path().join("sheet.png");
    let written = h
        .app
        .export_assets(&ids, ExportFormat::Sheet(SheetOptions::default()), &sheet)
        .unwrap();
    let json = written
        .iter()
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .expect("atlas written");
    let atlas: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(json).unwrap()).unwrap();
    assert!(
        atlas["frames"].as_object().is_some_and(|f| !f.is_empty())
            || atlas["frames"].as_array().is_some_and(|f| !f.is_empty())
    );
    image::open(&sheet).unwrap();
}
