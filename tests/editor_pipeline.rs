//! The editor ↔ pipeline loop: Edit a Copy / Save Back with versions,
//! extraction from the canvas, dragging assets onto canvases, Clean Up, and
//! the full video → library → composition loop.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Pos2, Vec2};
use game_asset_creator::asset_library::AssetLibrary;
use game_asset_creator::canvas::{CanvasEvent, ViewAction};
use game_asset_creator::document::{Layer, PixelRect};
use game_asset_creator::extract::models::Source;
use game_asset_creator::extract::segment::Config;
use game_asset_creator::layers;
use game_asset_creator::library_ui::LibraryAction;
use game_asset_creator::tools::Tool;
use game_asset_creator::ui::Panel;
use game_asset_creator::DocId;
use image::{Rgba, RgbaImage};

mod common;
use common::Harness;

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path)
}

fn wait_until(h: &mut Harness, what: &str, mut done: impl FnMut(&Harness) -> bool) {
    let start = Instant::now();
    while !done(h) {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(2));
        h.frame(vec![]);
    }
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
    h.click_at(pos + Vec2::new(4.0, 4.0));
}

/// Where to press to drag the library thumbnail of `name` (the library
/// panel is below the Layers panel, which may list a layer of that name).
fn thumbnail(out: &egui::FullOutput, name: &str) -> Pos2 {
    out.shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == name => Some(t.pos),
            _ => None,
        })
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .expect("thumbnail")
        + Vec2::new(4.0, -20.0)
}

/// Opens a menu-bar menu and clicks one of its items.
fn menu_item(h: &mut Harness, menu: &str, item: &str) {
    for _ in 0..3 {
        click_text(h, menu);
        h.frame(vec![]);
        let out = h.frame(vec![]);
        // The item in the popup under the menu title (other panels may show
        // a button with the same text).
        let title = text_pos(&out, menu).unwrap();
        let found = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == item => Some(t.pos),
                _ => None,
            })
            .filter(|p| p.y > title.y && (p.x - title.x).abs() < 120.0)
            .min_by(|a, b| a.y.total_cmp(&b.y));
        if let Some(pos) = found {
            h.click_at(pos + Vec2::new(4.0, 4.0));
            return;
        }
    }
    panic!("no {item:?} in the {menu} menu");
}

/// A sprite: a filled red rectangle with a dark outline on transparency.
fn sprite(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
            Rgba([20, 10, 10, 255])
        } else {
            Rgba([200, (x * 5) as u8, (y * 7) as u8, 255])
        }
    })
}

fn grabcut_only() -> Config {
    Config {
        runtime_paths: Vec::new(),
        model_dir: std::env::temp_dir().join("gac-no-models"),
        source: Source::Directory(std::env::temp_dir().join("gac-no-models")),
    }
}

fn active_layer(h: &Harness, id: DocId) -> RgbaImage {
    let open = h.app.state.document(id).unwrap();
    open.document
        .layer(open.active_layer())
        .unwrap()
        .pixels
        .clone()
}

fn save_png(dir: &Path, name: &str, img: &RgbaImage) -> PathBuf {
    let p = dir.join(name);
    img.save(&p).unwrap();
    p
}

fn open_doc(h: &mut Harness, path: &Path) -> DocId {
    let id = h.app.open_path(path).unwrap();
    h.app.view_action(ViewAction::ActualSize);
    h.frames(2);
    id
}

/// Library with one single asset, the panel shown.
fn library_with_sprite(h: &mut Harness, dir: &Path) -> u64 {
    h.app.open_library(dir);
    let id = h
        .app
        .state
        .assets
        .add("art.png", sprite(24, 16), PixelRect::new(0, 0, 24, 16));
    h.app.set_panel_open(Panel::AssetLibrary, true);
    h.frames(2);
    id
}

// --- Versions (library level) ---------------------------------------------------

