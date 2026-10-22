//! Recolouring through the app: Parts Layout Recolor… and Image ▸ Recolor
//! Paint… (needs PDFium in `GAC_PDFIUM_DIR`; protection needs
//! `GAC_MODEL_DIR`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Pos2, Vec2};
use game_asset_creator::canvas::ViewAction;
use game_asset_creator::extract::models::Source;
use game_asset_creator::parts_ui::PartsId;
use game_asset_creator::pdf;
use game_asset_creator::recolor::{delta_e, lab};
use game_asset_creator::tools::Tool;
use game_asset_creator::ui::PdfConfig;
use image::RgbaImage;

mod common;
use common::Harness;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/recolor")
        .join(name)
}

fn installed() -> Option<PathBuf> {
    let dir = pdf::pdfium::default_dir();
    pdf::pdfium::installed(&dir).then_some(dir).or_else(|| {
        eprintln!("skipped: set GAC_PDFIUM_DIR");
        None
    })
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

fn truth() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(fixture("recolor.json")).unwrap()).unwrap()
}

fn olive() -> [f32; 3] {
    lab([107, 115, 71])
}

fn px(mm: f64) -> u32 {
    pdf::mm_to_px(mm as f32).round() as u32
}

fn at(img: &RgbaImage, x: u32, y: u32) -> [f32; 3] {
    let p = img.get_pixel(x, y);
    lab([p[0], p[1], p[2]])
}

fn open(h: &mut Harness) -> PartsId {
    h.app.set_pdf_config(PdfConfig {
        dir: installed().unwrap(),
        source: Source::Directory(PathBuf::from("/nonexistent")),
    });
    h.app.open_path(&fixture("model_paint.pdf"));
    let id = *h.app.parts_tabs().last().unwrap();
    wait_until(h, "pages", |h| !h.app.parts_tab(id).unwrap().loading());
    h.frames(2);
    id
}

/// A plain-paint spot of the first panel of page 1 (olive on sheet 1).
fn olive_spot(t: &serde_json::Value) -> (u32, u32) {
    let r: Vec<f64> = t["pages"][0]["panels"][0]["rect_mm"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    (px(r[0] + r[2] / 6.0), px(r[1] + r[3] / 2.0 + 7.0))
}

#[test]
fn parts_layout_recolor_previews_applies_keeps_layout_and_reverts() {
    if installed().is_none() {
        return;
    }
    let t = truth();
    let mut h = Harness::new();
    let id = open(&mut h);
    let original: Vec<RgbaImage> = h
        .app
        .parts_tab(id)
        .unwrap()
        .pages
        .iter()
        .map(|p| p.image.clone())
        .collect();
    click_text(&mut h, "Recolor…");
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Recolor Paint") && has_text(&out, "Detail strength"));
    let tab = h.app.parts_tab(id).unwrap();
    let rc = &tab.recolor.as_ref().unwrap().rc;
    assert_eq!(rc.swatches.len(), 4, "{:?}", rc.swatches);
    let k = rc
        .swatches
        .iter()
        .position(|s| delta_e(s.lab, olive()) < 8.0)
        .unwrap();
    // Clicking the olive panel picks its swatch.
    let (sx, sy) = olive_spot(&t);
    let (_, rect, scale) = tab.page_rects[0];
    h.click_at(rect.min + Vec2::new(sx as f32, sy as f32) * scale);
    assert_eq!(
        h.app
            .parts_tab(id)
            .unwrap()
            .recolor
            .as_ref()
            .unwrap()
            .selected,
        Some(k)
    );
    // Map olive → blue: the preview follows within a frame.
    let blue = [40u8, 70, 160];
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.recolor.as_mut().unwrap().rc.targets[k] = Some(blue);
    tab.recolor_changed();
    let f = Instant::now();
    h.frame(vec![]);
    let preview = f.elapsed();
    eprintln!("preview frame {preview:?}");
    if !cfg!(debug_assertions) {
        assert!(
            preview < Duration::from_millis(200),
            "preview frame {preview:?}"
        );
    }
    let placements = h.app.parts_tab(id).unwrap().layout.placements.clone();
    let rects: Vec<_> = h
        .app
        .parts_tab(id)
        .unwrap()
        .proposals
        .iter()
        .map(|p| p.rect)
        .collect();
    // Apply in the background.
    click_text(&mut h, "Apply");
    assert!(h.app.parts_tab(id).unwrap().recolor_progress().is_some());
    wait_until(&mut h, "recoloured", |h| {
        h.app.parts_tab(id).unwrap().recolor_progress().is_none()
    });
    let tab = h.app.parts_tab(id).unwrap();
    assert!(tab.recolored());
    assert!(delta_e(at(&tab.pages[0].image, sx, sy), lab(blue)) < 5.0);
    assert_eq!(tab.layout.placements, placements, "no re-pack");
    assert_eq!(
        tab.proposals.iter().map(|p| p.rect).collect::<Vec<_>>(),
        rects
    );
    // The part images (and so the export) carry the new colour.
    let has_blue = tab.layout.items.iter().any(|it| {
        it.image
            .pixels()
            .any(|p| p[3] > 0 && delta_e(lab([p[0], p[1], p[2]]), lab(blue)) < 5.0)
    });
    assert!(has_blue);
    let dir = tempfile::tempdir().unwrap();
    let out_pdf = dir.path().join("a4.pdf");
    tab.export(&out_pdf).unwrap();
    let sheet = pdf::render::render_page(&installed().unwrap(), &out_pdf, 0).unwrap();
    assert!(sheet
        .image
        .pixels()
        .any(|p| delta_e(lab([p[0], p[1], p[2]]), lab(blue)) < 5.0));
    // Revert restores the original pages exactly.
    click_text(&mut h, "Revert to Original");
    let tab = h.app.parts_tab(id).unwrap();
    assert!(!tab.recolored());
    for (p, o) in tab.pages.iter().zip(&original) {
        assert!(p.image == *o, "page restored");
    }
    assert_eq!(tab.layout.placements, placements);
}

#[test]
fn protect_needs_mobilesam_and_says_so() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    h.app
        .set_segment_config(game_asset_creator::extract::segment::Config {
            runtime_paths: Vec::new(),
            model_dir: std::env::temp_dir().join("gac-no-models"),
            source: Source::Directory(PathBuf::from("/nonexistent")),
        });
    let id = open(&mut h);
    h.app.parts_tab_mut(id).unwrap().open_recolor();
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Protect needs the MobileSAM model."));
    assert!(!h.app.parts_tab(id).unwrap().sam_ready);
}

