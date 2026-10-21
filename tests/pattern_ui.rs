//! Find Repeating Pattern window, Filter ▸ Make Seamless, pattern assets, and
//! Tileable PNG export through the app.

use std::path::PathBuf;

use egui::Pos2;
use game_asset_creator::asset_library::{AssetLibrary, CategorySource, PATTERN};
use game_asset_creator::canvas::ViewAction;
use game_asset_creator::document::PixelRect;
use game_asset_creator::library::pack::ExportFormat;
use game_asset_creator::pattern::{self, SEAMLESS_RATIO};
use game_asset_creator::tools::Tool;
use game_asset_creator::DocId;
use image::RgbaImage;

mod common;
use common::Harness;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/patterns")
        .join(name)
}

fn has_text(out: &egui::FullOutput, needle: &str) -> bool {
    out.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(t) => t.galley.text().contains(needle),
        _ => false,
    })
}

fn text_pos(out: &egui::FullOutput, needle: &str) -> Option<Pos2> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text() == needle => Some(t.pos),
        _ => None,
    })
}

fn click_text(h: &mut Harness, needle: &str) {
    let out = h.frame(vec![]);
    let pos = text_pos(&out, needle).unwrap_or_else(|| panic!("no {needle:?} on screen"));
    h.click_at(pos + egui::Vec2::new(4.0, 4.0));
}

fn layer(h: &Harness, id: DocId) -> RgbaImage {
    let open = h.app.state.document(id).unwrap();
    open.document
        .layer(open.active_layer())
        .unwrap()
        .pixels
        .clone()
}

fn crop(img: &RgbaImage, r: PixelRect) -> RgbaImage {
    image::imageops::crop_imm(img, r.x, r.y, r.width, r.height).to_image()
}

/// Opens a fixture at Actual Size.
fn open(h: &mut Harness, name: &str) -> DocId {
    let id = h.app.open_path(&fixture(name)).unwrap();
    h.app.view_action(ViewAction::ActualSize);
    h.frames(2);
    id
}

/// The yellow tile outline on the canvas, in screen points.
fn outline(out: &egui::FullOutput) -> Option<egui::Rect> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Rect(r) if r.stroke.color == egui::Color32::from_rgb(255, 200, 0) => {
            Some(r.rect)
        }
        _ => None,
    })
}

#[test]
fn window_detects_outlines_previews_and_saves_a_pattern() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    let id = open(&mut h, "brick_synthetic.png");
    assert!(h.app.open_pattern());
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Find Repeating Pattern"));
    assert!(has_text(&out, "Period 40 × 32 px"), "detected period shown");
    assert!(has_text(&out, "3 × 3 tiled preview"));
    let d = h.app.pattern_dialog().unwrap();
    assert!(d.detection.found && d.detection.confidence >= pattern::MIN_CONFIDENCE);
    let tile = d.tile;
    assert_eq!((tile.width, tile.height), (40, 32));
    assert_eq!(d.region, PixelRect::new(0, 0, 200, 150), "whole layer");
    assert_eq!(d.shown_preview().unwrap(), (tile, false, [120, 96]));
    // Outlined on the canvas where the tile is.
    let r = outline(&out).expect("tile outline");
    let tl = h.screen_of(id, Pos2::new(tile.x as f32, tile.y as f32));
    let br = h.screen_of(id, Pos2::new(tile.right() as f32, tile.bottom() as f32));
    assert!(
        (r.min - tl).length() < 2.0 && (r.max - br).length() < 2.0,
        "{r:?}"
    );
    // Analysis does not touch the document.
    assert!(!h.app.state.document(id).unwrap().is_dirty());
    // Add to Library.
    click_text(&mut h, "Add to Library");
    let assets = h.app.state.assets.assets();
    assert_eq!(assets.len(), 1);
    let a = &assets[0];
    assert_eq!(a.category, PATTERN);
    assert_eq!(a.category_source, CategorySource::Manual);
    assert_eq!(a.origin.method, "pattern");
    assert_eq!(a.origin.params["period"], serde_json::json!([40, 32]));
    assert_eq!(a.origin.params["seamless"], false);
    assert_eq!(a.image, crop(&layer(&h, id), tile));
    assert!(pattern::seam_ratio(&a.image) <= SEAMLESS_RATIO);
    // Persisted with its category.
    let lib = AssetLibrary::open(dir.path());
    assert_eq!(lib.assets()[0].category, PATTERN);
}

