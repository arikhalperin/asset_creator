//! App-level tests: the same operations the File menu runs, plus headless
//! rendering of the full egui UI.

use std::path::{Path, PathBuf};

use game_asset_creator::ui::{NewDocumentForm, Panel, Tab};
use game_asset_creator::{EditorApp, Layer, APP_TITLE};
use image::{Rgba, RgbaImage};

fn write_png(dir: &Path, name: &str, width: u32, height: u32) -> (PathBuf, RgbaImage) {
    let pixels = RgbaImage::from_fn(width, height, |x, y| {
        Rgba([(x * 40) as u8, (y * 30) as u8, 200, ((x + y) * 20) as u8])
    });
    let path = dir.join(name);
    pixels.save(&path).unwrap();
    (path, pixels)
}

/// Runs one headless egui frame of the whole UI.
fn frame(app: &mut EditorApp, ctx: &egui::Context) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.show(ui));
    // No renderer here to upload textures to.
    output.textures_delta.clear();
}

fn tab_titles(app: &EditorApp) -> Vec<Tab> {
    app.dock().iter_all_tabs().map(|(_, tab)| *tab).collect()
}

#[test]
fn title_constant() {
    assert_eq!(APP_TITLE, "Game Asset Creator");
}

#[test]
fn default_layout_has_three_panels_and_no_documents() {
    let app = EditorApp::new();
    for panel in Panel::ALL {
        assert!(app.is_panel_open(panel), "{panel:?}");
    }
    assert!(app.document_tabs().is_empty());
    assert!(app.state.current_document().is_none());
}

#[test]
fn panels_can_be_hidden_and_restored() {
    let mut app = EditorApp::new();
    for panel in Panel::ALL {
        app.set_panel_open(panel, false);
        assert!(!app.is_panel_open(panel));
    }
    for panel in Panel::ALL {
        app.set_panel_open(panel, true);
        assert!(app.is_panel_open(panel));
    }
    // Re-opening an open panel does not duplicate it.
    app.set_panel_open(Panel::Tools, true);
    let tools = tab_titles(&app)
        .into_iter()
        .filter(|t| *t == Tab::Panel(Panel::Tools))
        .count();
    assert_eq!(tools, 1);
}

#[test]
fn opening_an_image_adds_a_focused_tab() {
    let dir = tempfile::tempdir().unwrap();
    let (path, pixels) = write_png(dir.path(), "sprite.png", 6, 5);
    let mut app = EditorApp::new();

    let id = app.open_path(&path).expect("opened");
    assert_eq!(app.document_tabs(), vec![id]);
    assert_eq!(app.state.current_id(), Some(id));
    let open = app.state.current_document().unwrap();
    assert_eq!(open.document.name, "sprite.png");
    assert_eq!(open.document.composite(), pixels);
    assert!(app.error_message().is_none());

    // The new tab is the active, focused one after a frame.
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    let mut dock = app.dock().clone();
    assert_eq!(
        dock.find_active_focused().map(|(_, t)| *t),
        Some(Tab::Document(id))
    );
    assert_eq!(app.state.current_id(), Some(id));
}

#[test]
fn opening_a_second_image_groups_tabs_and_focuses_it() {
    let dir = tempfile::tempdir().unwrap();
    let (a, _) = write_png(dir.path(), "a.png", 4, 4);
    let (b, _) = write_png(dir.path(), "b.png", 3, 3);
    let mut app = EditorApp::new();
    let ctx = egui::Context::default();
    let first = app.open_path(&a).unwrap();
    frame(&mut app, &ctx);
    let second = app.open_path(&b).unwrap();
    frame(&mut app, &ctx);
    assert_eq!(app.document_tabs(), vec![first, second]);
    assert_eq!(app.state.current_id(), Some(second));
    let first_path = app.dock().find_tab(&Tab::Document(first)).unwrap();
    let second_path = app.dock().find_tab(&Tab::Document(second)).unwrap();
    assert_eq!(
        first_path.node_path(),
        second_path.node_path(),
        "same tab group"
    );
}

#[test]
fn failed_open_shows_error_and_adds_no_tab() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("broken.png");
    std::fs::write(&bad, b"garbage").unwrap();
    let unsupported = dir.path().join("x.tga");
    std::fs::write(&unsupported, b"garbage").unwrap();

    let mut app = EditorApp::new();
    for path in [bad, unsupported] {
        assert!(app.open_path(&path).is_none());
        assert!(app.error_message().is_some());
        app.dismiss_error();
    }
    assert!(app.document_tabs().is_empty());
    assert!(app.state.documents().is_empty());

    // The error window renders.
    app.open_path(&dir.path().join("missing.png"));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    assert!(app.error_message().is_some());
}

