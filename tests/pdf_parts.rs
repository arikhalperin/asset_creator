//! PDF parts layout on the synthetic A3 sheets (needs PDFium:
//! `GAC_PDFIUM_DIR`, or it is installed from the downloaded archive).

use std::path::PathBuf;

use game_asset_creator::pdf::{self, pack, parts, render};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pdf")
        .join(name)
}

fn lib() -> Option<PathBuf> {
    let dir = pdf::pdfium::default_dir();
    if pdf::pdfium::installed(&dir) {
        Some(dir)
    } else {
        eprintln!("skipped: set GAC_PDFIUM_DIR to a folder with libpdfium.dylib");
        None
    }
}

fn truth() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(fixture("model_a3.json")).unwrap()).unwrap()
}

#[test]
fn pages_render_at_300_dpi_with_their_size() {
    let Some(lib) = lib() else { return };
    let path = fixture("model_a3.pdf");
    assert_eq!(render::page_count(&lib, &path).unwrap(), 2);
    let p = render::render_page(&lib, &path, 0).unwrap();
    assert!((p.width_mm - 297.0).abs() < 0.1 && (p.height_mm - 420.0).abs() < 0.1);
    assert_eq!(p.image.dimensions(), (3508, 4961));
    let p = render::render_page(&lib, &path, 1).unwrap();
    assert_eq!(p.image.dimensions(), (4961, 3508), "landscape");
}

