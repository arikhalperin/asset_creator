//! The Parts Layout tab through the app (needs PDFium in `GAC_PDFIUM_DIR`;
//! the download test also needs `GAC_PDFIUM_ARCHIVE`, a folder holding the
//! pdfium-binaries archive).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Pos2, Vec2};
use game_asset_creator::extract::models::Source;
use game_asset_creator::parts_ui::PartsId;
use game_asset_creator::pdf::{self, pack::Placement};
use game_asset_creator::ui::{Consent, PdfConfig};

mod common;
use common::Harness;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pdf/model_a3.pdf")
}

fn installed() -> Option<PathBuf> {
    let dir = pdf::pdfium::default_dir();
    if pdf::pdfium::installed(&dir) {
        Some(dir)
    } else {
        eprintln!("skipped: set GAC_PDFIUM_DIR to a folder with libpdfium.dylib");
        None
    }
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

/// Opens the fixture and waits for both pages and their parts.
fn open(h: &mut Harness) -> PartsId {
    let lib = installed().unwrap();
    h.app.set_pdf_config(PdfConfig {
        dir: lib,
        source: Source::Directory(PathBuf::from("/nonexistent")),
    });
    assert_eq!(h.app.open_path(&fixture()), None, "PDFs open in a tab");
    let id = *h.app.parts_tabs().last().expect("a Parts Layout tab");
    wait_until(h, "pages", |h| !h.app.parts_tab(id).unwrap().loading());
    h.frames(2);
    id
}

#[test]
fn a_pdf_opens_in_a_parts_layout_tab() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let tab = h.app.parts_tab(id).unwrap();
    assert!(tab.error.is_none());
    assert_eq!(tab.pages.len(), 2);
    assert_eq!(tab.pages[0].image.dimensions(), (3508, 4961));
    // 20 parts + title fragments.
    assert!(tab.proposals.len() >= 20);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "model_a3.pdf — Parts Layout"));
    assert!(has_text(&out, "Page 1  —  297 × 420 mm"));
    assert!(has_text(&out, "Page 2  —  420 × 297 mm"));
    assert!(
        has_text(&out, "does not fit on A4: 344 × 75 mm") || has_text(&out, "does not fit on A4")
    );
    assert!(has_text(&out, "A4 sheet 1"));
}

#[test]
fn titles_are_deleted_parts_merged_and_the_layout_follows() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    // The title fragments: everything starting above 30 mm.
    let titles: Vec<usize> = h
        .app
        .parts_tab(id)
        .unwrap()
        .proposals
        .iter()
        .enumerate()
        .filter(|(_, p)| pdf::px_to_mm(p.rect.y as f32) < 30.0)
        .map(|(i, _)| i)
        .collect();
    assert!(!titles.is_empty());
    h.app.parts_tab_mut(id).unwrap().selected = titles.iter().copied().collect();
    click_text(&mut h, "Delete");
    let tab = h.app.parts_tab(id).unwrap();
    assert_eq!(tab.proposals.len(), 20);
    assert_eq!(tab.layout.items.len(), 20);
    // Merge two small parts of page 1 into one.
    h.app.parts_tab_mut(id).unwrap().selected = [4, 5].into();
    click_text(&mut h, "Merge");
    let tab = h.app.parts_tab(id).unwrap();
    assert_eq!(tab.proposals.len(), 19);
    assert_eq!(tab.layout.items.len(), 19);
    // Every part that fits is placed, legally.
    let l = &tab.layout;
    for (i, p) in l.placements.iter().enumerate() {
        if l.items[i].fits() {
            assert!(p.is_some(), "{} placed", l.items[i].label);
        }
    }
}

#[test]
fn the_wing_is_split_in_the_split_window() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let wing = h.app.parts_tab(id).unwrap().layout.oversized()[0];
    let before = h.app.parts_tab(id).unwrap().layout.items.len();
    h.app.parts_tab_mut(id).unwrap().item = Some(wing);
    click_text(&mut h, "Split…");
    h.frame(vec![]);
    let out = h.frame(vec![]);
    let title = h.app.parts_tab(id).unwrap().layout.items[wing]
        .label
        .clone();
    assert!(has_text(&out, &format!("Split Part {title}")));
    // Drag a vertical line through the middle of the picture.
    let hint = text_pos(&out, "Glue strip").unwrap();
    let picture_mid = Pos2::new(640.0, hint.y - 40.0);
    h.drag(
        picture_mid - Vec2::new(0.0, 30.0),
        picture_mid + Vec2::new(0.0, 25.0),
        5,
    );
    assert!(h
        .app
        .parts_tab(id)
        .unwrap()
        .split
        .as_ref()
        .unwrap()
        .line
        .is_some());
    // The strip goes on piece 2 (right of a downward line: the right half).
    let (w, _) = h.app.parts_tab(id).unwrap().layout.items[wing].size();
    click_text(&mut h, "Piece 2");
    assert_eq!(
        h.app.parts_tab(id).unwrap().split.as_ref().unwrap().glue_on,
        game_asset_creator::pdf::split::GlueOn::Second
    );
    click_text(&mut h, "Split");
    let tab = h.app.parts_tab(id).unwrap();
    assert!(tab.split.is_none());
    assert_eq!(tab.layout.items.len(), before + 1);
    let glue = pdf::mm_to_px(8.0);
    let first = tab.layout.items[wing].size().0 as f32;
    let second = tab.layout.items.last().unwrap().size().0 as f32;
    assert!(
        (first + second - w as f32 - glue).abs() < w as f32 * 0.02,
        "{first} + {second} vs {w} + strip"
    );
    assert!(second > first - 0.5 * glue, "piece 2 carries the strip");
    assert!(tab.layout.oversized().is_empty(), "both halves fit");
    assert!(tab.layout.placements.iter().all(Option::is_some));
    let out = h.frame(vec![]);
    assert!(!has_text(&out, "does not fit on A4"));
}