#[test]
fn versions_are_stored_restored_persisted_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = AssetLibrary::open(dir.path());
    let id = lib.add("a.png", sprite(10, 8), PixelRect::new(0, 0, 10, 8));
    assert_eq!(lib.get(id).unwrap().version, 1);
    let v2 = RgbaImage::from_pixel(12, 9, Rgba([0, 255, 0, 255]));
    assert_eq!(lib.set_image(id, v2.clone(), Vec::new()), Some(2));
    assert_eq!(lib.get(id).unwrap().image, v2);
    assert_eq!(lib.restore_version(id, 1), Some(3));
    assert_eq!(lib.get(id).unwrap().image, sprite(10, 8));
    // Survives a restart with every version readable.
    let lib = AssetLibrary::open(dir.path());
    let a = lib.get(id).unwrap();
    assert_eq!(a.version, 3);
    assert_eq!(
        a.versions.iter().map(|v| v.version).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(a.versions[1].width, 12);
    assert_eq!(lib.version_images(id, 2).unwrap(), vec![v2]);
    assert_eq!(lib.version_images(id, 1).unwrap(), vec![sprite(10, 8)]);
    let files: Vec<PathBuf> = a
        .versions
        .iter()
        .flat_map(|v| v.files.iter().map(|f| dir.path().join("assets").join(f)))
        .collect();
    assert!(files.iter().all(|f| f.exists()));
    // Remove deletes every version.
    let mut lib = lib;
    lib.remove(&[id]);
    assert!(files.iter().all(|f| !f.exists()));
}

#[test]
fn older_libraries_load_as_version_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("assets")).unwrap();
    sprite(6, 5).save(dir.path().join("assets/1.png")).unwrap();
    std::fs::write(
        dir.path().join("library.json"),
        r#"{"version":2,"categories":["other"],"next_id":1,"counters":{},"assets":[
        {"id":1,"name":"old","kind":"single","files":["1.png"],"width":6,"height":5,
         "category":"other","category_source":"none","confidence":null,"tags":[],
         "source":{"document":"d","file":null,"frame":null,"rect":[0,0,6,5]},
         "extraction":{"method":"selection","params":null},"created":5}]}"#,
    )
    .unwrap();
    let mut lib = AssetLibrary::open(dir.path());
    assert!(lib.notices().is_empty(), "{:?}", lib.notices());
    let a = lib.get(1).unwrap();
    assert_eq!((a.version, a.versions.len()), (1, 1));
    assert_eq!(a.versions[0].files, ["1.png"]);
    assert_eq!(lib.set_image(1, sprite(3, 3), Vec::new()), Some(2));
    assert!(dir.path().join("assets/1.v2.png").exists());
    assert!(dir.path().join("assets/1.png").exists(), "version 1 kept");
}

#[test]
fn animations_get_versions_of_all_frames() {
    let dir = tempfile::tempdir().unwrap();
    let mut lib = AssetLibrary::open(dir.path());
    let frames: Vec<RgbaImage> = (0..3).map(|i| sprite(8 + i, 8)).collect();
    let id = lib.add_animation("walk.mp4", frames.clone(), PixelRect::new(0, 0, 10, 8));
    let edited: Vec<RgbaImage> = (0..4).map(|i| sprite(9, 9 + i)).collect();
    assert_eq!(
        lib.set_image(id, edited[0].clone(), edited.clone()),
        Some(2)
    );
    let lib = AssetLibrary::open(dir.path());
    assert_eq!(lib.get(id).unwrap().frames, edited);
    assert_eq!(lib.version_images(id, 1).unwrap(), frames);
}

// --- Clean Up --------------------------------------------------------------------

/// A sprite in a transparent border, with a stray speck and faint alpha.
fn messy() -> RgbaImage {
    let mut img = RgbaImage::new(60, 40);
    image::imageops::replace(&mut img, &sprite(20, 12), 15, 10);
    img.put_pixel(2, 2, Rgba([255, 255, 255, 255]));
    img.put_pixel(50, 35, Rgba([10, 10, 10, 4]));
    img
}

#[test]
fn library_clean_up_crops_and_despeckles_as_a_new_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    let id = h
        .app
        .state
        .assets
        .add("m.png", messy(), PixelRect::new(0, 0, 60, 40));
    let clean = h
        .app
        .state
        .assets
        .add("c.png", sprite(9, 9), PixelRect::new(0, 0, 9, 9));
    h.app.set_panel_open(Panel::AssetLibrary, true);
    h.frames(2);
    h.app
        .library_action(LibraryAction::CleanUp(vec![id, clean]));
    let a = h.app.state.assets.get(id).unwrap();
    assert_eq!(a.version, 2);
    assert_eq!(a.image, sprite(20, 12));
    assert_eq!(
        h.app.state.assets.get(clean).unwrap().version,
        1,
        "unchanged"
    );
    // The panel button does the same for the selection.
    let id2 = h
        .app
        .state
        .assets
        .add("m2.png", messy(), PixelRect::new(0, 0, 60, 40));
    h.frames(2);
    let name = h.app.state.assets.get(id2).unwrap().name.clone();
    click_text(&mut h, &name);
    click_text(&mut h, "Clean Up");
    assert_eq!(h.app.state.assets.get(id2).unwrap().image, sprite(20, 12));
}