#[test]
fn new_document_dialog_creates_transparent_document() {
    let mut app = EditorApp::new();
    app.show_new_dialog();
    assert_eq!(app.new_dialog(), Some(&NewDocumentForm::default()));

    let form = NewDocumentForm {
        width: "40".into(),
        height: "30".into(),
        dpi: "300".into(),
        error: None,
    };
    let id = app.submit_new_dialog(form).expect("created");
    assert!(app.new_dialog().is_none());
    assert_eq!(app.document_tabs(), vec![id]);
    let doc = &app.state.current_document().unwrap().document;
    assert_eq!((doc.width(), doc.height(), doc.dpi()), (40, 30, 300.0));
    assert_eq!(doc.name, "Untitled-1");
    assert_eq!(doc.layer_count(), 1);
    assert!(doc.composite().pixels().all(|p| p[3] == 0));
}

#[test]
fn new_document_dialog_rejects_invalid_values() {
    let mut app = EditorApp::new();
    let cases = [
        ("0", "10", "72"),
        ("abc", "10", "72"),
        ("10", "-3", "72"),
        ("10", "10", "0"),
        ("10", "10", "x"),
        ("99999", "10", "72"),
    ];
    for (w, h, dpi) in cases {
        app.show_new_dialog();
        let form = NewDocumentForm {
            width: w.into(),
            height: h.into(),
            dpi: dpi.into(),
            error: None,
        };
        assert!(app.submit_new_dialog(form).is_none(), "{w} {h} {dpi}");
        let dialog = app.new_dialog().expect("dialog stays open");
        assert!(dialog.error.is_some());
    }
    assert!(app.state.documents().is_empty());

    // The dialog with its inline error renders.
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    assert!(app.new_dialog().is_some());
}

#[test]
fn export_round_trips_png_pixels_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let (path, pixels) = write_png(dir.path(), "src.png", 9, 7);
    let mut app = EditorApp::new();
    app.open_path(&path).unwrap();

    let out = dir.path().join("out.png");
    assert!(app.export_to(&out));
    assert_eq!(image::open(&out).unwrap().into_rgba8(), pixels);
}

#[test]
fn export_to_each_format() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = write_png(dir.path(), "src.png", 5, 5);
    let mut app = EditorApp::new();
    app.open_path(&path).unwrap();
    for name in ["o.png", "o.jpg", "o.jpeg", "o.bmp", "o.webp"] {
        let out = dir.path().join(name);
        assert!(app.export_to(&out), "{name}");
        let img = image::open(&out).unwrap();
        assert_eq!((img.width(), img.height()), (5, 5), "{name}");
    }
    assert!(!app.export_to(&dir.path().join("o.gif")));
    assert!(app.error_message().is_some());
}

#[test]
fn export_without_document_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = EditorApp::new();
    let out = dir.path().join("nothing.png");
    assert!(!app.export_to(&out));
    assert!(!out.exists());
}

#[test]
fn layers_follow_current_document_and_close() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = write_png(dir.path(), "img.png", 2, 2);
    let mut app = EditorApp::new();
    let ctx = egui::Context::default();
    assert!(app.state.current_layer_rows().is_empty());

    let opened = app.open_path(&path).unwrap();
    let blank = app.create_document(4, 4, 72.0).unwrap();
    frame(&mut app, &ctx);
    assert_eq!(app.state.current_id(), Some(blank));

    // Add layers to the blank document; the listing is top-first.
    {
        let doc = app.state.document_mut(blank).unwrap();
        doc.add_layer(Layer::transparent("Top", 4, 4)).unwrap();
        let mut hidden = Layer::transparent("Hidden", 4, 4);
        hidden.visible = false;
        doc.insert_layer(1, hidden).unwrap();
    }
    let rows: Vec<_> = app
        .state
        .current_layer_rows()
        .into_iter()
        .map(|r| (r.name, r.visible))
        .collect();
    assert_eq!(
        rows,
        [
            ("Top".to_owned(), true),
            ("Hidden".to_owned(), false),
            ("Background".to_owned(), true)
        ]
    );
    frame(&mut app, &ctx);

    app.state.set_current(opened);
    assert_eq!(
        app.state.current_document().unwrap().document.name,
        "img.png"
    );

    app.close_current();
    assert_eq!(app.document_tabs(), vec![blank]);
    assert_eq!(app.state.current_id(), Some(blank));
    app.close_document(blank);
    frame(&mut app, &ctx);
    assert!(app.document_tabs().is_empty());
    assert!(app.state.current_document().is_none());
    assert!(app.state.current_layer_rows().is_empty());

    // Documents open again into the document area after everything closed.
    let again = app.open_path(&path).unwrap();
    frame(&mut app, &ctx);
    assert_eq!(app.document_tabs(), vec![again]);
    assert_eq!(app.state.current_id(), Some(again));
}

#[test]
fn full_ui_renders_headlessly() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = write_png(dir.path(), "render.png", 64, 48);
    let mut app = EditorApp::new();
    let ctx = egui::Context::default();
    frame(&mut app, &ctx);
    app.open_path(&path).unwrap();
    app.create_document(32, 32, 96.0).unwrap();
    for _ in 0..3 {
        frame(&mut app, &ctx);
    }
    app.set_panel_open(Panel::Layers, false);
    frame(&mut app, &ctx);
    app.set_panel_open(Panel::Layers, true);
    frame(&mut app, &ctx);
    assert_eq!(app.document_tabs().len(), 2);
}