#[test]
fn a_selection_without_repeats_proposes_the_whole_region() {
    let mut h = Harness::new();
    let id = open(&mut h, "stone_source.png");
    h.app.set_tool(Tool::RectSelect);
    h.frames(1);
    let a = h.screen_of(id, Pos2::new(20.0, 30.0));
    let b = h.screen_of(id, Pos2::new(120.0, 100.0));
    h.drag(a, b, 6);
    assert!(h.app.open_pattern());
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "No repeating pattern found"));
    let d = h.app.pattern_dialog().unwrap();
    assert_eq!(d.region, PixelRect::new(20, 30, 100, 70));
    assert_eq!(d.tile, d.region);
}

#[test]
fn manual_crop_drags_on_the_canvas_and_the_preview_follows() {
    let mut h = Harness::new();
    let id = open(&mut h, "brick_synthetic.png");
    h.app.set_tool(Tool::Brush);
    assert!(h.app.open_pattern());
    h.frames(2);
    h.app
        .pattern_dialog_mut()
        .unwrap()
        .set_tile(PixelRect::new(50, 40, 40, 32));
    click_text(&mut h, "Manual crop (drag on the canvas)");
    assert!(h.app.pattern_dialog().unwrap().manual);
    let before = layer(&h, id);
    // Move from the middle.
    let a = h.screen_of(id, Pos2::new(70.0, 56.0));
    let b = h.screen_of(id, Pos2::new(80.0, 62.0));
    h.drag(a, b, 5);
    let d = h.app.pattern_dialog().unwrap();
    assert_eq!(d.tile, PixelRect::new(60, 46, 40, 32));
    assert_eq!(d.shown_preview().unwrap().0, d.tile, "preview updated");
    // Resize from the right edge.
    let a = h.screen_of(id, Pos2::new(100.0, 60.0));
    let b = h.screen_of(id, Pos2::new(112.0, 60.0));
    h.drag(a, b, 4);
    assert_eq!(
        h.app.pattern_dialog().unwrap().tile,
        PixelRect::new(60, 46, 52, 32)
    );
    // Draw a new one outside, clamped to the layer.
    let a = h.screen_of(id, Pos2::new(150.0, 120.0));
    let b = h.screen_of(id, Pos2::new(260.0, 200.0));
    h.drag(a, b, 4);
    let d = h.app.pattern_dialog().unwrap();
    assert_eq!(d.tile, PixelRect::new(150, 120, 50, 30));
    assert_eq!(d.shown_preview().unwrap(), (d.tile, false, [150, 90]));
    assert_eq!(layer(&h, id), before, "manual crop never paints");
    // Typed values are clamped to the layer.
    h.app
        .pattern_dialog_mut()
        .unwrap()
        .set_tile(PixelRect::new(190, 10, 64, 64));
    h.frame(vec![]);
    assert_eq!(
        h.app.pattern_dialog().unwrap().tile,
        PixelRect::new(190, 10, 10, 64)
    );
    // Without manual crop the Brush paints again.
    click_text(&mut h, "Manual crop (drag on the canvas)");
    let a = h.screen_of(id, Pos2::new(20.0, 20.0));
    let b = h.screen_of(id, Pos2::new(40.0, 20.0));
    h.drag(a, b, 3);
    assert_ne!(layer(&h, id), before);
}

#[test]
fn make_seamless_and_revert_in_the_window() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    open(&mut h, "stone_source.png");
    assert!(h.app.open_pattern());
    h.frames(2);
    h.app
        .pattern_dialog_mut()
        .unwrap()
        .set_tile(PixelRect::new(13, 9, 97, 61));
    let raw = h.app.pattern_dialog().unwrap().raw_tile();
    assert!(pattern::seam_ratio(&raw) > SEAMLESS_RATIO);
    click_text(&mut h, "Make Seamless");
    h.frame(vec![]);
    let d = h.app.pattern_dialog().unwrap();
    let s = d.seamless.clone().expect("made seamless");
    assert_eq!(s.dimensions(), (97, 61));
    assert!(pattern::seam_ratio(&s) <= SEAMLESS_RATIO);
    assert!(d.shown_preview().unwrap().1, "preview shows it");
    click_text(&mut h, "Revert");
    assert!(h.app.pattern_dialog().unwrap().seamless.is_none());
    click_text(&mut h, "Make Seamless");
    click_text(&mut h, "Add to Library");
    let a = &h.app.state.assets.assets()[0];
    assert_eq!(a.image, s);
    assert_eq!(a.origin.params["seamless"], true);
    assert_eq!(a.origin.params["blend"], 25);
    click_text(&mut h, "Close");
    assert!(h.app.pattern_dialog().is_none());
}