#[test]
fn image_clean_up_crops_the_canvas_in_one_undo_step() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    let id = open_doc(&mut h, &save_png(tmp.path(), "messy.png", &messy()));
    let steps = h.app.state.document(id).unwrap().undo_stack().undo_len();
    menu_item(&mut h, "Image", "Clean Up");
    let open = h.app.state.document(id).unwrap();
    assert_eq!((open.document.width(), open.document.height()), (20, 12));
    assert_eq!(active_layer(&h, id), sprite(20, 12));
    assert_eq!(open.undo_stack().undo_len(), steps + 1);
    assert_eq!(open.undo_stack().undo_label(), Some("Clean Up"));
    assert_eq!(open.document.name, "messy.png", "name kept");
    h.frames(2);
    assert!(h.app.undo_current());
    let open = h.app.state.document(id).unwrap();
    assert_eq!((open.document.width(), open.document.height()), (60, 40));
    assert_eq!(active_layer(&h, id), messy());
    // Already clean: nothing happens.
    assert!(h.app.clean_up_document());
    assert!(!h.app.clean_up_document());
}

// --- Edit a Copy / Save Back ------------------------------------------------------

#[test]
fn edit_a_copy_and_save_back_make_a_new_version() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    let asset = library_with_sprite(&mut h, dir.path());
    click_text(&mut h, "art 1");
    click_text(&mut h, "Edit a Copy");
    let doc = *h.app.document_tabs().last().unwrap();
    assert_eq!(h.app.library_link(doc), Some(asset));
    let open = h.app.state.document(doc).unwrap();
    assert_eq!(open.document.name, "art 1 (library)");
    assert!(!open.is_dirty());
    // Edit: paint a stroke.
    h.app.view_action(ViewAction::ActualSize);
    h.app.set_tool(Tool::Brush);
    h.app.colors_mut().foreground = [0, 0, 255, 255];
    h.frames(2);
    let a = h.screen_of(doc, Pos2::new(3.0, 8.0));
    let b = h.screen_of(doc, Pos2::new(20.0, 8.0));
    h.drag(a, b, 4);
    assert!(h.app.state.document(doc).unwrap().is_dirty());
    let edited = h.app.state.document(doc).unwrap().document.composite();
    assert_ne!(edited, sprite(24, 16));
    // File ▸ Save Back to Library.
    menu_item(&mut h, "File", "Save Back to Library");
    let a = h.app.state.assets.get(asset).unwrap();
    assert_eq!(a.version, 2);
    assert_eq!(a.image, edited);
    assert!(!h.app.state.document(doc).unwrap().is_dirty());
    // The panel's thumbnail is rebuilt from version 2 on the next frame.
    assert_eq!(h.app.asset_view().thumbnail_version(asset), Some(1));
    h.frame(vec![]);
    assert_eq!(h.app.asset_view().thumbnail_version(asset), Some(2));
    // Versions… shows both, and Restore brings version 1 back as version 3.
    click_text(&mut h, "Versions…");
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Version 2 (current)") && has_text(&out, "Version 1"));
    // Newest first: the enabled Restore is version 1's (the last one).
    let out = h.frame(vec![]);
    let restore = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Restore" => Some(t.pos),
            _ => None,
        })
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    h.click_at(restore + Vec2::new(4.0, 4.0));
    let a = h.app.state.assets.get(asset).unwrap();
    assert_eq!((a.version, &a.image), (3, &sprite(24, 16)));
    // Persisted.
    let lib = AssetLibrary::open(dir.path());
    assert_eq!(lib.version_images(asset, 2).unwrap(), vec![edited]);
}

#[test]
fn save_back_after_the_asset_was_removed_adds_a_new_asset() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    let asset = library_with_sprite(&mut h, dir.path());
    let doc = h.app.library_action(LibraryAction::Open(asset)).unwrap();
    h.app.state.assets.remove(&[asset]);
    let new = h.app.save_back_to_library().unwrap();
    assert_ne!(new, asset);
    let a = h.app.state.assets.get(new).unwrap();
    assert_eq!(a.name, "art 1");
    assert_eq!(a.image, sprite(24, 16));
    assert_eq!(h.app.library_link(doc), Some(new));
}