/// With MobileSAM (`GAC_MODEL_DIR`): a protected olive panel keeps its
/// colour while the other olive panels change.
#[test]
fn protected_areas_are_never_recoloured() {
    use game_asset_creator::extract::{sam, segment};
    let Some(models) = std::env::var_os("GAC_MODEL_DIR").map(PathBuf::from) else {
        eprintln!("skipped: set GAC_MODEL_DIR");
        return;
    };
    if installed().is_none() {
        return;
    }
    let runtime = sam::default_runtime_paths();
    if sam::runtime(&runtime).is_err() {
        eprintln!("skipped: ONNX Runtime not installed");
        return;
    }
    let t = truth();
    let mut h = Harness::new();
    h.app.set_segment_config(segment::Config {
        runtime_paths: runtime,
        model_dir: models,
        source: Source::Directory(PathBuf::from("/nonexistent")),
    });
    let id = open(&mut h);
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.open_recolor();
    assert!(tab.sam_ready);
    tab.recolor.as_mut().unwrap().protect_mode = true;
    let (sx, sy) = olive_spot(&t);
    h.frames(2);
    let (_, rect, scale) = h.app.parts_tab(id).unwrap().page_rects[0];
    h.click_at(rect.min + Vec2::new(sx as f32, sy as f32) * scale);
    wait_until(&mut h, "protected", |h| {
        h.app
            .parts_tab(id)
            .unwrap()
            .protect
            .first()
            .is_some_and(Option::is_some)
    });
    let mask = h.app.parts_tab(id).unwrap().protect[0].clone().unwrap();
    assert!(mask.get_pixel(sx, sy)[0] > 0);
    let before = h.app.parts_tab(id).unwrap().pages[0].image.clone();
    let tab = h.app.parts_tab_mut(id).unwrap();
    let d = tab.recolor.as_mut().unwrap();
    d.protect_mode = false;
    let k =
        d.rc.swatches
            .iter()
            .position(|s| delta_e(s.lab, olive()) < 8.0)
            .unwrap();
    d.rc.targets[k] = Some([150, 30, 30]);
    assert!(tab.start_recolor(std::sync::Arc::new(|| {})));
    wait_until(&mut h, "recoloured", |h| {
        h.app.parts_tab(id).unwrap().recolor_progress().is_none()
    });
    let after = &h.app.parts_tab(id).unwrap().pages[0].image;
    for (x, y, m) in mask.enumerate_pixels() {
        if m[0] > 0 {
            assert_eq!(
                after.get_pixel(x, y),
                before.get_pixel(x, y),
                "protected ({x}, {y}) changed"
            );
        }
    }
    // The big olive panel at the bottom (not protected) did change.
    let big: Vec<f64> = t["pages"][0]["panels"][4]["rect_mm"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let (bx, by) = (px(big[0] + 15.0), px(big[1] + big[3] / 2.0));
    assert!(delta_e(at(after, bx, by), lab([150, 30, 30])) < 5.0);
}

/// Image ▸ Recolor Paint…: within the selection only, live preview, Cancel
/// restores, OK is one undo step.
#[test]
fn editor_recolor_paint_uses_the_selection_and_undo() {
    let Some(lib) = installed() else { return };
    let page = pdf::render::render_page(&lib, &fixture("model_paint.pdf"), 0)
        .unwrap()
        .image;
    // A crop holding the olive and sand panels.
    let crop =
        image::imageops::crop_imm(&page, px(15.0), px(35.0), px(280.0), px(110.0)).to_image();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("panels.png");
    crop.save(&file).unwrap();
    let mut h = Harness::new();
    let id = h.app.open_path(&file).unwrap();
    h.app.view_action(ViewAction::Fit);
    h.frames(2);
    // Select the left half (the olive panel).
    h.app.set_tool(Tool::RectSelect);
    h.frames(1);
    let (w, hh) = crop.dimensions();
    let a = h.screen_of(id, Pos2::new(0.0, 0.0));
    let b = h.screen_of(id, Pos2::new(w as f32 / 2.0, hh as f32));
    h.drag(a, b, 5);
    let original = h
        .app
        .state
        .document(id)
        .unwrap()
        .document
        .layer(0)
        .unwrap()
        .pixels
        .clone();
    let steps = h.app.state.document(id).unwrap().undo_stack().undo_len();
    let open_menu = |h: &mut Harness| {
        for _ in 0..3 {
            click_text(h, "Image");
            h.frame(vec![]);
            let out = h.frame(vec![]);
            let title = text_pos(&out, "Image").unwrap();
            let item = out
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == "Recolor Paint…" => Some(t.pos),
                    _ => None,
                })
                .find(|p| p.y > title.y);
            if let Some(p) = item {
                h.click_at(p + Vec2::new(4.0, 4.0));
                return;
            }
        }
        panic!("no Recolor Paint… in the Image menu");
    };
    open_menu(&mut h);
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Paint colours of the layer:"));
    {
        let rc = h.app.recolor_paint_dialog_mut().expect("dialog open");
        // Only the selected (olive) paint is offered.
        assert_eq!(rc.swatches.len(), 1, "{:?}", rc.swatches);
        rc.targets[0] = Some([40, 70, 160]);
    }
    h.app.refresh_recolor_paint();
    let layer = |h: &Harness| {
        h.app
            .state
            .document(id)
            .unwrap()
            .document
            .layer(0)
            .unwrap()
            .pixels
            .clone()
    };
    let live = layer(&h);
    let (ox, oy) = (px(20.0), px(40.0));
    assert!(
        delta_e(at(&live, ox, oy), lab([40, 70, 160])) < 5.0,
        "previewed"
    );
    click_text(&mut h, "Cancel");
    assert!(layer(&h) == original, "Cancel restores");
    open_menu(&mut h);
    h.app.recolor_paint_dialog_mut().unwrap().targets[0] = Some([40, 70, 160]);
    h.app.refresh_recolor_paint();
    click_text(&mut h, "OK");
    let done = layer(&h);
    let open = h.app.state.document(id).unwrap();
    assert_eq!(open.undo_stack().undo_len(), steps + 1);
    assert_eq!(open.undo_stack().undo_label(), Some("Recolor Paint"));
    assert!(delta_e(at(&done, ox, oy), lab([40, 70, 160])) < 5.0);
    for (x, y, p) in done.enumerate_pixels() {
        if x > w / 2 {
            assert_eq!(
                p,
                original.get_pixel(x, y),
                "outside the selection ({x}, {y})"
            );
        }
    }
    assert!(h.app.undo_current());
    assert!(layer(&h) == original);
}

