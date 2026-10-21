//! Extraction through the app: Extract menu dialogs, the Extract Object tool
//! (engine choice, consent, download), and the Asset Library panel.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Key, Modifiers, Pos2};
use game_asset_creator::canvas::{CanvasEvent, PointMode};
use game_asset_creator::extract::keying::KeyParams;
use game_asset_creator::extract::models::{self, Source};
use game_asset_creator::extract::sam;
use game_asset_creator::extract::segment::{Availability, Config, Engine};
use game_asset_creator::extract_ui::AssetAction;
use game_asset_creator::tools::Tool;
use game_asset_creator::ui::{Consent, Panel};
use game_asset_creator::DocId;
use image::GrayImage;

mod common;
use common::Harness;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/extract")
        .join(name)
}

fn open(h: &mut Harness, name: &str) -> DocId {
    let id = h.app.open_path(&fixture(name)).expect("opens");
    h.frames(2);
    id
}

fn layer(h: &Harness, id: DocId) -> image::RgbaImage {
    let open = h.app.state.document(id).unwrap();
    open.document
        .layer(open.active_layer())
        .unwrap()
        .pixels
        .clone()
}

fn wait_until(h: &mut Harness, what: &str, mut done: impl FnMut(&Harness) -> bool) {
    let start = Instant::now();
    while !done(h) {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
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
        egui::Shape::Text(t) if t.galley.text().contains(needle) => Some(t.pos),
        _ => None,
    })
}

/// GrabCut only: no ONNX Runtime.
fn grabcut_only() -> Config {
    Config {
        runtime_paths: Vec::new(),
        model_dir: std::env::temp_dir().join("gac-no-models"),
        source: Source::Directory(std::env::temp_dir().join("gac-no-models")),
    }
}

fn photo_click() -> (f32, f32) {
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture("photos.json")).unwrap()).unwrap();
    let c = &json["photos"][0]["click"];
    (c[0].as_f64().unwrap() as f32, c[1].as_f64().unwrap() as f32)
}

#[test]
fn remove_background_previews_picks_and_commits_one_step() {
    let mut h = Harness::new();
    let id = open(&mut h, "sheet_flat.png");
    let original = layer(&h, id);
    let steps = h.app.state.document(id).unwrap().undo_stack().undo_len();
    assert!(h.app.open_remove_background());
    h.frames(2);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Remove Background") && has_text(&out, "Edge Softness"));
    let dialog = h.app.remove_background_dialog().unwrap();
    assert_eq!(dialog.key().color, [255, 0, 255], "auto color");
    assert_eq!(
        layer(&h, id).get_pixel(0, 0)[3],
        0,
        "live preview keys the background"
    );
    assert_eq!(
        h.app.state.document(id).unwrap().undo_stack().undo_len(),
        steps
    );

    // Preview off shows the original.
    let key = dialog.key();
    h.app.set_remove_background(key, false);
    assert_eq!(layer(&h, id), original);
    // Pick a sprite color instead: keys that sprite (non-contiguous).
    h.app.set_remove_background(
        KeyParams {
            contiguous: false,
            ..key
        },
        true,
    );
    h.app.pick_background_color();
    h.frame(vec![]);
    let sprite = (50.0, 50.0); // the yellow circle's center
    let screen = h.screen_of(id, Pos2::new(sprite.0, sprite.1));
    h.click_at(screen);
    let picked = h.app.remove_background_dialog().unwrap().key().color;
    assert_eq!(
        picked,
        [
            original.get_pixel(50, 50)[0],
            original.get_pixel(50, 50)[1],
            original.get_pixel(50, 50)[2]
        ]
    );
    assert!(!h.app.remove_background_dialog().unwrap().picking);
    assert_eq!(layer(&h, id).get_pixel(50, 50)[3], 0, "picked color keyed");
    assert_eq!(layer(&h, id).get_pixel(0, 0)[3], 255, "magenta kept now");

    // Cancel restores exactly.
    h.app.remove_background_cancel();
    assert_eq!(layer(&h, id), original);

    // OK records one step; undo restores.
    assert!(h.app.open_remove_background());
    assert!(h.app.remove_background_ok());
    let open = h.app.state.document(id).unwrap();
    assert_eq!(open.undo_stack().undo_len(), steps + 1);
    assert_eq!(open.undo_stack().undo_label(), Some("Remove Background"));
    assert_eq!(layer(&h, id).get_pixel(0, 0)[3], 0);
    assert!(h.app.undo_current());
    assert_eq!(layer(&h, id), original);
}