#[test]
fn parts_are_dragged_between_sheets_and_illegal_drops_refused() {
    use game_asset_creator::ui::Panel;
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    // A wide centre so two sheets show side by side.
    for p in [
        Panel::Tools,
        Panel::Layers,
        Panel::AssetLibrary,
        Panel::FrameSet,
    ] {
        h.app.set_panel_open(p, false);
    }
    let id = open(&mut h);
    h.frames(3);
    let tab = h.app.parts_tab(id).unwrap();
    assert!(tab.layout.sheets >= 2);
    let s1 = tab.sheet_rects.iter().find(|(s, _)| *s == 1).unwrap().1;
    assert!(
        s1.max.x <= common::SCREEN.x && s1.min.y < common::SCREEN.y - 100.0,
        "{s1:?}"
    );
    // A part on sheet 1 (index 0) and a free spot for it on sheet 2.
    let (i, r) = *tab
        .item_rects
        .iter()
        .find(|(i, _)| tab.layout.placements[*i].is_some_and(|p| p.sheet == 0))
        .unwrap();
    let before = tab.layout.placements[i].unwrap();
    let size = tab.layout.items[i].size();
    let mut probe = tab.layout.clone();
    probe.unplace(i);
    let (pw, ph) = pdf::pack::printable_px();
    let (w, hh) = if before.rotated {
        (size.1, size.0)
    } else {
        size
    };
    let mut spot = None;
    'find: for y in (0..ph.saturating_sub(hh)).step_by(40) {
        for x in (0..pw.saturating_sub(w)).step_by(40) {
            let place = Placement {
                sheet: 1,
                x,
                y,
                rotated: before.rotated,
            };
            if probe.move_item(i, place) {
                spot = Some(place);
                break 'find;
            }
        }
    }
    let spot = spot.expect("room on sheet 2");
    let (sw, _) = pdf::layout::sheet_px();
    let scale = s1.width() / sw as f32;
    let margin = pdf::mm_to_px(pdf::pack::MARGIN_MM).round();
    // Grab the part's centre; drop so its centre lands at the spot's centre
    // (a little inside it, to stay clear of rounding).
    let to = s1.min
        + Vec2::new(
            margin + spot.x as f32 + w as f32 / 2.0 + 4.0,
            margin + spot.y as f32 + hh as f32 / 2.0 + 4.0,
        ) * scale;
    h.drag(r.center(), to, 8);
    let after = h.app.parts_tab(id).unwrap().layout.placements[i].unwrap();
    assert_eq!(after.sheet, 1, "moved to sheet 2");
    // Moving onto another part is refused.
    let tab = h.app.parts_tab_mut(id).unwrap();
    let other = (0..tab.layout.items.len())
        .find(|&k| k != i && tab.layout.placements[k].is_some_and(|p| p.sheet == 1))
        .unwrap();
    let target = tab.layout.placements[other].unwrap();
    assert!(!tab.layout.move_item(
        i,
        Placement {
            rotated: after.rotated,
            ..target
        }
    ));
    // A drag onto another part through the UI is refused too.
    h.frames(2);
    let tab = h.app.parts_tab(id).unwrap();
    let r_now = tab.item_rects.iter().find(|(k, _)| *k == i).unwrap().1;
    let r_other = tab.item_rects.iter().find(|(k, _)| *k == other).unwrap().1;
    h.drag(r_now.center(), r_other.center(), 8);
    assert_eq!(
        h.app.parts_tab(id).unwrap().layout.placements[i].unwrap(),
        after
    );
    let out = h.frame(vec![]);
    assert!(has_text(&out, "does not fit there"));
    // Rotate and To Tray from the toolbar.
    h.app.parts_tab_mut(id).unwrap().item = Some(i);
    click_text(&mut h, "To Tray");
    assert!(h.app.parts_tab(id).unwrap().layout.placements[i].is_none());
    click_text(&mut h, "Place Tray Items");
    let p = h.app.parts_tab(id).unwrap().layout.placements[i].unwrap();
    click_text(&mut h, "Rotate 90°");
    let q = h.app.parts_tab(id).unwrap().layout.placements[i].unwrap();
    assert!(
        q.rotated != p.rotated
            || h.app.parts_tab(id).unwrap().status.as_deref() == Some("No room to turn it there")
    );
}

