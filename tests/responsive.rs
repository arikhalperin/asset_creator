//! The UI never freezes: Remove Background and Auto-Detect compute on
//! workers (R1), Extract Assets on a 1-minute clip keeps every frame short
//! and cancels promptly (R2), and downloads and Extract Frames cancel at
//! once (R3). Frame-time limits apply to release builds.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use game_asset_creator::document::PixelRect;
use game_asset_creator::extract::detect::{self, DetectParams};
use game_asset_creator::extract::keying::{self, KeyParams, RemoveBackground};
use game_asset_creator::extract::models::Source;
use game_asset_creator::extract::segment::Config;
use game_asset_creator::filters;
use game_asset_creator::tools::Tool;
use image::{Rgba, RgbaImage};

mod common;
use common::Harness;

/// The UI-frame limit while work runs in the background (release).
const FRAME_LIMIT: Duration = Duration::from_millis(50);

fn video(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/video")
        .join(name)
}

/// Runs frames until `done`, asserting each stays under the limit
/// (release); returns the slowest frame and the frame count.
fn frames_until(
    h: &mut Harness,
    what: &str,
    mut done: impl FnMut(&mut Harness) -> bool,
) -> (Duration, usize) {
    let start = Instant::now();
    let (mut worst, mut n) = (Duration::ZERO, 0);
    while !done(h) {
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "timed out: {what}"
        );
        let t = Instant::now();
        h.frame(vec![]);
        let d = t.elapsed();
        worst = worst.max(d);
        n += 1;
        if !cfg!(debug_assertions) {
            assert!(d < FRAME_LIMIT, "{what}: a frame took {d:?}");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    eprintln!("{what}: {n} frames, slowest {worst:?}");
    (worst, n)
}

/// A 2048 × 2048 flat sheet: magenta background, opaque coloured discs.
fn big_sheet() -> RgbaImage {
    RgbaImage::from_fn(2048, 2048, |x, y| {
        let (cx, cy) = ((x / 256) * 256 + 128, (y / 256) * 256 + 128);
        let d = ((x as f32 - cx as f32).powi(2) + (y as f32 - cy as f32).powi(2)).sqrt();
        if d < 80.0 {
            Rgba([((x / 256) * 30) as u8, 200, ((y / 256) * 30) as u8, 255])
        } else {
            Rgba([255, 0, 255, 255])
        }
    })
}

fn open_sheet(h: &mut Harness, dir: &std::path::Path) -> (game_asset_creator::DocId, RgbaImage) {
    let sheet = big_sheet();
    let file = dir.join("sheet.png");
    sheet.save(&file).unwrap();
    let id = h.app.open_path(&file).unwrap();
    h.frames(2);
    (id, sheet)
}

/// The active layer's pixels (Remove Background from the canvas previews
/// on a new layer above the original).
fn layer(h: &Harness, id: game_asset_creator::DocId) -> RgbaImage {
    let doc = &h.app.state.document(id).unwrap().document;
    doc.layer(doc.active_layer()).unwrap().pixels.clone()
}

/// R1: Remove Background renders on a worker: frames stay short, the result
/// equals the synchronous render, a setting changed mid-run wins, and
/// Cancel restores at once.
#[test]
fn remove_background_previews_off_the_ui_thread() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    let (id, sheet) = open_sheet(&mut h, dir.path());
    let full = PixelRect::new(0, 0, sheet.width(), sheet.height());
    assert!(h.app.open_remove_background());
    assert!(
        h.app.remove_background_busy(),
        "the preview renders on a worker"
    );
    // Change the softness while the first render runs: only the new one shows.
    let mut key = h.app.remove_background_dialog().unwrap().key();
    key.softness = (key.softness + 2.0).min(10.0);
    h.app.set_remove_background(key, true);
    frames_until(&mut h, "remove background", |h| {
        !h.app.remove_background_busy()
    });
    let params = h.app.remove_background_dialog().unwrap().params.clone();
    let expected = filters::render_filter(&sheet, None, full, &RemoveBackground, &params);
    assert!(layer(&h, id) == expected, "equals the synchronous render");
    // OK reuses it: one step, same pixels.
    assert!(h.app.remove_background_ok());
    assert!(layer(&h, id) == expected);
    // Cancel while rendering restores at once.
    h.app.undo_current();
    assert!(h.app.open_remove_background());
    assert!(h.app.remove_background_busy());
    let t = Instant::now();
    h.app.remove_background_cancel();
    let call = t.elapsed();
    h.frame(vec![]);
    let d = t.elapsed();
    eprintln!("cancel: call {call:?}, with the next frame {d:?}");
    if !cfg!(debug_assertions) {
        assert!(d < FRAME_LIMIT, "cancel took {d:?}");
    }
    let doc = &h.app.state.document(id).unwrap().document;
    assert_eq!(doc.layer_count(), 1, "the preview layer is gone");
    assert!(layer(&h, id) == sheet, "Cancel restores");
}