#[test]
fn animation_copies_save_one_frame_per_layer() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    let frames: Vec<RgbaImage> = (0..3).map(|_| sprite(10, 10)).collect();
    let asset = h
        .app
        .state
        .assets
        .add_animation("walk.mp4", frames, PixelRect::new(0, 0, 10, 10));
    let doc = h.app.library_action(LibraryAction::Open(asset)).unwrap();
    assert_eq!(h.app.state.document(doc).unwrap().document.layer_count(), 3);
    // Hide a layer and add a fourth: frames follow the layers, visible or not.
    let extra = Layer::new(
        "Frame 4",
        RgbaImage::from_pixel(10, 10, Rgba([1, 2, 3, 255])),
    );
    h.app
        .state
        .execute(doc, Box::new(layers::insert_layer("Add", 3, extra)))
        .unwrap();
    h.app.save_back_to_library().unwrap();
    let a = h.app.state.assets.get(asset).unwrap();
    assert_eq!(a.frames.len(), 4);
    assert_eq!(a.frames[3].get_pixel(0, 0).0, [1, 2, 3, 255]);
    assert_eq!(a.version, 2);
}

// --- Extraction from the canvas -------------------------------------------------

/// Two layers: a magenta background with sprite A, and a transparent layer
/// with sprite B. Returns the document and A's / B's rectangles.
fn two_layer_doc(h: &mut Harness, tmp: &Path) -> (DocId, PixelRect, PixelRect) {
    let mut bottom = RgbaImage::from_pixel(200, 120, Rgba([255, 0, 255, 255]));
    image::imageops::replace(&mut bottom, &sprite(30, 20), 20, 30);
    let id = open_doc(h, &save_png(tmp, "two.png", &bottom));
    let mut top = RgbaImage::new(200, 120);
    image::imageops::replace(&mut top, &sprite(25, 25), 130, 60);
    h.app
        .state
        .execute(
            id,
            Box::new(layers::insert_layer("Add", 1, Layer::new("B", top))),
        )
        .unwrap();
    h.frames(2);
    (
        id,
        PixelRect::new(20, 30, 30, 20),
        PixelRect::new(130, 60, 25, 25),
    )
}

fn select(h: &mut Harness, id: DocId, r: PixelRect) {
    h.app.set_tool(Tool::RectSelect);
    h.frames(1);
    let a = h.screen_of(id, Pos2::new(r.x as f32, r.y as f32));
    let b = h.screen_of(id, Pos2::new(r.right() as f32, r.bottom() as f32));
    h.drag(a, b, 5);
}

fn detected(h: &Harness) -> Vec<PixelRect> {
    let mut v: Vec<PixelRect> = h
        .app
        .auto_detect_dialog()
        .unwrap()
        .proposals
        .iter()
        .map(|p| p.rect)
        .collect();
    v.sort_by_key(|r| r.x);
    v
}

#[test]
fn auto_detect_reads_the_canvas_within_the_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    let (id, a, b) = two_layer_doc(&mut h, tmp.path());
    // Canvas (default): both sprites, from both layers.
    assert!(h.app.open_auto_detect());
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Source") && has_text(&out, "Canvas") && has_text(&out, "Active layer"));
    assert_eq!(detected(&h), vec![a, b]);
    // Active layer: only the top layer's sprite.
    click_text(&mut h, "Active layer");
    assert!(!h.app.auto_detect_dialog().unwrap().canvas);
    assert_eq!(detected(&h), vec![b]);
    click_text(&mut h, "Canvas");
    h.app.auto_detect_cancel();
    // A selection around A (part of B too): only what is inside counts.
    let sel = PixelRect::new(10, 20, 130, 60);
    select(&mut h, id, sel);
    assert!(h.app.open_auto_detect());
    let found = detected(&h);
    assert_eq!(found[0], a);
    for r in &found {
        assert!(
            r.x >= sel.x && r.right() <= sel.right() && r.y >= sel.y && r.bottom() <= sel.bottom()
        );
    }
    assert_eq!(found.len(), 2, "A and the selected corner of B");
    assert!(h.app.auto_detect_add() == 2);
    let params = &h.app.state.assets.assets()[0].origin.params;
    assert_eq!(params["source"], "canvas");
    assert_eq!(params["region"], serde_json::json!([10, 20, 130, 60]));
}

