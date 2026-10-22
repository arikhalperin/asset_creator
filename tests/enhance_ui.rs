//! Enhance Lines through the app: the Parts Layout window (needs PDFium in
//! `GAC_PDFIUM_DIR`) and Filter ▸ Enhance Lines… in the editor.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Pos2, Vec2};
use game_asset_creator::canvas::ViewAction;
use game_asset_creator::enhance;
use game_asset_creator::extract::models::Source;
use game_asset_creator::parts_ui::PartsId;
use game_asset_creator::pdf;
use game_asset_creator::recolor::lab;
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

fn text_pos(out: &egui::FullOutput, needle: &str) -> Option<Pos2> {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text() == needle => Some(t.pos),
        _ => None,
    })
}

fn has_text(out: &egui::FullOutput, needle: &str) -> bool {
    out.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(t) => t.galley.text().contains(needle),
        _ => false,
    })
}

fn click_text(h: &mut Harness, needle: &str) {
    let out = h.frame(vec![]);
    let pos = text_pos(&out, needle).unwrap_or_else(|| panic!("no {needle:?} on screen"));
    h.click_at(pos + Vec2::new(4.0, 4.0));
}

fn mm(v: f64) -> u32 {
    pdf::mm_to_px(v as f32).round() as u32
}

fn l_at(img: &RgbaImage, x: u32, y: u32) -> f32 {
    let p = img.get_pixel(x, y);
    lab([p[0], p[1], p[2]])[0]
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

fn page_mm(h: &Harness, id: PartsId, x: f64, y: f64) -> Pos2 {
    let (_, rect, scale) = h.app.parts_tab(id).unwrap().page_rects[0];
    rect.min + Vec2::new(mm(x) as f32, mm(y) as f32) * scale
}

/// Darkest L* across the vertical panel line at x = 60 mm (±1 mm) at `y`.
fn line_l(img: &RgbaImage, y: f64) -> f32 {
    (mm(59.0)..=mm(61.0))
        .map(|x| l_at(img, x, mm(y)))
        .fold(f32::MAX, f32::min)
}

/// Parts Layout ▸ Enhance Lines…: a rectangle and a Shift-lasso on page 1,
/// live preview, Apply in the background, only the areas change, no
/// re-pack, export, Revert; the areas are not Recolour Area's.
#[test]
fn parts_layout_enhance_lines_in_drawn_areas() {
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
    let placements = h.app.parts_tab(id).unwrap().layout.placements.clone();
    let dir = tempfile::tempdir().unwrap();
    let export = |h: &Harness, name: &str| {
        let path = dir.path().join(name);
        h.app.parts_tab(id).unwrap().export(&path).unwrap();
        let sheets = pdf::render::page_count(&installed().unwrap(), &path).unwrap();
        (0..sheets)
            .map(|k| {
                pdf::render::render_page(&installed().unwrap(), &path, k)
                    .unwrap()
                    .image
            })
            .collect::<Vec<_>>()
    };
    let before = export(&h, "before.pdf");
    click_text(&mut h, "Enhance Lines…");
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Line strength") && has_text(&out, "Smooth grain"));
    assert!(has_text(&out, "Draw an area on a page first"));
    // A rectangle across the first olive panel's left panel line (x 60 mm).
    let (a, b) = (page_mm(&h, id, 50.0, 60.0), page_mm(&h, id, 70.0, 100.0));
    h.drag(a, b, 6);
    // A Shift-lasso (triangle) in the sand panel.
    h.modifiers = egui::Modifiers::SHIFT;
    let pts = [(170.0, 60.0), (220.0, 60.0), (195.0, 110.0), (170.0, 60.0)];
    h.pointer = page_mm(&h, id, pts[0].0, pts[0].1);
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
            h.pointer = page_mm(&h, id, x, y);
            h.frame(vec![]);
        }
    }
    let up = h.press(egui::PointerButton::Primary, false);
    h.frame(vec![up]);
    h.modifiers = egui::Modifiers::NONE;
    h.frames(2);
    let tab = h.app.parts_tab(id).unwrap();
    assert_eq!(tab.enhance_areas.len(), 2, "rectangle and lasso");
    assert!(tab.areas.is_empty(), "not Recolour Area's areas");
    // The preview follows a change within a frame.
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.enhance.as_mut().unwrap().settings.strength = 0.9;
    tab.enhance_changed();
    let f = Instant::now();
    h.frame(vec![]);
    let preview = f.elapsed();
    eprintln!("enhance preview frame {preview:?}");
    if !cfg!(debug_assertions) {
        assert!(preview < Duration::from_millis(200), "{preview:?}");
    }
    // Whole-page areas on both pages preview within the limit too (reduced
    // scale, budget shared by the pages).
    let full = vec![(0.0, 0.0), (3507.0, 0.0), (3507.0, 4960.0), (0.0, 4960.0)];
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.enhance_areas.push((0, full.clone()));
    tab.enhance_areas.push((1, full));
    tab.enhance.as_mut().unwrap().settings.thickness = 1.5;
    tab.enhance_changed();
    let f = Instant::now();
    h.frame(vec![]);
    let big = f.elapsed();
    eprintln!("two whole-page areas: preview frame {big:?}");
    if !cfg!(debug_assertions) {
        assert!(big < Duration::from_millis(200), "{big:?}");
    }
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.enhance_areas.truncate(2);
    tab.enhance.as_mut().unwrap().settings.thickness = 1.0;
    tab.enhance_changed();
    h.frame(vec![]);
    // Apply (the button) in the background.
    click_text(&mut h, "Apply");
    wait_until(&mut h, "enhanced", |h| {
        h.app.parts_tab(id).unwrap().recolor_progress().is_none()
    });
    let tab = h.app.parts_tab(id).unwrap();
    assert!(
        !tab.enhance_previewing(),
        "no preview stacked on the applied result"
    );
    let page = &tab.pages[0].image;
    let (was, is) = (line_l(&original[0], 80.0), line_l(page, 80.0));
    assert!(
        is < was - 5.0,
        "the panel line inside the area got darker: {is:.1} vs {was:.1}"
    );
    assert_eq!(
        line_l(page, 120.0),
        line_l(&original[0], 120.0),
        "the same line outside the area is unchanged"
    );
    // Every changed pixel is inside the rectangle or the lasso's box.
    let inside = |x: u32, y: u32| {
        (x >= mm(50.0) && x <= mm(70.0) && y >= mm(60.0) && y <= mm(100.0))
            || (x >= mm(170.0) && x <= mm(220.0) && y >= mm(60.0) && y <= mm(110.0))
    };
    let outside = page
        .enumerate_pixels()
        .filter(|(x, y, p)| *p != original[0].get_pixel(*x, *y) && !inside(*x, *y))
        .count();
    assert_eq!(outside, 0, "pixels changed outside the areas");
    assert!(
        tab.pages[1].image == original[1],
        "a page without areas is unchanged"
    );
    assert_eq!(tab.layout.placements, placements, "no re-pack");
    // The export carries the enhanced line (some sheet differs).
    let after = export(&h, "after.pdf");
    assert_eq!(after.len(), before.len());
    assert!(
        after.iter().zip(&before).any(|(a, b)| a != b),
        "the A4 export changed"
    );
    // Revert to Original restores the PDF render exactly.
    click_text(&mut h, "Revert to Original");
    let tab = h.app.parts_tab(id).unwrap();
    for (p, o) in tab.pages.iter().zip(&original) {
        assert!(p.image == *o, "page restored");
    }
    click_text(&mut h, "Clear Areas");
    assert!(h.app.parts_tab(id).unwrap().enhance_areas.is_empty());
}