#[test]
fn filter_make_seamless_on_layer_and_selection_with_undo() {
    let mut h = Harness::new();
    let id = open(&mut h, "brick_red_source.png");
    let original = layer(&h, id);
    assert!(pattern::seam_ratio(&original) > 0.0);
    // Whole layer: a raw crop-like layer becomes seamless.
    let steps = h.app.state.document(id).unwrap().undo_stack().undo_len();
    assert!(h.app.open_filter("make_seamless"));
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Make Seamless") && has_text(&out, "Blend width (%)"));
    assert!(h.app.filter_ok());
    let after = layer(&h, id);
    assert_eq!(after.dimensions(), original.dimensions());
    assert!(pattern::seam_ratio(&after) <= SEAMLESS_RATIO);
    assert_eq!(
        h.app.state.document(id).unwrap().undo_stack().undo_len(),
        steps + 1
    );
    assert!(h.app.undo_current());
    assert_eq!(layer(&h, id), original);
    // A selection: only its bounds change, and they become seamless.
    h.app.set_tool(Tool::RectSelect);
    h.frames(1);
    let a = h.screen_of(id, Pos2::new(10.0, 10.0));
    let b = h.screen_of(id, Pos2::new(110.0, 90.0));
    h.drag(a, b, 6);
    assert!(h.app.open_filter("make_seamless"));
    h.frames(2);
    assert!(h.app.filter_ok());
    let after = layer(&h, id);
    let r = PixelRect::new(10, 10, 100, 80);
    assert!(pattern::seam_ratio(&crop(&after, r)) <= SEAMLESS_RATIO);
    for (x, y, p) in after.enumerate_pixels() {
        if !((10..110).contains(&x) && (10..90).contains(&y)) {
            assert_eq!(p, original.get_pixel(x, y), "{x},{y} outside changed");
        }
    }
}

#[test]
fn tileable_png_export_repeats_and_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    open(&mut h, "brick_synthetic.png");
    assert!(h.app.open_pattern());
    h.frames(2);
    let id = h.app.pattern_add().unwrap();
    let tile = h.app.state.assets.get(id).unwrap().image.clone();
    let out = tempfile::tempdir().unwrap();
    let one = h
        .app
        .export_assets(
            &[id],
            ExportFormat::Tileable {
                columns: 1,
                rows: 1,
            },
            out.path(),
        )
        .unwrap();
    assert_eq!(image::open(&one[0]).unwrap().into_rgba8(), tile);
    let many = h
        .app
        .export_assets(
            &[id],
            ExportFormat::Tileable {
                columns: 3,
                rows: 2,
            },
            out.path(),
        )
        .unwrap();
    assert_ne!(one[0], many[0], "no overwrite");
    let img = image::open(&many[0]).unwrap().into_rgba8();
    assert_eq!(img.dimensions(), (3 * tile.width(), 2 * tile.height()));
    for (x, y, p) in img.enumerate_pixels() {
        assert_eq!(p, tile.get_pixel(x % tile.width(), y % tile.height()));
    }
    // The repeat count is limited to 16.
    let big = h
        .app
        .export_assets(
            &[id],
            ExportFormat::Tileable {
                columns: 40,
                rows: 1,
            },
            out.path(),
        )
        .unwrap();
    let img = image::open(&big[0]).unwrap();
    assert_eq!(img.width(), 16 * tile.width());
}