#[test]
fn remove_background_from_the_canvas_adds_a_layer_in_one_step() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    let (id, a, b) = two_layer_doc(&mut h, tmp.path());
    let before = h.app.state.document(id).unwrap().document.clone();
    let steps = h.app.state.document(id).unwrap().undo_stack().undo_len();
    // Cancel leaves no trace.
    assert!(h.app.open_remove_background());
    h.frames(2);
    assert_eq!(
        h.app.state.document(id).unwrap().document.layer_count(),
        3,
        "preview layer"
    );
    h.app.remove_background_cancel();
    let open = h.app.state.document(id).unwrap();
    assert_eq!(open.document.layers(), before.layers());
    assert_eq!(open.document.active_layer(), before.active_layer());
    assert_eq!(
        (open.undo_stack().undo_len(), open.undo_stack().redo_len()),
        (steps, 0)
    );
    // OK: one step adding "Background removed" with both sprites keyed out of
    // the magenta.
    assert!(h.app.open_remove_background());
    assert_eq!(
        h.app.remove_background_dialog().unwrap().key().color,
        [255, 0, 255]
    );
    assert!(h.app.remove_background_ok());
    let open = h.app.state.document(id).unwrap();
    assert_eq!(open.undo_stack().undo_len(), steps + 1);
    assert_eq!(open.undo_stack().undo_label(), Some("Remove Background"));
    assert_eq!(open.document.layer_count(), 3);
    let layer = open.document.layer(open.document.active_layer()).unwrap();
    assert_eq!(layer.name, "Background removed");
    assert_eq!(layer.pixels.get_pixel(0, 0)[3], 0, "background keyed");
    assert_eq!(layer.pixels.get_pixel(a.x + 3, a.y + 3)[3], 255);
    assert_eq!(layer.pixels.get_pixel(b.x + 3, b.y + 3)[3], 255);
    assert_eq!(
        open.document.layer(0).unwrap().pixels,
        before.layer(0).unwrap().pixels
    );
    assert!(h.app.undo_current());
    assert_eq!(
        h.app.state.document(id).unwrap().document.layers(),
        before.layers()
    );
    // With a selection: nothing outside it.
    select(&mut h, id, PixelRect::new(10, 20, 60, 50));
    assert!(h.app.open_remove_background());
    assert!(h.app.remove_background_ok());
    let open = h.app.state.document(id).unwrap();
    let layer = open.document.layer(open.document.active_layer()).unwrap();
    assert!(layer
        .pixels
        .enumerate_pixels()
        .all(|(x, y, p)| p[3] == 0 || ((10..70).contains(&x) && (20..70).contains(&y))));
    assert_eq!(layer.pixels.get_pixel(a.x + 3, a.y + 3)[3], 255);
}

#[test]
fn extract_object_segments_the_canvas_inside_the_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    h.app.set_segment_config(grabcut_only());
    let (id, _a, b) = two_layer_doc(&mut h, tmp.path());
    // Select the left part of B only; box the whole of B.
    let sel = PixelRect::new(120, 50, 22, 45);
    select(&mut h, id, sel);
    h.app.set_tool(Tool::ExtractObject);
    h.frames(1);
    assert!(h.app.extract_prompt(CanvasEvent::ExtractBox {
        start: (b.x as f32 - 6.0, b.y as f32 - 6.0),
        end: (b.right() as f32 + 6.0, b.bottom() as f32 + 6.0),
    }));
    wait_until(&mut h, "mask", |h| {
        h.app.extract_session().is_some_and(|s| s.mask.is_some())
    });
    let session = h.app.extract_session().unwrap();
    assert!(session.canvas);
    let (mask, _) = session.mask.as_ref().unwrap();
    assert!(mask
        .enumerate_pixels()
        .all(|(x, y, m)| m[0] == 0 || ((120..142).contains(&x) && (50..95).contains(&y))));
    assert!(h.app.extract_accept());
    let asset = &h.app.state.assets.assets()[0];
    assert!(asset.image.width() <= 22 && asset.rect.x >= 120);
    assert_eq!(asset.origin.params["source"], "canvas");
    assert_eq!(
        asset.origin.params["region"],
        serde_json::json!([120, 50, 22, 45])
    );
    // The tool options offer the Active layer source too.
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Source") && has_text(&out, "Active layer"));
    h.app.tool_options_mut().extract_from_canvas = false;
    assert!(h.app.extract_prompt(CanvasEvent::ExtractBox {
        start: (b.x as f32 - 6.0, b.y as f32 - 6.0),
        end: (b.right() as f32 + 6.0, b.bottom() as f32 + 6.0),
    }));
    wait_until(&mut h, "layer mask", |h| {
        h.app
            .extract_session()
            .is_some_and(|s| !s.canvas && s.mask.is_some())
    });
    assert!(h.app.extract_accept());
    let asset = h.app.state.assets.assets().last().unwrap();
    assert_eq!(asset.origin.params["source"], "layer");
}

// --- Drag and drop ---------------------------------------------------------------