#[test]
fn exported_a4_pdf_has_one_page_per_sheet() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let wing = h.app.parts_tab(id).unwrap().layout.oversized()[0];
    let tab = h.app.parts_tab_mut(id).unwrap();
    let (w, hgt) = tab.layout.items[wing].size();
    assert!(tab.split_item(
        wing,
        (w as f32 / 2.0, -1.0),
        (w as f32 / 2.0, hgt as f32 + 1.0),
        8.0
    ));
    tab.pngs = true;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("a4.pdf");
    let files = tab.export(&out).unwrap();
    let sheets = tab.layout.sheets;
    assert_eq!(files.len(), 1 + sheets);
    let lib = installed().unwrap();
    assert_eq!(pdf::render::page_count(&lib, &out).unwrap(), sheets);
}

#[test]
fn the_pdf_engine_is_downloaded_only_after_consent() {
    if installed().is_none() {
        return;
    }
    // A damaged archive is refused and the app keeps working.
    let bad = tempfile::tempdir().unwrap();
    std::fs::write(
        bad.path().join(pdf::pdfium::PDFIUM.files[0].name),
        b"not an archive",
    )
    .unwrap();
    let target = tempfile::tempdir().unwrap();
    let mut h = Harness::new();
    h.app.set_pdf_config(PdfConfig {
        dir: target.path().join("pdfium"),
        source: Source::Directory(bad.path().to_path_buf()),
    });
    assert_eq!(h.app.open_path(&fixture()), None);
    assert!(
        h.app.parts_tabs().is_empty(),
        "nothing opens before consent"
    );
    assert_eq!(h.app.pdf_consent(), Consent::Asking);
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Download the PDF engine?"));
    click_text(&mut h, "Download");
    wait_until(&mut h, "download end", |h| {
        !h.app.pdf_download_in_progress()
    });
    h.frames(2);
    assert!(h.app.parts_tabs().is_empty());
    assert!(!pdf::pdfium::installed(&target.path().join("pdfium")));
    let out = h.frame(vec![]);
    assert!(has_text(&out, "failed verification") || has_text(&out, "could not be installed"));
    // With the real archive (when available): installed, then the PDF opens.
    let Some(archive) = std::env::var_os("GAC_PDFIUM_ARCHIVE").map(PathBuf::from) else {
        eprintln!("skipped the real download: set GAC_PDFIUM_ARCHIVE");
        return;
    };
    let mut h = Harness::new();
    h.app.set_pdf_config(PdfConfig {
        dir: target.path().join("pdfium"),
        source: Source::Directory(archive),
    });
    h.app.open_path(&fixture());
    assert_eq!(h.app.pdf_consent(), Consent::Asking);
    h.app.start_pdf_download();
    wait_until(&mut h, "installed and opened", |h| {
        !h.app.parts_tabs().is_empty()
    });
    assert!(pdf::pdfium::installed(&target.path().join("pdfium")));
}

/// The UI keeps drawing frames while pages render and parts are found.
#[test]
fn the_ui_stays_responsive_while_loading() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let lib = installed().unwrap();
    h.app.set_pdf_config(PdfConfig {
        dir: lib,
        source: Source::Directory(PathBuf::from("/nonexistent")),
    });
    let t = Instant::now();
    h.app.open_path(&fixture());
    assert!(
        t.elapsed() < Duration::from_millis(200),
        "open returns at once"
    );
    let id = *h.app.parts_tabs().last().unwrap();
    let mut frames = 0;
    let mut worst = Duration::ZERO;
    let mut saw_progress = false;
    while h.app.parts_tab(id).unwrap().loading() {
        let f = Instant::now();
        let out = h.frame(vec![]);
        worst = worst.max(f.elapsed());
        saw_progress |= has_text(&out, "Rendering and finding parts");
        frames += 1;
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(saw_progress && frames > 3, "{frames} frames");
    // Frame times are only meaningful in a release build (the debug suite
    // runs everything in parallel); the release run checks them.
    if !cfg!(debug_assertions) {
        assert!(worst < Duration::from_millis(250), "worst frame {worst:?}");
    }
}

