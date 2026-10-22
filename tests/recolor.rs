//! Recolouring the generated paper-model sheets (pages rendered with
//! PDFium from `GAC_PDFIUM_DIR`).

use std::path::PathBuf;

use game_asset_creator::pdf::{self, render};
use game_asset_creator::recolor::{self, delta_e, lab, Recolor};
use image::RgbaImage;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/recolor")
        .join(name)
}

fn truth() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(fixture("recolor.json")).unwrap()).unwrap()
}

fn pages() -> Option<Vec<RgbaImage>> {
    let lib = pdf::pdfium::default_dir();
    if !pdf::pdfium::installed(&lib) {
        eprintln!("skipped: set GAC_PDFIUM_DIR");
        return None;
    }
    Some(
        (0..2)
            .map(|i| {
                render::render_page(&lib, &fixture("model_paint.pdf"), i)
                    .unwrap()
                    .image
            })
            .collect(),
    )
}

fn px(mm: f64) -> u32 {
    pdf::mm_to_px(mm as f32).round() as u32
}

fn at(img: &RgbaImage, x: u32, y: u32) -> [f32; 3] {
    let p = img.get_pixel(x, y);
    lab([p[0], p[1], p[2]])
}

/// Mean lab of a small square (to average away anti-aliasing).
fn mean(img: &RgbaImage, cx: u32, cy: u32, r: u32) -> [f32; 3] {
    let mut s = [0.0f32; 3];
    let mut n = 0.0;
    for y in cy - r..=cy + r {
        for x in cx - r..=cx + r {
            let l = at(img, x, y);
            for c in 0..3 {
                s[c] += l[c];
            }
            n += 1.0;
        }
    }
    [s[0] / n, s[1] / n, s[2] / n]
}

fn paint_lab(t: &serde_json::Value, name: &str) -> [f32; 3] {
    let c = &t["paints"][name];
    lab([
        c[0].as_u64().unwrap() as u8,
        c[1].as_u64().unwrap() as u8,
        c[2].as_u64().unwrap() as u8,
    ])
}

fn setup() -> Option<(Vec<RgbaImage>, Recolor, serde_json::Value)> {
    let pages = pages()?;
    let paper = recolor::paper_of(&pages[0]);
    let refs: Vec<&RgbaImage> = pages.iter().collect();
    let pal = recolor::palette(&refs, paper);
    Some((pages, Recolor::new(pal, paper), truth()))
}

#[test]
fn the_palette_holds_exactly_the_paints() {
    let Some((_, rc, t)) = setup() else { return };
    let names = ["olive", "sand", "grey", "lightblue"];
    assert_eq!(rc.swatches.len(), names.len(), "{:?}", rc.swatches);
    for name in names {
        let want = paint_lab(&t, name);
        let best = rc
            .swatches
            .iter()
            .map(|s| delta_e(s.lab, want))
            .fold(f32::MAX, f32::min);
        assert!(best < 8.0, "{name}: ΔE {best}");
    }
}