#[test]
fn auto_detect_proposes_boxes_and_adds_checked_assets() {
    let mut h = Harness::new();
    let id = open(&mut h, "sheet_flat.png");
    let original = layer(&h, id);
    assert!(h.app.open_auto_detect());
    let truth: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(fixture("sheet_flat.json")).unwrap())
            .unwrap();
    let sprites = truth["sprites"].as_array().unwrap().len();
    assert_eq!(h.app.auto_detect_dialog().unwrap().proposals.len(), sprites);
    assert_eq!(layer(&h, id), original, "the layer is not changed");
    h.frames(2);
    let out = h.frame(vec![]);
    assert!(has_text(
        &out,
        &format!("{sprites} objects found, {sprites} selected")
    ));
    assert!(has_text(&out, "19"), "numbered boxes on the canvas");
    // Uncheck two, then add.
    let dialog = h.app.auto_detect_dialog_mut().unwrap();
    dialog.checked[0] = false;
    dialog.checked[1] = false;
    assert_eq!(h.app.auto_detect_add(), sprites - 2);
    assert!(h.app.auto_detect_dialog().is_none());
    assert_eq!(h.app.state.assets.len(), sprites - 2);
    let first = &h.app.state.assets.assets()[0];
    assert_eq!(first.name, "sheet_flat 1");
    assert_eq!(first.source, "sheet_flat.png");
    assert!(first.image.get_pixel(0, 0)[3] == 0 || first.image.pixels().any(|p| p[3] == 0));
    assert!(h.app.is_panel_open(Panel::AssetLibrary));
    let out = h.frame(vec![]);
    assert!(has_text(&out, &format!("{} assets", sprites - 2)));
    assert!(has_text(&out, "sheet_flat 1"));
}

#[test]
fn extract_object_with_grabcut_click_box_points_and_accept() {
    let mut h = Harness::new();
    h.app.set_segment_config(grabcut_only());
    let id = open(&mut h, "photo_potion.png");
    h.app.set_tool(Tool::ExtractObject);
    h.frame(vec![]);
    assert!(
        !h.app.segmenter_started(),
        "nothing loads before the first prompt"
    );
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Click an object or drag a box"));

    // Click on the canvas.
    let click = photo_click();
    let screen = h.screen_of(id, Pos2::new(click.0, click.1));
    h.click_at(screen);
    assert!(h.app.segmenter_started());
    wait_until(&mut h, "mask", |h| {
        h.app.extract_session().is_some_and(|s| s.mask.is_some())
    });
    let session = h.app.extract_session().unwrap();
    let (px, py, include) = session.prompt.points[0];
    assert!(include && session.prompt.points.len() == 1 && session.prompt.rect.is_none());
    assert!(
        (px - click.0).abs() <= 1.0 && (py - click.1).abs() <= 1.0,
        "click at ({px}, {py})"
    );
    assert_eq!(session.mask.as_ref().unwrap().1, Engine::GrabCut);
    assert_eq!(
        h.app.availability(),
        Some(&Availability::Unavailable(
            "ONNX Runtime not found".to_owned()
        ))
    );
    let out = h.frame(vec![]);
    assert!(
        has_text(&out, "GrabCut — ONNX Runtime not found"),
        "engine shown"
    );
    assert!(has_text(&out, "Add to Library") && has_text(&out, "To Selection"));

    // Shift-click adds an include point, Option-click an exclude point.
    h.modifiers = Modifiers::SHIFT;
    h.click_at(h.screen_of(id, Pos2::new(330.0, 150.0)));
    h.modifiers = Modifiers::ALT;
    h.click_at(h.screen_of(id, Pos2::new(60.0, 60.0)));
    h.modifiers = Modifiers::NONE;
    let points: Vec<bool> = h
        .app
        .extract_session()
        .unwrap()
        .prompt
        .points
        .iter()
        .map(|p| p.2)
        .collect();
    assert_eq!(points, vec![true, true, false]);
    wait_until(&mut h, "refined mask", |h| {
        h.app
            .extract_session()
            .is_some_and(|s| !s.busy && s.mask.is_some())
    });

    // To Selection is undoable; Enter adds the asset.
    let steps = h.app.state.document(id).unwrap().undo_stack().undo_len();
    assert!(h.app.extract_to_selection());
    let open_doc = h.app.state.document(id).unwrap();
    assert_eq!(open_doc.undo_stack().undo_len(), steps + 1);
    let selection: GrayImage = open_doc.document.selection().unwrap().clone();
    assert_eq!(selection.get_pixel(click.0 as u32, click.1 as u32)[0], 255);
    h.tap_key(Key::Enter);
    assert_eq!(h.app.state.assets.len(), 1);
    assert!(
        h.app.extract_session().is_none(),
        "accepting clears the prompt"
    );
    let asset = &h.app.state.assets.assets()[0];
    assert_eq!(asset.name, "photo_potion 1");
    assert!(asset.image.width() < 640 && asset.image.pixels().any(|p| p[3] == 0));

    // A dragged box; Esc clears.
    let a = h.screen_of(id, Pos2::new(220.0, 100.0));
    let b = h.screen_of(id, Pos2::new(440.0, 420.0));
    h.drag(a, b, 6);
    assert!(h.app.extract_session().unwrap().prompt.rect.is_some());
    wait_until(&mut h, "box mask", |h| {
        h.app
            .extract_session()
            .is_some_and(|s| !s.busy && s.mask.is_some())
    });
    h.tap_key(Key::Escape);
    assert!(h.app.extract_session().is_none());
    // Switching tools drops a session too.
    assert!(h.app.extract_prompt(CanvasEvent::ExtractPoint {
        point: click,
        mode: PointMode::Replace
    }));
    h.app.set_tool(Tool::Brush);
    h.frame(vec![]);
    assert!(h.app.extract_session().is_none());
}