/// With MobileSAM (`GAC_MODEL_DIR`) a click refines the part under it.
#[test]
fn mobilesam_refines_a_part() {
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
    let mut h = Harness::new();
    h.app.set_segment_config(segment::Config {
        runtime_paths: runtime,
        model_dir: models,
        source: Source::Directory(PathBuf::from("/nonexistent")),
    });
    let id = open(&mut h);
    assert!(h.app.parts_tab(id).unwrap().sam_ready);
    // Part "3" of page 1: the 60 × 90 mm panel at (210, 40) mm.
    let px = |mm: f32| pdf::mm_to_px(mm);
    let point = (px(240.0), px(85.0));
    let before = h.app.parts_tab(id).unwrap().proposals.len();
    h.app.refine_part(id, 0, point);
    wait_until(&mut h, "refined", |h| {
        h.app
            .parts_tab(id)
            .unwrap()
            .status
            .as_deref()
            .is_some_and(|s| s.starts_with("Refined"))
    });
    let tab = h.app.parts_tab(id).unwrap();
    assert_eq!(tab.proposals.len(), before, "replaced, not added");
    let k = *tab.selected.iter().next().unwrap();
    let r = tab.proposals[k].rect;
    assert!(r.x <= point.0 as u32 && r.right() >= point.0 as u32);
    assert!(r.y <= point.1 as u32 && r.bottom() >= point.1 as u32);
    // Inside the unfilled part "27" on page 2: replaced too, not duplicated.
    let blank = (px(310.0 + 55.0), px(240.0 + 10.0));
    let before = h.app.parts_tab(id).unwrap().proposals.len();
    h.app.parts_tab_mut(id).unwrap().status = None;
    h.app.refine_part(id, 1, blank);
    wait_until(&mut h, "refined inside", |h| {
        h.app
            .parts_tab(id)
            .unwrap()
            .status
            .as_deref()
            .is_some_and(|s| s.starts_with("Refined"))
    });
    assert_eq!(h.app.parts_tab(id).unwrap().proposals.len(), before);
}

/// A page in "Manual (A4 frames)" mode: its parts leave the layout, frames
/// are added, moved, and turned, and each frame exports as an A4 sheet of
/// exactly that page area.
#[test]
fn manual_pages_export_their_a4_frames() {
    use game_asset_creator::pdf::layout;
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let all = h.app.parts_tab(id).unwrap().layout.items.len();
    let page1 = h
        .app
        .parts_tab(id)
        .unwrap()
        .proposals
        .iter()
        .filter(|p| p.page == 1)
        .count();
    // The second "Manual (A4 frames)" checkbox belongs to page 2.
    let out = h.frame(vec![]);
    let boxes: Vec<Pos2> = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Manual (A4 frames)" => Some(t.pos),
            _ => None,
        })
        .collect();
    assert_eq!(boxes.len(), 2);
    let page2_box = boxes.iter().max_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    h.click_at(*page2_box + Vec2::new(4.0, 4.0));
    let tab = h.app.parts_tab(id).unwrap();
    assert!(tab.manual[1] && !tab.manual[0]);
    assert_eq!(
        tab.layout.items.len(),
        all - page1,
        "page 2's parts left the layout"
    );
    assert_eq!(tab.frames.len(), 1);
    assert_eq!(
        tab.frames[0],
        game_asset_creator::parts_ui::Frame {
            page: 1,
            x: 0,
            y: 0,
            rotated: false
        }
    );
    // Add a frame (next to the first), drag it, and turn it.
    click_text(&mut h, "Add Frame");
    h.frames(2);
    let tab = h.app.parts_tab(id).unwrap();
    assert_eq!(tab.frames.len(), 2);
    let (pw, _) = pdf::layout::sheet_px();
    assert_eq!((tab.frames[1].x, tab.frames[1].y), (pw as i32, 0));
    let r = tab.frame_rects.iter().find(|(f, _)| *f == 1).unwrap().1;
    let start = Pos2::new(r.min.x + 20.0, r.min.y + 20.0);
    h.drag(start, start + Vec2::new(-10.0, 12.0), 6);
    let f = h.app.parts_tab(id).unwrap().frames[1];
    assert!(f.x < pw as i32 && f.y > 0, "moved: {f:?}");
    assert_eq!(h.app.parts_tab(id).unwrap().frame, Some(1));
    click_text(&mut h, "Rotate Frame");
    let f = h.app.parts_tab(id).unwrap().frames[1];
    assert!(f.rotated);
    // Export: packed sheets then the two frame sheets.
    let tab = h.app.parts_tab(id).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let out_pdf = dir.path().join("a4.pdf");
    tab.export(&out_pdf).unwrap();
    let lib = installed().unwrap();
    let packed = tab.layout.sheets;
    assert_eq!(pdf::render::page_count(&lib, &out_pdf).unwrap(), packed + 2);
    for (k, frame) in tab.frames.iter().enumerate() {
        let want = layout::compose_frame(&tab.pages[1].image, frame.x, frame.y, frame.rotated);
        let got = pdf::render::render_page(&lib, &out_pdf, packed + k).unwrap();
        assert!((got.width_mm - 210.0).abs() < 0.05 && (got.height_mm - 297.0).abs() < 0.05);
        assert_eq!(got.image.dimensions(), want.dimensions());
        let diff: u64 = got
            .image
            .pixels()
            .zip(want.pixels())
            .map(|(a, b)| {
                (0..3)
                    .map(|c| (a[c] as i64 - b[c] as i64).unsigned_abs())
                    .sum::<u64>()
            })
            .sum();
        let mean = diff as f64 / (want.width() * want.height() * 3) as f64;
        assert!(mean < 1.5, "frame {k}: mean difference {mean:.2}");
    }
    // Overlapping frames are fine: the same area on two sheets.
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.frames[1] = game_asset_creator::parts_ui::Frame {
        page: 1,
        x: 100,
        y: 0,
        rotated: false,
    };
    assert_eq!(tab.sheets().len(), packed + 2);
    // Unmarking brings the parts back.
    h.click_at(*page2_box + Vec2::new(4.0, 4.0));
    let tab = h.app.parts_tab(id).unwrap();
    assert!(!tab.manual[1]);
    assert_eq!(tab.layout.items.len(), all);
    assert_eq!(tab.sheets().len(), tab.layout.sheets, "no frame sheets");
}