#[test]
fn pattern_category_is_added_once_to_older_libraries() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("library.json"),
        r#"{"version":1,"categories":["chalice","wall","crown","other"],"next_id":0,"counters":{},"assets":[]}"#,
    )
    .unwrap();
    let mut lib = AssetLibrary::open(dir.path());
    assert_eq!(
        lib.categories(),
        ["chalice", "wall", "crown", "pattern", "other"]
    );
    assert!(lib.remove_category(PATTERN));
    let lib = AssetLibrary::open(dir.path());
    assert!(
        !lib.categories().iter().any(|c| c == PATTERN),
        "removal sticks"
    );
    let fresh = AssetLibrary::open(&dir.path().join("new"));
    assert!(fresh.categories().iter().any(|c| c == PATTERN));
}

/// With the real model (`GAC_CLIP_DIR`): auto-tagging never proposes
/// "pattern", and a saved tile keeps its "pattern" category.
#[test]
fn auto_tagging_never_proposes_pattern() {
    use game_asset_creator::extract::models::Source;
    use game_asset_creator::extract::sam;
    use game_asset_creator::library::tagger::TagConfig;
    let Some(clip) = std::env::var_os("GAC_CLIP_DIR").map(PathBuf::from) else {
        eprintln!("skipped: set GAC_CLIP_DIR to the OpenCLIP model folder");
        return;
    };
    let runtime = sam::default_runtime_paths();
    if sam::runtime(&runtime).is_err() {
        eprintln!("skipped: ONNX Runtime not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.set_tag_config(TagConfig {
        runtime_paths: runtime,
        model_dir: clip,
        source: Source::Directory(PathBuf::from("/nonexistent")),
    });
    h.app.open_library(dir.path());
    let mut ids = Vec::new();
    for name in [
        "brick_red.png",
        "brick_grey.png",
        "cobblestone.png",
        "stone.png",
        "brick_synthetic.png",
    ] {
        open(&mut h, name);
        assert!(h.app.open_pattern());
        h.frames(1);
        ids.push(h.app.pattern_add().unwrap());
        h.app.pattern_close();
        // The same texture also as a plain (auto-tagged) asset.
        assert!(h.app.select_all());
        assert!(h.app.add_selection_to_library());
    }
    let start = std::time::Instant::now();
    while h
        .app
        .state
        .assets
        .assets()
        .iter()
        .any(|a| a.category_source == CategorySource::None)
    {
        assert!(start.elapsed().as_secs() < 60, "tagging timed out");
        std::thread::sleep(std::time::Duration::from_millis(5));
        h.frame(vec![]);
    }
    for a in h.app.state.assets.assets() {
        if ids.contains(&a.id) {
            assert_eq!(a.category, PATTERN, "{}", a.name);
        } else {
            assert_eq!(a.category_source, CategorySource::Auto);
            assert_ne!(a.category, PATTERN, "{}", a.name);
        }
    }
    // Even an explicit Auto-Tag of the tiles picks another category.
    h.app.queue_tagging(&ids, true);
    let start = std::time::Instant::now();
    while ids
        .iter()
        .any(|id| h.app.state.assets.get(*id).unwrap().category == PATTERN)
    {
        assert!(start.elapsed().as_secs() < 60, "re-tag timed out");
        std::thread::sleep(std::time::Duration::from_millis(5));
        h.frame(vec![]);
    }
}

/// Release build: the window's 3×3 preview of a 512×512 tile is rebuilt
/// within one frame of the tile changing.
#[test]
#[ignore]
fn bench_pattern_preview() {
    let mut h = Harness::new();
    let tile = image::open(fixture("stone_source.png"))
        .unwrap()
        .into_rgba8();
    let big = pattern::repeat(&tile, 4, 4);
    let path = tempfile::tempdir().unwrap();
    let file = path.path().join("big.png");
    big.save(&file).unwrap();
    h.app.open_path(&file).unwrap();
    h.frames(2);
    assert!(h.app.open_pattern());
    h.frames(2);
    let mut worst = 0.0f64;
    for i in 0..10 {
        h.app
            .pattern_dialog_mut()
            .unwrap()
            .set_tile(PixelRect::new(i * 7, i * 5, 512, 512));
        let t = std::time::Instant::now();
        h.frame(vec![]);
        worst = worst.max(t.elapsed().as_secs_f64() * 1e3);
        let d = h.app.pattern_dialog().unwrap();
        assert_eq!(d.shown_preview().unwrap(), (d.tile, false, [1536, 1536]));
    }
    eprintln!("pattern preview frame with a 512x512 tile: worst {worst:.1} ms");
}