#[test]
fn missing_models_ask_first_and_a_bad_download_keeps_grabcut() {
    let runtime = sam::default_runtime_paths();
    if sam::runtime(&runtime).is_err() {
        eprintln!("skipped: ONNX Runtime not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let bad = tempfile::tempdir().unwrap();
    for f in models::FILES {
        std::fs::write(bad.path().join(f.name), b"not a model").unwrap();
    }
    let mut h = Harness::new();
    h.app.set_segment_config(Config {
        runtime_paths: runtime,
        model_dir: dir.path().to_path_buf(),
        source: Source::Directory(bad.path().to_path_buf()),
    });
    let id = open(&mut h, "photo_skull.png");
    h.app.set_tool(Tool::ExtractObject);
    assert!(h.app.extract_prompt(CanvasEvent::ExtractPoint {
        point: (320.0, 200.0),
        mode: PointMode::Replace
    }));
    wait_until(&mut h, "consent question", |h| {
        h.app.consent() == Consent::Asking
    });
    h.frames(2);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Download MobileSAM?"));
    // GrabCut answers meanwhile.
    wait_until(&mut h, "fallback mask", |h| {
        h.app.extract_session().is_some_and(|s| s.mask.is_some())
    });
    assert_eq!(
        h.app.extract_session().unwrap().mask.as_ref().unwrap().1,
        Engine::GrabCut
    );
    // Download (from a folder with damaged files): verification fails.
    h.app.start_model_download();
    wait_until(&mut h, "download end", |h| !h.app.download_in_progress());
    let out = h.frame(vec![]);
    assert!(has_text(&out, "failed verification"), "error shown");
    assert!(
        std::fs::read_dir(dir.path()).unwrap().next().is_none(),
        "no partial files kept"
    );
    h.app.extract_prompt(CanvasEvent::ExtractPoint {
        point: (320.0, 200.0),
        mode: PointMode::Replace,
    });
    wait_until(&mut h, "mask", |h| {
        h.app
            .extract_session()
            .is_some_and(|s| !s.busy && s.mask.is_some())
    });
    assert_eq!(
        h.app.extract_session().unwrap().mask.as_ref().unwrap().1,
        Engine::GrabCut
    );
    let _ = id;
}

/// The full download path, when the real models are available locally
/// (`GAC_MODEL_SOURCE=<folder with the two .onnx files>`).
#[test]
fn downloaded_models_switch_extract_object_to_mobilesam() {
    let Some(source) = std::env::var_os("GAC_MODEL_SOURCE") else {
        eprintln!("skipped: set GAC_MODEL_SOURCE to a folder with the MobileSAM files");
        return;
    };
    let runtime = sam::default_runtime_paths();
    if sam::runtime(&runtime).is_err() {
        eprintln!("skipped: ONNX Runtime not installed");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.set_segment_config(Config {
        runtime_paths: runtime,
        model_dir: dir.path().to_path_buf(),
        source: Source::Directory(PathBuf::from(source)),
    });
    open(&mut h, "photo_chalice.png");
    h.app.set_tool(Tool::ExtractObject);
    h.app.extract_prompt(CanvasEvent::ExtractPoint {
        point: (320.0, 160.0),
        mode: PointMode::Replace,
    });
    wait_until(&mut h, "consent", |h| h.app.consent() == Consent::Asking);
    h.app.start_model_download();
    let out = h.frame(vec![]);
    assert!(h.app.download_in_progress() || has_text(&out, "MobileSAM"));
    wait_until(&mut h, "download", |h| !h.app.download_in_progress());
    assert!(models::present(dir.path()));
    wait_until(&mut h, "MobileSAM ready", |h| {
        h.app.availability() == Some(&Availability::Ready)
    });
    h.app.extract_prompt(CanvasEvent::ExtractPoint {
        point: (320.0, 160.0),
        mode: PointMode::Replace,
    });
    wait_until(&mut h, "SAM mask", |h| {
        h.app
            .extract_session()
            .is_some_and(|s| !s.busy && s.mask.as_ref().is_some_and(|m| m.1 == Engine::Sam))
    });
    let out = h.frame(vec![]);
    assert!(has_text(&out, "MobileSAM"));
}

#[test]
fn asset_library_panel_saves_opens_and_removes() {
    let mut h = Harness::new();
    let id = open(&mut h, "sheet_transparent.png");
    // A selection becomes an asset too.
    h.app.set_tool(Tool::RectSelect);
    let a = h.screen_of(id, Pos2::new(0.0, 0.0));
    let b = h.screen_of(id, Pos2::new(104.0, 100.0));
    h.drag(a, b, 6);
    assert!(h.app.add_selection_to_library());
    assert!(h.app.deselect());
    assert!(h.app.open_auto_detect());
    h.app.auto_detect_add();
    let n = h.app.state.assets.len();
    assert!(n >= 20);
    let first = h.app.state.assets.assets()[0].clone();
    assert_eq!(first.name, "sheet_transparent 1");
    assert!(
        h.app.state.assets.assets()[1..]
            .iter()
            .any(|a| a.rect == first.rect && a.image == first.image),
        "the selection crop is tight: identical to the detected circle"
    );

    // Click selects in the panel; Open as Document.
    h.frames(2);
    let out = h.frame(vec![]);
    let row = text_pos(&out, "sheet_transparent 2").expect("listed");
    h.click_at(row + egui::vec2(4.0, 4.0));
    let selected: Vec<u64> = h.app.asset_view().selected.iter().copied().collect();
    assert_eq!(selected, vec![h.app.state.assets.assets()[1].id]);
    let doc = h.app.asset_action(AssetAction::Open(selected[0])).unwrap();
    let opened = h.app.state.document(doc).unwrap();
    assert_eq!(opened.document.name, "sheet_transparent 2 (library)");
    assert_eq!(
        opened.document.layer(0).unwrap().pixels,
        h.app.state.assets.assets()[1].image
    );

    // Save PNG and Save All.
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("one.png");
    assert!(h.app.save_asset_to(first.id, &png));
    let back = image::open(&png).unwrap();
    assert_eq!(back.color(), image::ColorType::Rgba8);
    assert_eq!(back.into_rgba8(), first.image);
    let all = h.app.save_all_assets_to(&dir.path().join("all")).unwrap();
    assert_eq!(all.len(), n);

    // Remove.
    h.app.asset_action(AssetAction::Remove(selected));
    assert_eq!(h.app.state.assets.len(), n - 1);
    h.frame(vec![]);
    assert!(h.app.asset_view().selected.is_empty());
}

#[test]
fn extract_menu_is_disabled_without_a_document() {
    let mut h = Harness::new();
    assert!(!h.app.open_remove_background());
    assert!(!h.app.open_auto_detect());
    assert!(!h.app.add_selection_to_library());
    let id = open(&mut h, "sheet_flat.png");
    assert!(!h.app.add_selection_to_library(), "no selection");
    h.shortcut(Modifiers::COMMAND, Key::T);
    assert!(!h.app.open_remove_background(), "during a transform");
    h.tap_key(Key::Escape);
    assert!(h.app.open_auto_detect());
    assert!(!h.app.open_remove_background(), "one dialog at a time");
    h.app.auto_detect_cancel();
    let _ = id;
}