/// A click inside an unfilled part (on its printed number or its blank
/// inside) selects that part, never a separate label.
#[test]
fn clicks_inside_an_unfilled_part_select_it() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let tab = h.app.parts_tab(id).unwrap();
    let px = |mm: f32| pdf::mm_to_px(mm);
    // Part "27": a white 70 × 45 mm panel at (310, 240) mm, number in the middle.
    let label = (px(310.0 + 70.0 * 0.25 + 2.0), px(240.0 + 45.0 * 0.55 - 1.5));
    let blank = (px(310.0 + 55.0), px(240.0 + 10.0));
    let k = tab.proposal_at(1, label).expect("a part under the number");
    assert_eq!(tab.proposal_at(1, blank), Some(k));
    let r = tab.proposals[k].rect;
    assert!((pdf::px_to_mm(r.x as f32) - 310.0).abs() < 2.0);
    assert!((pdf::px_to_mm(r.width as f32) - 70.0).abs() < 2.0);
}

/// Empty paper inside a non-rectangular part's box is no part: a click there
/// selects nothing and a refine adds a new part, leaving that part alone.
#[test]
fn empty_paper_beside_a_tapered_part_is_not_that_part() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let px = |mm: f32| pdf::mm_to_px(mm);
    // Part "23" on page 2: a trapezoid from (20, 190) mm, 150 × 80, top 20 mm
    // narrower on each side: (23 mm, 193 mm) is inside its box, outside it.
    let corner = (px(23.0), px(193.0));
    let tab = h.app.parts_tab(id).unwrap();
    let trapezoid = tab
        .proposal_at(1, (px(95.0), px(260.0)))
        .expect("inside the trapezoid");
    let r = tab.proposals[trapezoid].rect;
    assert!(
        r.x <= corner.0 as u32 && r.y <= corner.1 as u32,
        "corner is in its box"
    );
    assert_eq!(tab.proposal_at(1, corner), None);
    // A refine there adds a part; the trapezoid stays.
    let before = tab.proposals.len();
    let trapezoid_mask = tab.proposals[trapezoid].clone();
    let (w, hgt) = tab.pages[1].image.dimensions();
    let mut mask = image::GrayImage::new(w, hgt);
    for y in corner.1 as u32 - 10..corner.1 as u32 + 10 {
        for x in corner.0 as u32 - 10..corner.0 as u32 + 10 {
            mask.put_pixel(x, y, image::Luma([255]));
        }
    }
    let tab = h.app.parts_tab_mut(id).unwrap();
    assert!(tab.apply_refine(1, corner, &mask));
    assert_eq!(tab.proposals.len(), before + 1);
    assert!(tab.proposals.contains(&trapezoid_mask));
}

/// With every page manual, the export holds only the frame sheets (no blank
/// packed sheet first).
#[test]
fn all_manual_pages_export_only_frames() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.selected = [0].into();
    tab.set_manual(0, true);
    assert!(tab.selected.is_empty(), "hidden proposals are deselected");
    tab.set_manual(1, true);
    assert!(tab.layout.items.is_empty());
    assert_eq!(tab.frames.len(), 2);
    assert_eq!(tab.sheets().len(), 2, "one sheet per frame, nothing else");
}