/// Olive → blue on both pages: olive paint becomes blue, everything else is
/// untouched, and rivets and panel lines keep their contrast.
#[test]
fn mapping_one_paint_keeps_everything_else_and_the_markings() {
    let Some((pages, mut rc, t)) = setup() else {
        return;
    };
    let olive_lab = paint_lab(&t, "olive");
    let olive = rc
        .swatches
        .iter()
        .position(|s| delta_e(s.lab, olive_lab) < 8.0)
        .unwrap();
    let blue = [40u8, 70, 160];
    rc.targets[olive] = Some(blue);
    let r = pdf::mm_to_px(0.5).round() as usize;
    for (pi, page) in pages.iter().enumerate() {
        let out = recolor::apply(page, &rc, r, None);
        for panel in t["pages"][pi]["panels"].as_array().unwrap() {
            let rect: Vec<f64> = panel["rect_mm"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            // A spot of plain paint (between rivets and lines).
            let (cx, cy) = (
                px(rect[0] + rect[2] / 6.0),
                px(rect[1] + rect[3] / 2.0 + 7.0),
            );
            let now = mean(&out, cx, cy, 3);
            if panel["paint"] == "olive" {
                assert!(
                    delta_e(now, lab(blue)) < 5.0,
                    "page {pi} olive → blue: ΔE {}",
                    delta_e(now, lab(blue))
                );
            } else {
                assert!(
                    delta_e(now, mean(page, cx, cy, 3)) < 1.0,
                    "page {pi} {} changed",
                    panel["paint"]
                );
            }
            // Rivet and panel-line contrast against the paint next to them.
            let ry = px(panel["rivets"][0][1].as_f64().unwrap());
            let rx = px(panel["rivets"][1][0].as_f64().unwrap());
            let line_x = px(panel["lines"][0][0].as_f64().unwrap());
            for (name, x, y, ref_x) in [
                ("rivet", rx, ry, rx + px(2.5)),
                ("line", line_x, cy, line_x + px(3.0)),
            ] {
                let was = at(page, ref_x, y)[0] - at(page, x, y)[0];
                let is = at(&out, ref_x, y)[0] - at(&out, x, y)[0];
                assert!(was > 5.0, "{name} visible in the fixture");
                assert!(
                    (is - was).abs() <= 0.1 * was,
                    "page {pi} {} {name}: contrast {is:.1} vs {was:.1}",
                    panel["paint"]
                );
            }
        }
        // Insignia and paper unchanged.
        let ins: Vec<f64> = t["pages"][pi]["insignia"][0]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let (sx, sy) = (px(ins[0] + ins[2] / 2.0), px(ins[1] + ins[3] / 2.0));
        assert_eq!(out.get_pixel(sx, sy), page.get_pixel(sx, sy), "insignia");
        assert_eq!(
            out.get_pixel(px(5.0), px(5.0)),
            page.get_pixel(px(5.0), px(5.0)),
            "paper"
        );
    }
}

/// Each paint mapped in turn, over the whole of both pages: nothing outside
/// that paint's panels (and their outlines) changes, and no paper or ink
/// pixel changes anywhere.
#[test]
fn mapping_each_paint_changes_only_that_paint() {
    let Some((pages, rc, t)) = setup() else {
        return;
    };
    let paper = lab(rc.paper.expect("paper"));
    let r = pdf::mm_to_px(0.5).round() as usize;
    let margin = pdf::mm_to_px(0.15) + 2.0;
    for name in ["olive", "sand", "grey", "lightblue"] {
        let s = rc
            .swatches
            .iter()
            .position(|s| delta_e(s.lab, paint_lab(&t, name)) < 8.0)
            .unwrap();
        let mut rc = rc.clone();
        rc.targets[s] = Some([200, 30, 120]);
        for (pi, page) in pages.iter().enumerate() {
            let rects: Vec<[f32; 4]> = t["pages"][pi]["panels"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["paint"] == name)
                .map(|p| {
                    let v: Vec<f32> = p["rect_mm"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| pdf::mm_to_px(v.as_f64().unwrap() as f32))
                        .collect();
                    [
                        v[0] - margin,
                        v[1] - margin,
                        v[0] + v[2] + margin,
                        v[1] + v[3] + margin,
                    ]
                })
                .collect();
            let out = recolor::apply(page, &rc, r, None);
            let (mut outside, mut paper_ink) = (0, 0);
            for (x, y, p) in page.enumerate_pixels() {
                let q = out.get_pixel(x, y);
                if p == q {
                    continue;
                }
                let was = lab([p[0], p[1], p[2]]);
                if delta_e(was, lab([q[0], q[1], q[2]])) < 1.0 {
                    continue;
                }
                let (fx, fy) = (x as f32, y as f32);
                if !rects
                    .iter()
                    .any(|b| fx >= b[0] && fx <= b[2] && fy >= b[1] && fy <= b[3])
                {
                    outside += 1;
                }
                if delta_e(was, paper) < recolor::PAPER_DE || was[0] < recolor::INK_L {
                    paper_ink += 1;
                }
            }
            assert_eq!(
                (outside, paper_ink),
                (0, 0),
                "{name} on page {pi}: pixels changed outside its panels, paper/ink pixels changed"
            );
        }
    }
}

#[test]
fn detail_strength_fades_or_boosts_markings() {
    let Some((pages, mut rc, t)) = setup() else {
        return;
    };
    let olive = rc
        .swatches
        .iter()
        .position(|s| delta_e(s.lab, paint_lab(&t, "olive")) < 8.0)
        .unwrap();
    rc.targets[olive] = Some([150, 60, 50]);
    let panel = &t["pages"][0]["panels"][0];
    assert_eq!(panel["paint"], "olive");
    let ry = px(panel["rivets"][0][1].as_f64().unwrap());
    let rx = px(panel["rivets"][1][0].as_f64().unwrap());
    let contrast = |img: &RgbaImage| at(img, rx + px(2.5), ry)[0] - at(img, rx, ry)[0];
    let was = contrast(&pages[0]);
    let r = pdf::mm_to_px(0.5).round() as usize;
    rc.detail = 0.0;
    let flat = contrast(&recolor::apply(&pages[0], &rc, r, None));
    assert!(flat.abs() < 0.1 * was, "detail 0: {flat:.1} vs {was:.1}");
    rc.detail = 1.5;
    let boosted = contrast(&recolor::apply(&pages[0], &rc, r, None));
    assert!(
        boosted > 1.3 * was,
        "detail 150 %: {boosted:.1} vs {was:.1}"
    );
}

#[test]
#[ignore]
fn bench_recolor() {
    let Some((pages, mut rc, _)) = setup() else {
        return;
    };
    let paper = recolor::paper_of(&pages[0]);
    let t = std::time::Instant::now();
    let pal = recolor::palette(&[&pages[0]], paper);
    let palette_ms = t.elapsed().as_secs_f64() * 1e3;
    rc.targets[0] = Some([40, 70, 160]);
    let t = std::time::Instant::now();
    let _ = recolor::apply(&pages[0], &rc, pdf::mm_to_px(0.5).round() as usize, None);
    let apply_ms = t.elapsed().as_secs_f64() * 1e3;
    eprintln!(
        "recolor: palette {palette_ms:.0} ms ({} swatches), apply A3 page {apply_ms:.0} ms",
        pal.len()
    );
    assert!(palette_ms < 2000.0 && apply_ms < 3000.0);
}