/// Narrow tiles and selections (the first review crashed on 6 and 7 px).
#[test]
fn narrow_tiles_and_selections_are_made_seamless() {
    let mut h = Harness::new();
    let id = open(&mut h, "stone_source.png");
    assert!(h.app.open_pattern());
    h.frames(2);
    for w in 1..=8 {
        h.app
            .pattern_dialog_mut()
            .unwrap()
            .set_tile(PixelRect::new(30, 20, w, 40));
        click_text(&mut h, "Make Seamless");
        let d = h.app.pattern_dialog().unwrap();
        assert_eq!(d.tile_image().dimensions(), (w, 40));
        assert!(d.seamless.is_some());
    }
    click_text(&mut h, "Close");
    h.app.set_tool(Tool::RectSelect);
    h.frames(1);
    for w in [6.0, 7.0] {
        let a = h.screen_of(id, Pos2::new(10.0, 10.0));
        let b = h.screen_of(id, Pos2::new(10.0 + w, 50.0));
        h.drag(a, b, 4);
        assert!(h.app.open_filter("make_seamless"));
        h.frames(2);
        assert!(h.app.filter_ok());
        let after = crop(&layer(&h, id), PixelRect::new(10, 10, w as u32, 40));
        assert!(pattern::seam_ratio(&after).is_finite());
    }
}

#[test]
fn partial_brick_selection_reports_no_pattern() {
    let mut h = Harness::new();
    let id = open(&mut h, "brick_synthetic.png");
    h.app.set_tool(Tool::RectSelect);
    h.frames(1);
    let a = h.screen_of(id, Pos2::new(3.0, 2.0));
    let b = h.screen_of(id, Pos2::new(63.0, 50.0));
    h.drag(a, b, 6);
    assert!(h.app.open_pattern());
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "No repeating pattern found"));
    assert_eq!(
        h.app.pattern_dialog().unwrap().tile,
        PixelRect::new(3, 2, 60, 48)
    );
}

/// Small tiles move when dragged from inside at any zoom (the edge grab zone
/// never covers the whole tile), and their edges still resize.
#[test]
fn small_tiles_move_from_inside_at_every_zoom() {
    let mut h = Harness::new();
    let id = open(&mut h, "brick_synthetic.png");
    assert!(h.app.open_pattern());
    h.frames(2);
    h.app.pattern_dialog_mut().unwrap().manual = true;
    let zooms: [&[ViewAction]; 5] = [
        &[ViewAction::ActualSize],
        &[ViewAction::ActualSize, ViewAction::ZoomOut],
        &[
            ViewAction::ActualSize,
            ViewAction::ZoomOut,
            ViewAction::ZoomOut,
        ],
        &[ViewAction::ActualSize, ViewAction::ZoomIn],
        &[
            ViewAction::ActualSize,
            ViewAction::ZoomIn,
            ViewAction::ZoomIn,
        ],
    ];
    for steps in zooms {
        for a in steps {
            h.app.view_action(*a);
        }
        h.frames(2);
        let zoom = h.view(id).zoom;
        for s in [8u32, 16, 24, 32] {
            let start = PixelRect::new(60, 40, s, s);
            h.app.pattern_dialog_mut().unwrap().set_tile(start);
            let c = (60.0 + s as f32 / 2.0, 40.0 + s as f32 / 2.0);
            let a = h.screen_of(id, Pos2::new(c.0, c.1));
            let b = h.screen_of(id, Pos2::new(c.0 + 10.0, c.1 + 6.0));
            h.drag(a, b, 4);
            assert_eq!(
                h.app.pattern_dialog().unwrap().tile,
                PixelRect::new(70, 46, s, s),
                "zoom {zoom}, {s} px tile moved"
            );
            // The right edge still resizes.
            let a = h.screen_of(id, Pos2::new(70.0 + s as f32, 46.0 + s as f32 / 2.0));
            let b = h.screen_of(id, Pos2::new(74.0 + s as f32, 46.0 + s as f32 / 2.0));
            h.drag(a, b, 4);
            assert_eq!(
                h.app.pattern_dialog().unwrap().tile,
                PixelRect::new(70, 46, s + 4, s),
                "zoom {zoom}, {s} px tile resized"
            );
        }
    }
}