#[test]
fn dragging_a_thumbnail_onto_a_canvas_adds_a_layer() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    let asset = library_with_sprite(&mut h, dir.path());
    let doc = h.app.create_document(120, 80, 72.0).unwrap();
    h.app.view_action(ViewAction::ActualSize);
    h.frames(3);
    let steps = h.app.state.document(doc).unwrap().undo_stack().undo_len();
    let out = h.frame(vec![]);
    let from = thumbnail(&out, "art 1");
    let to = h.screen_of(doc, Pos2::new(60.0, 40.0));
    h.drag(from, to, 8);
    let open = h.app.state.document(doc).unwrap();
    assert_eq!(open.document.layer_count(), 2);
    assert_eq!(open.undo_stack().undo_len(), steps + 1);
    let layer = open.document.layer(open.document.active_layer()).unwrap();
    assert_eq!(layer.name, "art 1");
    // Centred on the drop point: 24×16 → top-left (48, 32).
    let placed = image::imageops::crop_imm(&layer.pixels, 48, 32, 24, 16).to_image();
    assert_eq!(placed, h.app.state.assets.get(asset).unwrap().image);
    assert_eq!(layer.pixels.get_pixel(47, 32)[3], 0);
    // Near the edge: clipped to the canvas.
    let to = h.screen_of(doc, Pos2::new(118.0, 2.0));
    let out = h.frame(vec![]);
    let from = thumbnail(&out, "art 1");
    h.drag(from, to, 8);
    let open = h.app.state.document(doc).unwrap();
    assert_eq!(open.document.layer_count(), 3);
    let layer = open.document.layer(open.document.active_layer()).unwrap();
    assert_eq!(layer.pixels.dimensions(), (120, 80));
    assert_eq!(
        *layer.pixels.get_pixel(119, 0),
        sprite(24, 16).get_pixel(13, 6).clone()
    );
    // Dropped back on the panel: nothing.
    let out = h.frame(vec![]);
    let from = thumbnail(&out, "art 1");
    h.drag(from, from + Vec2::new(30.0, 0.0), 6);
    assert_eq!(h.app.state.document(doc).unwrap().document.layer_count(), 3);
}

// --- Full loop ----------------------------------------------------------------------

/// Open a video, grab a frame, extract the skeleton, edit it, save it back,
/// and drag it into a new composition.
#[test]
fn full_loop_video_to_composition() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    h.app.set_segment_config(grabcut_only());
    // 1. Open the video and grab frame 0.
    assert_eq!(h.app.open_path(&fixture("video/skeleton.mp4")), None);
    let video = *h.app.video_tabs().last().unwrap();
    wait_until(&mut h, "frame 0", |h| {
        h.app
            .video(video)
            .unwrap()
            .displayed()
            .is_some_and(|d| d.exact && d.index == 0)
    });
    assert!(h.app.grab_frame(video));
    wait_until(&mut h, "grabbed document", |h| {
        !h.app.document_tabs().is_empty()
    });
    let frame = *h.app.document_tabs().last().unwrap();
    assert_eq!(h.app.state.current_id(), Some(frame));
    h.app.view_action(ViewAction::ActualSize);
    h.frames(2);
    // 2. Extract the skeleton (drawn at 40,64, 96×112) with Auto-Detect.
    assert!(h.app.open_auto_detect());
    let found: Vec<PixelRect> = h
        .app
        .auto_detect_dialog()
        .unwrap()
        .proposals
        .iter()
        .map(|p| p.rect)
        .collect();
    assert_eq!(found, vec![PixelRect::new(40, 64, 96, 112)]);
    assert_eq!(h.app.auto_detect_add(), 1);
    let asset = h.app.state.assets.assets()[0].id;
    let extracted = h.app.state.assets.get(asset).unwrap().image.clone();
    let (w, hgt) = extracted.dimensions();
    assert!(
        (90..=102).contains(&w) && (106..=118).contains(&hgt),
        "{w}×{hgt}"
    );
    // 3. Edit a copy: paint a stroke across the skull.
    let name = h.app.state.assets.get(asset).unwrap().name.clone();
    let doc = h.app.library_action(LibraryAction::Open(asset)).unwrap();
    h.app.view_action(ViewAction::ActualSize);
    h.app.set_tool(Tool::Brush);
    h.app.colors_mut().foreground = [255, 0, 0, 255];
    h.frames(2);
    let a = h.screen_of(doc, Pos2::new(w as f32 * 0.3, 12.0));
    let b = h.screen_of(doc, Pos2::new(w as f32 * 0.7, 12.0));
    h.drag(a, b, 5);
    let edited = h.app.state.document(doc).unwrap().document.composite();
    assert_ne!(edited, extracted);
    // 4. Save it back: version 2.
    menu_item(&mut h, "File", "Save Back to Library");
    let saved = h.app.state.assets.get(asset).unwrap();
    assert_eq!(saved.version, 2);
    assert_eq!(saved.image, edited);
    // 5. A new composition, and the asset dragged onto it.
    h.app.set_tool(Tool::Move);
    let comp = h.app.create_document(320, 240, 72.0).unwrap();
    h.app.view_action(ViewAction::ActualSize);
    h.app.set_panel_open(Panel::AssetLibrary, true);
    h.frames(3);
    let out = h.frame(vec![]);
    let from = thumbnail(&out, &name);
    let to = h.screen_of(comp, Pos2::new(160.0, 120.0));
    h.drag(from, to, 8);
    let open = h.app.state.document(comp).unwrap();
    assert_eq!(open.document.layer_count(), 2);
    let layer = open.document.layer(open.document.active_layer()).unwrap();
    let x = (160.0 - w as f32 / 2.0).round() as u32;
    let y = (120.0 - hgt as f32 / 2.0).round() as u32;
    let placed = image::imageops::crop_imm(&layer.pixels, x, y, w, hgt).to_image();
    assert_eq!(placed, edited, "the edited skeleton, pixel for pixel");
}