/// Export runs on a worker thread: frames keep coming with a progress bar,
/// and the finished PDF has every sheet.
#[test]
fn exporting_keeps_the_ui_responsive() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.set_manual(1, true);
    for _ in 0..5 {
        tab.add_frame(1);
    }
    tab.pngs = true;
    let total = tab.sheets().len();
    assert!(total >= 7);
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("a4.pdf");
    let t = Instant::now();
    assert!(tab.start_export(&out, std::sync::Arc::new(|| {})));
    assert!(t.elapsed() < Duration::from_millis(200), "returns at once");
    let mut worst = Duration::ZERO;
    let mut saw_progress = false;
    let mut frames = 0;
    while h.app.parts_tab(id).unwrap().export_progress().is_some() {
        let f = Instant::now();
        let shown = h.frame(vec![]);
        worst = worst.max(f.elapsed());
        saw_progress |= has_text(&shown, "Exporting sheet");
        frames += 1;
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(saw_progress && frames > 3, "{frames} frames");
    // Frame times are only meaningful in a release build (the debug suite
    // runs everything in parallel); the release run checks them.
    if !cfg!(debug_assertions) {
        assert!(worst < Duration::from_millis(250), "worst frame {worst:?}");
    }
    let status = h.app.parts_tab(id).unwrap().status.clone().unwrap();
    assert!(
        status.starts_with(&format!("Exported {} file(s)", total + 1)),
        "{status}"
    );
    let lib = installed().unwrap();
    assert_eq!(pdf::render::page_count(&lib, &out).unwrap(), total);
}

/// Cancel Export stops the worker and leaves no files behind.
#[test]
fn a_cancelled_export_leaves_nothing() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.set_manual(1, true);
    for _ in 0..8 {
        tab.add_frame(1);
    }
    tab.pngs = true;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("a4.pdf");
    assert!(tab.start_export(&out, std::sync::Arc::new(|| {})));
    h.frame(vec![]);
    let out_frame = h.frame(vec![]);
    assert!(has_text(&out_frame, "Cancel Export"));
    click_text(&mut h, "Cancel Export");
    wait_until(&mut h, "export stopped", |h| {
        h.app.parts_tab(id).unwrap().export_progress().is_none()
    });
    assert_eq!(
        h.app.parts_tab(id).unwrap().status.as_deref(),
        Some("Export cancelled")
    );
    let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");
}

/// Saves `img` as a PNG with a pHYs chunk for `dpi`.
fn save_png_dpi(path: &std::path::Path, img: &image::RgbaImage, dpi: f32) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width(), img.height());
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_pixel_dims(Some(png::PixelDimensions {
        xppu: (dpi / 0.0254).round() as u32,
        yppu: (dpi / 0.0254).round() as u32,
        unit: png::Unit::Meter,
    }));
    enc.write_header()
        .unwrap()
        .write_image_data(img.as_raw())
        .unwrap();
}

/// A folder of scans opens as a Parts Layout tab: sizes from the file's DPI
/// or A3, natural order, the same parts as the PDF, and a DPI override.
#[test]
fn a_folder_of_page_images_opens_like_a_pdf() {
    use game_asset_creator::pdf::images::SizeSource;
    let Some(lib) = installed() else { return };
    let pdf_pages: Vec<_> = (0..2)
        .map(|i| pdf::render::render_page(&lib, &fixture(), i).unwrap())
        .collect();
    let dir = tempfile::tempdir().unwrap();
    // "scan2" (PNG, 300 dpi) must come before "scan10" (JPEG, no DPI).
    save_png_dpi(&dir.path().join("scan2.png"), &pdf_pages[0].image, 300.0);
    image::DynamicImage::ImageRgba8(pdf_pages[1].image.clone())
        .to_rgb8()
        .save(dir.path().join("scan10.jpg"))
        .unwrap();
    std::fs::write(dir.path().join("readme.txt"), "not a page").unwrap();
    let mut h = Harness::new();
    let t = Instant::now();
    let id = h.app.open_image_folder(dir.path()).expect("a tab");
    assert!(
        t.elapsed() < Duration::from_millis(200),
        "loads in the background"
    );
    wait_until(&mut h, "pages", |h| !h.app.parts_tab(id).unwrap().loading());
    h.frames(2);
    let tab = h.app.parts_tab(id).unwrap();
    assert!(tab.error.is_none(), "{:?}", tab.error);
    assert_eq!(tab.pages.len(), 2);
    let names: Vec<String> = tab
        .image_pages
        .iter()
        .map(|p| {
            p.as_ref()
                .unwrap()
                .0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, ["scan2.png", "scan10.jpg"]);
    assert!(
        matches!(tab.image_pages[0].as_ref().unwrap().1, SizeSource::FileDpi(d) if (d - 300.0).abs() < 0.5)
    );
    assert_eq!(
        tab.image_pages[1].as_ref().unwrap().1,
        SizeSource::AssumedA3
    );
    assert!(
        (tab.pages[0].width_mm - 297.0).abs() < 1.0 && (tab.pages[0].height_mm - 420.0).abs() < 1.0
    );
    assert!(
        (tab.pages[1].width_mm - 420.0).abs() < 1.0 && (tab.pages[1].height_mm - 297.0).abs() < 1.0
    );

    // The same parts as the PDF pages (each fixture part within 2 mm).
    let truth: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixture().with_file_name("model_a3.json")).unwrap(),
    )
    .unwrap();
    for (i, page) in truth["pages"].as_array().unwrap().iter().enumerate() {
        for part in page["parts"].as_array().unwrap() {
            let r: Vec<f32> = part["rect_mm"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect();
            let found = tab.proposals.iter().any(|p| {
                let (w, hh) = p.size_mm();
                p.page == i
                    && (pdf::px_to_mm(p.rect.x as f32) - r[0]).abs() <= 2.0
                    && (pdf::px_to_mm(p.rect.y as f32) - r[1]).abs() <= 2.0
                    && (w - r[2]).abs() <= 2.0
                    && (hh - r[3]).abs() <= 2.0
            });
            assert!(found, "page {i} part {}", part["label"]);
        }
    }
    let out = h.frame(vec![]);
    assert!(has_text(&out, "(from file DPI (300 dpi))"));
    assert!(has_text(&out, "(assumed A3)"));
    // Override page 1's DPI: 150 dpi doubles its size; parts are found again.
    h.app.parts_tab_mut(id).unwrap().dpi_edit[0] = 150.0;
    let out = h.frame(vec![]);
    let apply = out
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Apply DPI" => Some(t.pos),
            _ => None,
        })
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    h.click_at(apply + Vec2::new(4.0, 4.0));
    wait_until(&mut h, "re-scaled", |h| {
        !h.app.parts_tab(id).unwrap().reloading()
    });
    h.frames(2);
    let tab = h.app.parts_tab(id).unwrap();
    assert_eq!(
        tab.image_pages[0].as_ref().unwrap().1,
        SizeSource::Override(150.0)
    );
    assert!(
        (tab.pages[0].width_mm - 594.0).abs() < 2.0,
        "{}",
        tab.pages[0].width_mm
    );
    // Every page-1 part is found again at twice its size (title text may
    // break into a different number of pieces).
    for part in truth["pages"][0]["parts"].as_array().unwrap() {
        let r: Vec<f32> = part["rect_mm"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| 2.0 * v.as_f64().unwrap() as f32)
            .collect();
        let found = tab.proposals.iter().any(|p| {
            let (w, hh) = p.size_mm();
            p.page == 0
                && (pdf::px_to_mm(p.rect.x as f32) - r[0]).abs() <= 4.0
                && (pdf::px_to_mm(p.rect.y as f32) - r[1]).abs() <= 4.0
                && (w - r[2]).abs() <= 4.0
                && (hh - r[3]).abs() <= 4.0
        });
        assert!(found, "re-scaled part {}", part["label"]);
    }
    // An empty folder: a message, no tab.
    let empty = tempfile::tempdir().unwrap();
    let tabs = h.app.parts_tabs().len();
    assert!(h.app.open_image_folder(empty.path()).is_none());
    assert_eq!(h.app.parts_tabs().len(), tabs);
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "No images in"));
}