/// Whether `needle` is on screen (a new window is laid out invisibly on its
/// first frame, so two frames).
fn shows(h: &mut Harness, needle: &str) -> bool {
    h.frame(vec![]);
    h.frame(vec![]).shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(t) => t.galley.text() == needle,
        _ => false,
    })
}

/// R1: OK while the preview still renders (or with Preview off) never
/// renders on the UI thread: the dialog shows "Applying…" and the step is
/// recorded when the worker finishes — the same pixels, one undo step.
#[test]
fn remove_background_ok_never_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    let (id, sheet) = open_sheet(&mut h, dir.path());
    let full = PixelRect::new(0, 0, sheet.width(), sheet.height());
    for preview in [true, false] {
        assert!(h.app.open_remove_background());
        let key = h.app.remove_background_dialog().unwrap().key();
        h.app.set_remove_background(key, preview);
        assert_eq!(h.app.remove_background_busy(), preview);
        if preview {
            assert!(shows(&mut h, "Working…"), "busy is shown");
        }
        let params = h.app.remove_background_dialog().unwrap().params.clone();
        let steps = h.app.state.document(id).unwrap().undo_stack().undo_len();
        let t = Instant::now();
        assert!(h.app.remove_background_ok(), "OK accepted");
        let call = t.elapsed();
        if !cfg!(debug_assertions) {
            assert!(call < FRAME_LIMIT, "OK took {call:?}");
        }
        assert!(shows(&mut h, "Applying…"), "applying is shown");
        frames_until(&mut h, "apply", |h| {
            h.app.remove_background_dialog().is_none()
        });
        let expected = filters::render_filter(&sheet, None, full, &RemoveBackground, &params);
        let doc = &h.app.state.document(id).unwrap();
        assert_eq!(doc.undo_stack().undo_len(), steps + 1, "one step");
        assert_eq!(doc.undo_stack().undo_label(), Some("Remove Background"));
        assert!(
            layer(&h, id) == expected,
            "the rendered result (preview {preview})"
        );
        assert!(h.app.undo_current());
        h.frames(1);
    }
}

/// R1: Auto-Detect keys and detects on a worker with the same proposals.
#[test]
fn auto_detect_runs_off_the_ui_thread() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    let (_, sheet) = open_sheet(&mut h, dir.path());
    assert!(h.app.open_auto_detect());
    assert!(h.app.auto_detect_dialog().unwrap().busy());
    frames_until(&mut h, "auto-detect", |h| {
        !h.app.auto_detect_dialog().unwrap().busy()
    });
    let keyed = keying::key(&sheet, &KeyParams::auto(&sheet));
    let expected: Vec<PixelRect> = detect::detect(&keyed, &DetectParams::default())
        .iter()
        .map(|p| p.rect)
        .collect();
    let got: Vec<PixelRect> = h
        .app
        .auto_detect_dialog()
        .unwrap()
        .proposals
        .iter()
        .map(|p| p.rect)
        .collect();
    assert_eq!(got.len(), 64, "the 8 × 8 discs");
    assert_eq!(got, expected, "same proposals as the synchronous detection");
    // A parameter change re-detects on the worker too.
    let dialog = h.app.auto_detect_dialog_mut().unwrap();
    let mut params = dialog.params;
    params.merge_distance = 64;
    dialog.set_params(params);
    assert!(h.app.auto_detect_dialog().unwrap().busy());
    frames_until(&mut h, "re-detect", |h| {
        !h.app.auto_detect_dialog().unwrap().busy()
    });
    let expected: Vec<PixelRect> = detect::detect(&keyed, &params)
        .iter()
        .map(|p| p.rect)
        .collect();
    let got: Vec<PixelRect> = h
        .app
        .auto_detect_dialog()
        .unwrap()
        .proposals
        .iter()
        .map(|p| p.rect)
        .collect();
    assert_eq!(got, expected, "re-detected with the new parameters");
}