#[test]
fn every_part_is_found_within_2_mm() {
    let Some(lib) = lib() else { return };
    let truth = truth();
    let path = fixture("model_a3.pdf");
    for (i, page) in truth["pages"].as_array().unwrap().iter().enumerate() {
        let rendered = render::render_page(&lib, &path, i).unwrap();
        let t = std::time::Instant::now();
        let found = parts::detect(&rendered.image, i);
        eprintln!("page {i}: {} proposals in {:?}", found.len(), t.elapsed());
        let mut used = vec![false; found.len()];
        for part in page["parts"].as_array().unwrap() {
            let r: Vec<f32> = part["rect_mm"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect();
            let hit = found.iter().position(|f| {
                let fx = pdf::px_to_mm(f.rect.x as f32);
                let fy = pdf::px_to_mm(f.rect.y as f32);
                let (fw, fh) = f.size_mm();
                (fx - r[0]).abs() <= 2.0
                    && (fy - r[1]).abs() <= 2.0
                    && (fw - r[2]).abs() <= 2.0
                    && (fh - r[3]).abs() <= 2.0
            });
            let label = part["label"].as_str().unwrap();
            let k = hit.unwrap_or_else(|| panic!("page {i} part {label} {r:?} not found"));
            assert!(!used[k], "part {label} shares a proposal");
            used[k] = true;
        }
        // Everything else lies in the title line.
        let title: Vec<f32> = page["title_mm"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();
        for (k, f) in found.iter().enumerate().filter(|(k, _)| !used[*k]) {
            let fy = pdf::px_to_mm(f.rect.y as f32);
            assert!(
                fy >= title[1] - 2.0 && fy <= title[1] + title[3],
                "page {i}: extra proposal {k} at {:?}",
                f.rect
            );
        }
    }
}

#[test]
fn the_wing_does_not_fit_and_the_spine_turns() {
    let Some(lib) = lib() else { return };
    let rendered = render::render_page(&lib, &fixture("model_a3.pdf"), 1).unwrap();
    let found = parts::detect(&rendered.image, 1);
    let sizes: Vec<(u32, u32)> = found
        .iter()
        .map(|p| (p.rect.width, p.rect.height))
        .collect();
    let wing = found
        .iter()
        .position(|p| p.size_mm().0 > 330.0)
        .expect("wing");
    assert!(!pack::fits(sizes[wing].0, sizes[wing].1));
    let (placed, _) = pack::pack(&sizes);
    assert!(placed[wing].is_none());
    let spine = found
        .iter()
        .position(|p| (255.0..270.0).contains(&p.size_mm().0) && p.size_mm().1 < 40.0)
        .expect("spine");
    assert!(placed[spine].unwrap().rotated);
}

/// Render → detect → layout → split the wing → export → render the export
/// back: every placed part keeps its size (±0.5 mm).
#[test]
fn exported_sheets_are_true_scale() {
    use game_asset_creator::pdf::layout::{Item, Layout};
    let Some(lib) = lib() else { return };
    let path = fixture("model_a3.pdf");
    let mut items = Vec::new();
    for i in 0..2 {
        let page = render::render_page(&lib, &path, i).unwrap();
        for (k, part) in parts::detect(&page.image, i).iter().enumerate() {
            // Skip the title line.
            if pdf::px_to_mm(part.rect.y as f32) < 30.0 {
                continue;
            }
            items.push(Item {
                label: format!("{}.{}", i + 1, k + 1),
                image: part.image(&page.image),
            });
        }
    }
    assert_eq!(items.len(), 12 + 8);
    let mut layout = Layout::new(items);
    let over = layout.oversized();
    assert_eq!(over.len(), 1, "the wing");
    let wing = over[0];
    let (w, h) = layout.items[wing].size();
    assert!(layout.split(
        wing,
        (w as f32 / 2.0, -1.0),
        (w as f32 / 2.0, h as f32 + 1.0),
        8.0,
        game_asset_creator::pdf::split::GlueOn::Smaller
    ));
    assert!(layout.oversized().is_empty());
    layout.place_tray();
    assert!(layout.placements.iter().all(Option::is_some));
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("a4.pdf");
    let files = layout.export(&out, true).unwrap();
    assert_eq!(files.len(), 1 + layout.sheets);
    assert_eq!(render::page_count(&lib, &out).unwrap(), layout.sheets);
    for s in 0..layout.sheets {
        let page = render::render_page(&lib, &out, s).unwrap();
        assert!((page.width_mm - 210.0).abs() < 0.05 && (page.height_mm - 297.0).abs() < 0.05);
        let composed = layout.compose(s);
        assert_eq!(page.image.dimensions(), composed.dimensions());
        // Each part's printed box, measured on the re-rendered page.
        let margin = pdf::mm_to_px(pack::MARGIN_MM).round() as u32;
        for (item, place) in layout.items.iter().zip(&layout.placements) {
            let p = place.unwrap();
            if p.sheet != s {
                continue;
            }
            let (x, y, pw, ph) = p.rect(item.size());
            let (x0, y0) = (margin + x, margin + y);
            let mut bbox = (u32::MAX, u32::MAX, 0, 0);
            for yy in y0.saturating_sub(10)..(y0 + ph + 10).min(3508) {
                for xx in x0.saturating_sub(10)..(x0 + pw + 10).min(2480) {
                    let c = page.image.get_pixel(xx, yy);
                    if c[0] < 235 || c[1] < 235 || c[2] < 235 {
                        bbox = (
                            bbox.0.min(xx),
                            bbox.1.min(yy),
                            bbox.2.max(xx + 1),
                            bbox.3.max(yy + 1),
                        );
                    }
                }
            }
            let mw = pdf::px_to_mm((bbox.2 - bbox.0) as f32);
            let mh = pdf::px_to_mm((bbox.3 - bbox.1) as f32);
            let (ew, eh) = (pdf::px_to_mm(pw as f32), pdf::px_to_mm(ph as f32));
            assert!(
                (mw - ew).abs() <= 0.5 && (mh - eh).abs() <= 0.5,
                "{}: printed {mw:.2}×{mh:.2} mm, expected {ew:.2}×{eh:.2}",
                item.label
            );
        }
    }
}

#[test]
#[ignore]
fn bench_pdf() {
    use game_asset_creator::pdf::layout::{Item, Layout};
    let Some(lib) = lib() else { return };
    let path = fixture("model_a3.pdf");
    let t = std::time::Instant::now();
    let page = render::render_page(&lib, &path, 0).unwrap();
    let render_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = std::time::Instant::now();
    let found = parts::detect(&page.image, 0);
    let detect_ms = t.elapsed().as_secs_f64() * 1e3;
    let items: Vec<Item> = (0..100)
        .map(|i| Item {
            label: i.to_string(),
            image: found[i % found.len()].image(&page.image),
        })
        .collect();
    let t = std::time::Instant::now();
    let layout = Layout::new(items);
    let pack_ms = t.elapsed().as_secs_f64() * 1e3;
    eprintln!(
        "render A3 page {render_ms:.0} ms, detect {detect_ms:.0} ms, pack 100 parts {pack_ms:.0} ms ({} sheets)",
        layout.sheets
    );
    assert!(render_ms < 2000.0 && detect_ms < 2000.0 && pack_ms < 1000.0);
}