/// Full-A4 frames: on an A3 portrait page the first frame is landscape, and
/// two frames tile the page exactly.
#[test]
fn two_a4_frames_tile_an_a3_page() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.set_manual(0, true);
    let second = tab.add_frame(0);
    let first = tab.frames.iter().position(|f| f.page == 0).unwrap();
    let (a, b) = (tab.frames[first], tab.frames[second]);
    assert!(
        a.rotated && b.rotated,
        "landscape frames on a portrait page"
    );
    let (fw, fh) = a.size();
    let (pw, ph) = tab.pages[0].image.dimensions();
    assert_eq!((a.x, a.y), (0, 0));
    assert_eq!((b.x, b.y), (0, fh as i32), "stacked");
    assert!(
        (fw as i32 - pw as i32).abs() <= 1,
        "width {fw} vs page {pw}"
    );
    assert!(
        ((2 * fh) as i32 - ph as i32).abs() <= 1,
        "2 × {fh} vs page {ph}"
    );
    // Their sheets are the two halves of the page, at true scale.
    let sheets = tab.sheets();
    let n = sheets.len();
    let top = &sheets[n - 2];
    let page = &tab.pages[0].image;
    // Landscape content turned clockwise: page (x, y) → sheet (fh-1-y, x).
    for (x, y) in [(10u32, 10u32), (3000, 2000), (100, 2400)] {
        assert_eq!(top.get_pixel(fh - 1 - y, x), page.get_pixel(x, y));
    }
}

/// The page view zooms with its buttons and with a Cmd+scroll / pinch over
/// the pages; frames scale with it.
#[test]
fn the_page_view_zooms() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    h.app.parts_tab_mut(id).unwrap().set_manual(1, true);
    h.frames(2);
    let width = |h: &Harness| h.app.parts_tab(id).unwrap().frame_rects[0].1.width();
    let w1 = width(&h);
    click_text(&mut h, "+");
    h.frames(2);
    assert!((h.app.parts_tab(id).unwrap().page_zoom - 1.25).abs() < 1e-3);
    assert!((width(&h) / w1 - 1.25).abs() < 0.02);
    // A zoom gesture over the pages.
    // A visible point of the page view (just under the Zoom controls).
    let out = h.frame(vec![]);
    let over = text_pos(&out, "Zoom").unwrap() + Vec2::new(40.0, 80.0);
    h.pointer = over;
    h.frame(vec![]);
    h.frame(vec![egui::Event::Zoom(2.0)]);
    h.frames(2);
    let z = h.app.parts_tab(id).unwrap().page_zoom;
    assert!((z - 2.5).abs() < 1e-3, "{z}");
    click_text(&mut h, "Fit");
    assert_eq!(h.app.parts_tab(id).unwrap().page_zoom, 1.0);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "100 %"));
}