/// R3: cancelling Extract Frames returns within one short frame.
#[test]
fn extract_frames_cancels_without_blocking() {
    let mut h = Harness::new();
    h.app.open_path(&video("minute.mp4"));
    let id = *h.app.video_tabs().last().unwrap();
    h.frames(2);
    assert!(h.app.open_extract_dialog(id));
    h.app.extract_dialog_mut().unwrap().n = 1;
    assert!(h.app.start_extract());
    h.frames(3);
    let t = Instant::now();
    h.app.cancel_extract();
    h.frame(vec![]);
    let d = t.elapsed();
    eprintln!("extract frames cancel: {d:?}");
    if !cfg!(debug_assertions) {
        assert!(d < FRAME_LIMIT, "cancel took {d:?}");
    }
    assert!(h.app.extract_dialog().is_none());
}

/// R3: a MobileSAM download (from a slow local server) shows Cancel
/// Download; cancelling stops it and leaves no partial files.
#[test]
fn model_download_can_be_cancelled() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { break };
            std::thread::spawn(move || {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100000000\r\n\r\n");
                let chunk = vec![7u8; 16 * 1024];
                while s.write_all(&chunk).is_ok() {
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
        }
    });
    let models = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.set_segment_config(Config {
        runtime_paths: Vec::new(),
        model_dir: models.path().to_path_buf(),
        source: Source::Http(format!("http://127.0.0.1:{port}/")),
    });
    h.new_doc(64, 64);
    h.app.set_tool(Tool::ExtractObject);
    h.app.start_model_download();
    let parts = || {
        std::fs::read_dir(models.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
            .count()
    };
    frames_until(&mut h, "download starts", |_| parts() > 0);
    // The Cancel Download button.
    let out = h.frame(vec![]);
    let pos = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Cancel Download" => Some(t.pos),
            _ => None,
        })
        .expect("Cancel Download shown while downloading");
    h.click_at(pos + egui::Vec2::new(4.0, 4.0));
    assert!(!h.app.extract_strip().downloading, "download stopped");
    let start = Instant::now();
    while parts() > 0 {
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "partial file removed"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// R2 (release benchmark): Extract Assets on a 1-minute clip keeps every
/// UI frame under the limit, progress rises steadily, and Cancel ends the
/// run within 1 s.
#[test]
#[ignore]
fn bench_extract_assets_on_a_minute_keeps_the_ui_responsive() {
    let mut h = Harness::new();
    h.app.open_path(&video("minute.mp4"));
    let id = *h.app.video_tabs().last().unwrap();
    h.frames(2);
    // Full run.
    assert!(h.app.open_video_assets(id));
    assert!(h.app.start_video_assets());
    let mut last = 0.0f32;
    let t = Instant::now();
    let (worst, n) = frames_until(&mut h, "extract assets (1 minute)", |h| {
        match h.app.video_assets_dialog().and_then(|d| d.progress()) {
            Some(p) => {
                assert!(p >= last, "progress never goes back");
                last = p;
                false
            }
            None => true,
        }
    });
    eprintln!(
        "extract assets on 1 minute: {:.1} s, {n} frames, slowest {worst:?}",
        t.elapsed().as_secs_f32()
    );
    assert!(last > 0.95, "progress reached the end ({last})");
    assert!(h.app.video_assets_dialog().is_none(), "the review opened");
    // Cancel mid-run.
    assert!(h.app.open_video_assets(id));
    assert!(h.app.start_video_assets());
    frames_until(&mut h, "until 20 %", |h| {
        h.app
            .video_assets_dialog()
            .and_then(|d| d.progress())
            .is_none_or(|p| p > 0.2)
    });
    let exited = h
        .app
        .video_assets_dialog()
        .unwrap()
        .worker_exited()
        .unwrap();
    let t = Instant::now();
    h.app.cancel_video_assets();
    h.frame(vec![]);
    let d = t.elapsed();
    if !cfg!(debug_assertions) {
        assert!(d < FRAME_LIMIT, "cancel frame {d:?}");
    }
    while !exited.load(Ordering::Relaxed) {
        assert!(
            t.elapsed() < Duration::from_secs(1),
            "the run ended within 1 s"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    eprintln!(
        "extract assets cancel: worker exited after {:?}",
        t.elapsed()
    );
}