/// Right-click a page ▸ Open Page in Editor, Recolor Paint… there, then
/// Image ▸ Update Parts Layout Page: the page, its parts, and the export
/// carry the edit, placements stay, Revert restores the PDF render; the
/// command is refused for unlinked documents, closed tabs, and other sizes.
#[test]
fn a_pdf_page_opens_in_the_editor_and_goes_back() {
    if installed().is_none() {
        return;
    }
    let t = truth();
    let mut h = Harness::new();
    let id = open(&mut h);
    let original: Vec<RgbaImage> = h
        .app
        .parts_tab(id)
        .unwrap()
        .pages
        .iter()
        .map(|p| p.image.clone())
        .collect();
    let placements = h.app.parts_tab(id).unwrap().layout.placements.clone();
    // Right-click page 1 ▸ Open Page in Editor.
    let (_, rect, _) = h.app.parts_tab(id).unwrap().page_rects[0];
    h.pointer = rect.center();
    h.frame(vec![]);
    let down = h.press(egui::PointerButton::Secondary, true);
    h.frame(vec![down]);
    let up = h.press(egui::PointerButton::Secondary, false);
    h.frame(vec![up]);
    h.frame(vec![]);
    click_text(&mut h, "Open Page in Editor");
    h.frames(2);
    let doc = h.app.state.current_id().expect("a document");
    let opened = h.app.state.document(doc).unwrap();
    assert!(
        opened.document.name.ends_with("– page 1"),
        "{}",
        opened.document.name
    );
    assert!(
        opened.document.composite() == original[0],
        "exactly the page"
    );
    assert_eq!(h.app.update_parts_page_blocker(doc), None);
    // Recolor Paint… olive → blue, OK.
    assert!(h.app.open_recolor_paint());
    let blue = [40u8, 70, 160];
    {
        let rc = h.app.recolor_paint_dialog_mut().unwrap();
        let k = rc
            .swatches
            .iter()
            .position(|s| delta_e(s.lab, olive()) < 8.0)
            .unwrap();
        rc.targets[k] = Some(blue);
    }
    h.app.refresh_recolor_paint();
    assert!(h.app.recolor_paint_ok());
    h.frames(2);
    // Image ▸ Update Parts Layout Page.
    let mut clicked = false;
    for _ in 0..3 {
        click_text(&mut h, "Image");
        h.frame(vec![]);
        let out = h.frame(vec![]);
        if let Some(p) = text_pos(&out, "Update Parts Layout Page") {
            h.click_at(p + Vec2::new(4.0, 4.0));
            clicked = true;
            break;
        }
    }
    assert!(clicked, "no Update Parts Layout Page in the Image menu");
    h.frames(2);
    let tab = h.app.parts_tab(id).unwrap();
    let (sx, sy) = olive_spot(&t);
    assert!(
        delta_e(at(&tab.pages[0].image, sx, sy), lab(blue)) < 5.0,
        "page updated"
    );
    assert!(tab.pages[1].image == original[1], "other pages unchanged");
    assert_eq!(tab.layout.placements, placements, "no re-pack");
    let has_blue = tab.layout.items.iter().any(|it| {
        it.image
            .pixels()
            .any(|p| p[3] > 0 && delta_e(lab([p[0], p[1], p[2]]), lab(blue)) < 5.0)
    });
    assert!(has_blue, "parts re-cut from the edited page");
    let dir = tempfile::tempdir().unwrap();
    let out_pdf = dir.path().join("a4.pdf");
    tab.export(&out_pdf).unwrap();
    let sheet = pdf::render::render_page(&installed().unwrap(), &out_pdf, 0).unwrap();
    assert!(sheet
        .image
        .pixels()
        .any(|p| delta_e(lab([p[0], p[1], p[2]]), lab(blue)) < 5.0));
    // A page of another size is refused and nothing changes.
    let tab = h.app.parts_tab_mut(id).unwrap();
    let small = image::imageops::crop_imm(&original[0], 0, 0, 100, 100).to_image();
    let err = tab.set_edited_page(0, small).unwrap_err();
    assert!(err.contains("canvas"), "{err}");
    assert_eq!(tab.layout.placements, placements);
    // Revert to Original restores the PDF render exactly.
    assert!(tab.revert_recolor());
    for (p, o) in tab.pages.iter().zip(&original) {
        assert!(p.image == *o, "page restored");
    }
    // An unlinked document and a closed tab are refused with a reason.
    let file = dir.path().join("plain.png");
    original[0].save(&file).unwrap();
    let plain = h.app.open_path(&file).unwrap();
    assert!(h
        .app
        .update_parts_page_blocker(plain)
        .unwrap()
        .contains("Open Page in Editor"));
    h.app.close_parts_tab(id);
    assert_eq!(
        h.app.update_parts_page_blocker(doc).as_deref(),
        Some("Its Parts Layout tab was closed")
    );
}