/// Zoomed in, a slow frame drag follows the pointer exactly (no movement
/// lost to per-frame rounding).
#[test]
fn zoomed_frame_drags_follow_the_pointer() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.set_manual(0, true);
    tab.page_zoom = 8.0;
    h.frames(3);
    let tab = h.app.parts_tab(id).unwrap();
    let (fi, r) = tab.frame_rects[0];
    let before = tab.frames[fi];
    // Screen points per page pixel.
    let (fw, _) = before.size();
    let scale = r.width() / fw as f32;
    // 20 points in 40 steps of half a point.
    let start = r.min + Vec2::new(30.0, 30.0);
    h.drag(start, start + Vec2::new(20.0, 10.0), 40);
    let after = h.app.parts_tab(id).unwrap().frames[fi];
    let want = (20.0 / scale, 10.0 / scale);
    assert!(
        ((after.x - before.x) as f32 - want.0).abs() <= 2.0
            && ((after.y - before.y) as f32 - want.1).abs() <= 2.0,
        "moved {:?}, wanted {want:?}",
        (after.x - before.x, after.y - before.y)
    );
}

/// Hidden and unreadable files in a folder are skipped (and listed); the
/// readable images still open.
#[test]
fn unreadable_folder_images_are_skipped() {
    let Some(lib) = installed() else { return };
    let page = pdf::render::render_page(&lib, &fixture(), 0).unwrap();
    let dir = tempfile::tempdir().unwrap();
    save_png_dpi(&dir.path().join("scan1.png"), &page.image, 300.0);
    save_png_dpi(&dir.path().join("scan3.png"), &page.image, 300.0);
    std::fs::write(
        dir.path().join("._scan1.png"),
        b"\x00\x05\x16\x07resource fork",
    )
    .unwrap();
    let mut broken = std::fs::read(dir.path().join("scan1.png")).unwrap();
    broken.truncate(broken.len() / 3);
    std::fs::write(dir.path().join("scan2.png"), broken).unwrap();
    let mut h = Harness::new();
    let id = h.app.open_image_folder(dir.path()).unwrap();
    wait_until(&mut h, "pages", |h| !h.app.parts_tab(id).unwrap().loading());
    h.frames(2);
    let tab = h.app.parts_tab(id).unwrap();
    assert!(tab.error.is_none(), "{:?}", tab.error);
    assert_eq!(tab.pages.len(), 2, "scan1 and scan3");
    assert_eq!(tab.skipped.len(), 1);
    assert_eq!(tab.skipped[0].0, "scan2.png");
    assert!(
        tab.proposals.iter().any(|p| p.page == 1),
        "page indices stay consistent"
    );
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Skipped 1 unreadable file(s): scan2.png"));
    // Nothing readable at all: a message.
    let bad = tempfile::tempdir().unwrap();
    std::fs::write(bad.path().join("a.png"), b"nope").unwrap();
    let id = h.app.open_image_folder(bad.path()).unwrap();
    wait_until(&mut h, "done", |h| !h.app.parts_tab(id).unwrap().loading());
    assert_eq!(
        h.app.parts_tab(id).unwrap().error.as_deref(),
        Some("none of the images could be read")
    );
}

/// Cancelling an export never touches files from an earlier export.
#[test]
fn a_cancelled_export_keeps_earlier_files() {
    if installed().is_none() {
        return;
    }
    let mut h = Harness::new();
    let id = open(&mut h);
    let tab = h.app.parts_tab_mut(id).unwrap();
    tab.set_manual(1, true);
    for _ in 0..8 {
        tab.add_frame(1);
    }
    tab.pngs = true;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("a4.pdf");
    let first = tab.export(&out).unwrap();
    let before: Vec<(PathBuf, Vec<u8>)> = first
        .iter()
        .map(|p| (p.clone(), std::fs::read(p).unwrap()))
        .collect();
    let tab = h.app.parts_tab_mut(id).unwrap();
    assert!(tab.start_export(&out, std::sync::Arc::new(|| {})));
    tab.cancel_export();
    wait_until(&mut h, "export stopped", |h| {
        h.app.parts_tab(id).unwrap().export_progress().is_none()
    });
    for (p, bytes) in before {
        assert_eq!(std::fs::read(&p).unwrap(), bytes, "{} kept", p.display());
    }
    let extra: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
        .collect();
    assert!(extra.is_empty(), "no temporary files left: {extra:?}");
}