/// Filter ▸ Enhance Lines… on a selection: preview, Cancel restores, OK is
/// one undo step, pixels outside the selection are unchanged.
#[test]
fn editor_enhance_lines_filter_on_the_selection() {
    let (sheet, t) = enhance::test_sheet();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sheet.png");
    sheet.save(&file).unwrap();
    let mut h = Harness::new();
    let id = h.app.open_path(&file).unwrap();
    assert!(h.app.state.set_document_dpi(id, 300.0));
    h.app.view_action(ViewAction::Fit);
    h.frames(2);
    // Select the left half.
    h.app.set_tool(Tool::RectSelect);
    h.frames(1);
    let (w, hh) = sheet.dimensions();
    let a = h.screen_of(id, Pos2::new(0.0, 0.0));
    let b = h.screen_of(id, Pos2::new(w as f32 / 2.0, hh as f32));
    h.drag(a, b, 5);
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
    let original = layer(&h);
    let steps = h.app.state.document(id).unwrap().undo_stack().undo_len();
    let open_menu = |h: &mut Harness| {
        for _ in 0..3 {
            click_text(h, "Filter");
            h.frame(vec![]);
            let out = h.frame(vec![]);
            if let Some(p) = text_pos(&out, "Enhance Lines…") {
                h.click_at(p + Vec2::new(4.0, 4.0));
                h.frame(vec![]);
                return;
            }
        }
        panic!("no Enhance Lines… in the Filter menu");
    };
    open_menu(&mut h);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Line strength (%)") && has_text(&out, "Close gaps (mm)"));
    let ppm = t.px_per_mm;
    let at = |x: f32, y: f32| ((x * ppm) as u32, (y * ppm) as u32);
    // The flap line on light green (left half) previews darker.
    let (lx, ly) = at(12.0, t.flap_y[0]);
    let darkest = |img: &RgbaImage| {
        (ly - 4..=ly + 4)
            .map(|y| l_at(img, lx, y))
            .fold(f32::MAX, f32::min)
    };
    assert!(darkest(&layer(&h)) < darkest(&original) - 5.0, "previewed");
    click_text(&mut h, "Cancel");
    assert!(layer(&h) == original, "Cancel restores");
    open_menu(&mut h);
    click_text(&mut h, "OK");
    h.frames(1);
    let after = layer(&h);
    assert!(darkest(&after) < darkest(&original) - 5.0, "applied");
    let doc = h.app.state.document(id).unwrap();
    assert_eq!(doc.undo_stack().undo_len(), steps + 1);
    assert_eq!(doc.undo_stack().undo_label(), Some("Enhance Lines"));
    // Right of the selection: unchanged (the flap line on tan too).
    let half = w / 2 + 2;
    let outside = after
        .enumerate_pixels()
        .filter(|(x, y, p)| *x >= half && *p != original.get_pixel(*x, *y))
        .count();
    assert_eq!(outside, 0, "outside the selection");
}