/// The swatch rows offer the new-colour picker directly: clicking the
/// button after → opens it, and picking a colour there maps that paint and
/// previews it (no hidden switch first).
#[test]
fn picking_a_new_colour_in_the_window_maps_the_paint() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    click_text(&mut h, "Recolor…");
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Click a colour after →"));
    // The first row's "→", then its colour button just right of it.
    let arrow = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "→" => Some(t.pos),
            _ => None,
        })
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .expect("an → per swatch");
    let before: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
            _ => None,
        })
        .collect();
    h.click_at(arrow + Vec2::new(30.0, 6.0));
    let out = h.frame(vec![]);
    let texts: Vec<String> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
            _ => None,
        })
        .filter(|t| !before.contains(t))
        .collect();
    assert!(
        texts.iter().any(|t| t == "U8"),
        "the picker opened: {texts:?}"
    );
    // Drag across the picker's colour field (below its R G B values): the
    // colour changes.
    let rgb = text_pos(&out, "U8").unwrap() + Vec2::new(40.0, 0.0);
    let start = rgb + Vec2::new(20.0, 80.0);
    h.drag(start, start + Vec2::new(40.0, 20.0), 4);
    let tab = h.app.parts_tab(id).unwrap();
    let rc = &tab.recolor.as_ref().unwrap().rc;
    assert!(
        rc.targets[0].is_some(),
        "picking a colour maps the paint: {:?}",
        rc.targets
    );
}