#[test]
fn dropping_an_animation_adds_its_first_frame() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    let frames: Vec<RgbaImage> = (0..3)
        .map(|i| RgbaImage::from_pixel(10, 10, Rgba([50 * i as u8, 0, 0, 255])))
        .collect();
    let asset =
        h.app
            .state
            .assets
            .add_animation("walk.mp4", frames.clone(), PixelRect::new(0, 0, 10, 10));
    h.app.set_panel_open(Panel::AssetLibrary, true);
    let name = h.app.state.assets.get(asset).unwrap().name.clone();
    let doc = h.app.create_document(60, 40, 72.0).unwrap();
    h.app.view_action(ViewAction::ActualSize);
    h.frames(3);
    let out = h.frame(vec![]);
    let to = h.screen_of(doc, Pos2::new(30.0, 20.0));
    h.drag(thumbnail(&out, &name), to, 8);
    let open = h.app.state.document(doc).unwrap();
    assert_eq!(open.document.layer_count(), 2, "one layer");
    let layer = open.document.layer(open.document.active_layer()).unwrap();
    let placed = image::imageops::crop_imm(&layer.pixels, 25, 15, 10, 10).to_image();
    assert_eq!(placed, frames[0]);
}

// --- Regressions from the first review ------------------------------------------

#[test]
fn save_back_of_a_removed_animation_keeps_its_frames() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    let frames: Vec<RgbaImage> = (0..2)
        .map(|i| RgbaImage::from_pixel(8, 8, Rgba([100 + i as u8, 0, 0, 255])))
        .collect();
    let asset =
        h.app
            .state
            .assets
            .add_animation("walk.mp4", frames.clone(), PixelRect::new(0, 0, 8, 8));
    let doc = h.app.library_action(LibraryAction::Open(asset)).unwrap();
    h.app.state.assets.remove(&[asset]);
    let new = h.app.save_back_to_library().unwrap();
    let a = h.app.state.assets.get(new).unwrap();
    assert!(a.is_animation());
    assert_eq!(a.frames, frames);
    assert_eq!(h.app.library_link(doc), Some(new));
}

#[test]
fn the_remove_background_preview_layer_never_leaks() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    let (id, _, _) = two_layer_doc(&mut h, tmp.path());
    // Link the document to an asset so Save Back is available.
    let asset = h
        .app
        .state
        .assets
        .add("x.png", sprite(4, 4), PixelRect::new(0, 0, 4, 4));
    let doc = h.app.library_action(LibraryAction::Open(asset)).unwrap();
    let before_doc = h.app.state.document(doc).unwrap().document.clone();
    h.app.state.set_current(doc);
    h.frames(2);
    assert!(h.app.can_save_back(doc));
    assert!(h.app.open_remove_background());
    h.frames(2);
    assert!(!h.app.can_save_back(doc), "no Save Back during the preview");
    assert!(h.app.save_back_to_library().is_none());
    assert_eq!(h.app.state.assets.get(asset).unwrap().version, 1);
    // Layer edits are refused while the preview layer is there.
    let steps = h.app.state.document(doc).unwrap().undo_stack().undo_len();
    h.app
        .layer_action(game_asset_creator::layers_panel::LayerAction::Op(
            game_asset_creator::layers_panel::LayerOp::Duplicate,
        ));
    h.app
        .layer_action(game_asset_creator::layers_panel::LayerAction::ToggleVisible(0));
    h.app
        .layer_action(game_asset_creator::layers_panel::LayerAction::OpacityPreview(0.5));
    h.app
        .layer_action(game_asset_creator::layers_panel::LayerAction::OpacityCommit);
    let dup = layers::duplicate_layer(&h.app.state.document(doc).unwrap().document).unwrap();
    assert!(h.app.state.execute(doc, Box::new(dup)).is_err());
    assert_eq!(
        h.app.state.document(doc).unwrap().undo_stack().undo_len(),
        steps
    );
    h.app.remove_background_cancel();
    let open = h.app.state.document(doc).unwrap();
    assert_eq!(open.document.layers(), before_doc.layers());
    assert!(h.app.can_save_back(doc));
    assert_ne!(id, doc);
}

#[test]
fn drops_outside_the_image_do_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    library_with_sprite(&mut h, dir.path());
    let doc = h.app.create_document(60, 40, 72.0).unwrap();
    h.app.view_action(ViewAction::ActualSize);
    h.frames(3);
    let out = h.frame(vec![]);
    // Grey area left of the image, still inside the canvas view.
    let to = h.screen_of(doc, Pos2::new(-30.0, 20.0));
    assert!(h.app.canvas(doc).unwrap().rect().unwrap().contains(to));
    h.drag(thumbnail(&out, "art 1"), to, 8);
    let open = h.app.state.document(doc).unwrap();
    assert_eq!(open.document.layer_count(), 1);
    assert_eq!(open.undo_stack().undo_len(), 0);
}

#[cfg(unix)]
#[test]
fn a_failed_version_write_keeps_the_current_version() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let mut lib = AssetLibrary::open(dir.path());
    let id = lib.add("a.png", sprite(6, 6), PixelRect::new(0, 0, 6, 6));
    let assets = dir.path().join("assets");
    std::fs::set_permissions(&assets, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = lib.set_image(id, sprite(9, 9), Vec::new());
    std::fs::set_permissions(&assets, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(result, None);
    let a = lib.get(id).unwrap();
    assert_eq!(
        (a.version, a.versions.len(), &a.image),
        (1, 1, &sprite(6, 6))
    );
    assert!(!lib.notices().is_empty());
    let lib = AssetLibrary::open(dir.path());
    assert!(lib.notices().is_empty());
    assert_eq!(lib.get(id).unwrap().version, 1);
}

/// Extraction never reads Remove Background's preview layer: Extract Object
/// prompts, Accept, and Add Selection to Library wait until it ends.
#[test]
fn extraction_waits_while_remove_background_is_open() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.open_library(dir.path());
    h.app.set_segment_config(grabcut_only());
    let (id, _a, b) = two_layer_doc(&mut h, tmp.path());
    let real = h.app.state.document(id).unwrap().document.composite();
    // A mask made before the dialog opened cannot be accepted during it.
    h.app.set_tool(Tool::ExtractObject);
    h.frames(1);
    let prompt = CanvasEvent::ExtractBox {
        start: (b.x as f32 - 6.0, b.y as f32 - 6.0),
        end: (b.right() as f32 + 6.0, b.bottom() as f32 + 6.0),
    };
    assert!(h.app.extract_prompt(prompt.clone()));
    wait_until(&mut h, "mask", |h| {
        h.app.extract_session().is_some_and(|s| s.mask.is_some())
    });
    assert!(h.app.open_remove_background());
    h.frames(2);
    assert!(!h.app.extract_prompt(prompt.clone()), "prompt refused");
    assert!(!h.app.extract_accept(), "accept refused");
    // A real canvas box drag is ignored too.
    let a = h.screen_of(id, Pos2::new(b.x as f32 - 6.0, b.y as f32 - 6.0));
    let c = h.screen_of(
        id,
        Pos2::new(b.right() as f32 + 6.0, b.bottom() as f32 + 6.0),
    );
    let request = h.app.extract_session().unwrap().request;
    h.drag(a, c, 5);
    if h.app.remove_background_dialog().is_some() {
        assert_eq!(h.app.extract_session().unwrap().request, request);
    }
    assert!(!h.app.add_selection_to_library());
    assert!(h.app.state.assets.is_empty());
    h.app.remove_background_cancel();
    // Afterwards it reads the real canvas.
    assert!(h.app.extract_prompt(prompt));
    wait_until(&mut h, "mask after", |h| {
        h.app
            .extract_session()
            .is_some_and(|s| s.mask.is_some() && !s.busy)
    });
    assert!(h.app.extract_accept());
    let asset = &h.app.state.assets.assets()[0];
    for (x, y, p) in asset.image.enumerate_pixels() {
        if p[3] > 0 {
            assert_eq!(p, real.get_pixel(asset.rect.x + x, asset.rect.y + y));
        }
    }
}