/// Screen position of a page point given in millimetres.
fn page_mm(h: &Harness, id: PartsId, page: usize, x: f64, y: f64) -> Pos2 {
    let (_, rect, scale) = h
        .app
        .parts_tab(id)
        .unwrap()
        .page_rects
        .iter()
        .copied()
        .find(|r| r.0 == page)
        .unwrap();
    rect.min + Vec2::new(px(x) as f32, px(y) as f32) * scale
}

/// Recolour Area: a rectangle and a Shift-lasso on page 1 limit the
/// palette and the recolour to them; page 2 and olive outside the areas
/// stay; Clear Areas goes back to whole pages.
#[test]
fn drawn_areas_limit_the_recolour() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let original: Vec<RgbaImage> = h
        .app
        .parts_tab(id)
        .unwrap()
        .pages
        .iter()
        .map(|p| p.image.clone())
        .collect();
    click_text(&mut h, "Recolor…");
    h.frames(2);
    click_text(&mut h, "Recolour Area");
    h.frames(1);
    assert!(
        h.app
            .parts_tab(id)
            .unwrap()
            .recolor
            .as_ref()
            .unwrap()
            .area_mode
    );
    // A rectangle over the left part of the first olive panel.
    let (a, b) = (
        page_mm(&h, id, 0, 25.0, 45.0),
        page_mm(&h, id, 0, 70.0, 135.0),
    );
    h.drag(a, b, 6);
    // A Shift-lasso (triangle) in the big olive panel.
    h.modifiers = egui::Modifiers::SHIFT;
    let pts = [(25.0, 300.0), (90.0, 300.0), (60.0, 385.0), (25.0, 300.0)];
    h.pointer = page_mm(&h, id, 0, pts[0].0, pts[0].1);
    h.frame(vec![]);
    let down = h.press(egui::PointerButton::Primary, true);
    h.frame(vec![down]);
    for w in pts.windows(2) {
        for k in 1..=8 {
            let f = k as f64 / 8.0;
            let (x, y) = (
                w[0].0 + (w[1].0 - w[0].0) * f,
                w[0].1 + (w[1].1 - w[0].1) * f,
            );
            h.pointer = page_mm(&h, id, 0, x, y);
            h.frame(vec![]);
        }
    }
    let up = h.press(egui::PointerButton::Primary, false);
    h.frame(vec![up]);
    h.modifiers = egui::Modifiers::NONE;
    h.frames(2);
    let tab = h.app.parts_tab(id).unwrap();
    assert_eq!(tab.areas.len(), 2, "rectangle and lasso");
    assert!(tab.areas[1].1.len() > 4, "the lasso keeps its points");
    let rc = &tab.recolor.as_ref().unwrap().rc;
    assert_eq!(
        rc.swatches.len(),
        1,
        "only the paint inside: {:?}",
        rc.swatches
    );
    assert!(delta_e(rc.swatches[0].lab, olive()) < 8.0);
    // Olive → blue, applied.
    let blue = [40u8, 70, 160];
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.recolor.as_mut().unwrap().rc.targets[0] = Some(blue);
    tab.recolor_changed();
    h.frame(vec![]);
    assert!(h
        .app
        .parts_tab_mut(id)
        .unwrap()
        .start_recolor(std::sync::Arc::new(|| {})));
    wait_until(&mut h, "recoloured", |h| {
        h.app.parts_tab(id).unwrap().recolor_progress().is_none()
    });
    let page = &h.app.parts_tab(id).unwrap().pages[0];
    let near = |x: f64, y: f64, c: [u8; 3]| delta_e(at(&page.image, px(x), px(y)), lab(c)) < 5.0;
    assert!(near(40.0, 97.0, blue), "inside the rectangle");
    assert!(near(55.0, 325.0, blue), "inside the lasso");
    let same =
        |x: f64, y: f64| page.image.get_pixel(px(x), px(y)) == original[0].get_pixel(px(x), px(y));
    assert!(same(110.0, 97.0), "olive outside the rectangle");
    assert!(same(220.0, 340.0), "olive outside the lasso");
    assert!(
        h.app.parts_tab(id).unwrap().pages[1].image == original[1],
        "a page without areas is unchanged"
    );
    // Clear Areas: the whole-page palette, olive still mapped.
    click_text(&mut h, "Clear Areas");
    h.frames(1);
    let tab = h.app.parts_tab(id).unwrap();
    assert!(tab.areas.is_empty());
    let rc = &tab.recolor.as_ref().unwrap().rc;
    // The pages' paints (now with the new blue too).
    assert!(rc.swatches.len() >= 4, "{:?}", rc.swatches);
    let k = rc
        .swatches
        .iter()
        .position(|s| delta_e(s.lab, olive()) < 8.0)
        .unwrap();
    assert_eq!(rc.targets[k], Some(blue));
}

/// Typing into a swatch row: hex code, R G B values, invalid hex ignored,
/// the paint's own colour means unchanged.
#[test]
fn typed_rgb_and_hex_set_the_new_colour() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    click_text(&mut h, "Recolor…");
    h.frames(2);
    let own = h
        .app
        .parts_tab(id)
        .unwrap()
        .recolor
        .as_ref()
        .unwrap()
        .rc
        .swatches[0]
        .rgb;
    let own_hex = format!("#{:02X}{:02X}{:02X}", own[0], own[1], own[2]);
    let target = |h: &Harness| {
        h.app
            .parts_tab(id)
            .unwrap()
            .recolor
            .as_ref()
            .unwrap()
            .rc
            .targets[0]
    };
    let type_into = |h: &mut Harness, at: &str, text: &str| {
        click_text(h, at);
        h.frames(1);
        h.shortcut(egui::Modifiers::COMMAND, egui::Key::A);
        h.frame(vec![egui::Event::Text(text.to_owned())]);
        let enter = h.key(egui::Key::Enter, true);
        h.frame(vec![enter]);
        h.frames(2);
    };
    type_into(&mut h, &own_hex, "#2846A0");
    assert_eq!(target(&h), Some([0x28, 0x46, 0xA0]), "hex");
    type_into(&mut h, "#2846A0", "#zz12");
    assert_eq!(target(&h), Some([0x28, 0x46, 0xA0]), "invalid hex ignored");
    // The first row's R value.
    let out = h.frame(vec![]);
    let r = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "40" => Some(t.pos),
            _ => None,
        })
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap_or_else(|| {
            let texts: Vec<String> = out
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
            panic!("no R value 40: {texts:?}")
        });
    h.click_at(r + Vec2::new(4.0, 4.0));
    h.frames(1);
    h.shortcut(egui::Modifiers::COMMAND, egui::Key::A);
    h.frame(vec![egui::Event::Text("200".to_owned())]);
    let enter = h.key(egui::Key::Enter, true);
    h.frame(vec![enter]);
    h.frames(2);
    assert_eq!(target(&h), Some([200, 0x46, 0xA0]), "R typed");
    type_into(&mut h, "#C846A0", &own_hex);
    assert_eq!(target(&h), None, "own colour = unchanged");
}
