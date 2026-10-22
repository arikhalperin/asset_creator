//! Enhance Lines on the generated test sheet: contrast and hue (E1), paint
//! kept and grain smoothed (E2), gaps (E3), thickness (E4), crisp edges (E5),
//! and an A3 benchmark (E9).

use game_asset_creator::enhance::{self, Settings, TestSheet};
use game_asset_creator::recolor::{delta_e, lab, rgb};
use image::RgbaImage;

fn l_at(img: &RgbaImage, x: u32, y: u32) -> [f32; 3] {
    let p = img.get_pixel(x, y);
    lab([p[0], p[1], p[2]])
}

fn px(t: &TestSheet, mm: f32) -> u32 {
    (mm * t.px_per_mm) as u32
}

/// Mean L* over a square (radius `r` px).
fn mean_l(img: &RgbaImage, cx: u32, cy: u32, r: u32) -> f32 {
    let mut s = 0.0;
    let mut n = 0.0;
    for y in cy - r..=cy + r {
        for x in cx - r..=cx + r {
            s += l_at(img, x, y)[0];
            n += 1.0;
        }
    }
    s / n
}

/// Darkest L* (mean over a 3 px wide strip) across a horizontal line at
/// (x mm, y mm): the line's lightness.
fn line_l(img: &RgbaImage, t: &TestSheet, x: f32, y: f32) -> f32 {
    let (cx, cy) = (px(t, x), px(t, y));
    (cy - 4..=cy + 4)
        .map(|yy| {
            (cx - 1..=cx + 1)
                .map(|xx| l_at(img, xx, yy)[0])
                .sum::<f32>()
                / 3.0
        })
        .fold(f32::MAX, f32::min)
}

/// Paint lightness 1.5 mm above (x, y).
fn paint_l(img: &RgbaImage, t: &TestSheet, x: f32, y: f32) -> f32 {
    mean_l(img, px(t, x), px(t, y - 1.5), 4)
}

fn hue(c: [f32; 3]) -> f32 {
    c[2].atan2(c[1]).to_degrees()
}

/// Spots on the flap lines (one per paint) and on the insignia ring.
fn spots(t: &TestSheet) -> Vec<(f32, f32, &'static str)> {
    vec![
        (12.0, t.flap_y[0], "flap on light green"),
        (48.0, t.flap_y[0], "flap on dark green"),
        (75.0, t.flap_y[1], "flap on tan"),
        (t.ring.0, t.ring.1 - t.ring.2, "insignia ring"),
    ]
}

#[test]
fn faded_lines_get_twice_the_contrast_and_keep_their_hue() {
    let (img, t) = enhance::test_sheet();
    let out = enhance::enhance(&img, &Settings::default(), t.px_per_mm, None);
    for (x, y, name) in spots(&t) {
        let was = paint_l(&img, &t, x, y) - line_l(&img, &t, x, y);
        let line = line_l(&out, &t, x, y);
        let is = paint_l(&out, &t, x, y) - line;
        assert!(was > 5.0, "{name}: visible in the sheet ({was:.1})");
        assert!(
            is >= 2.0 * was || line <= 20.0,
            "{name}: contrast {is:.1} vs {was:.1}"
        );
        // A darker shade of its own colour.
        let (cx, cy) = (px(&t, x), px(&t, y));
        let before = l_at(&img, cx, cy);
        let after = l_at(&out, cx, cy);
        let chroma = (before[1].powi(2) + before[2].powi(2)).sqrt();
        if chroma > 10.0 {
            let d = (hue(after) - hue(before) + 540.0) % 360.0 - 180.0;
            assert!(d.abs() < 15.0, "{name}: hue moved {d:.1}°");
        }
    }
    // Strength 0: lines unchanged.
    let flat = Settings {
        strength: 0.0,
        gaps_mm: 0.0,
        ..Settings::default()
    };
    let out = enhance::enhance(&img, &flat, t.px_per_mm, None);
    for (x, y, name) in spots(&t) {
        let (cx, cy) = (px(&t, x), px(&t, y));
        assert_eq!(out.get_pixel(cx, cy), img.get_pixel(cx, cy), "{name}");
    }
}

/// Whether (x, y) px is within 1 mm of a dark line of the sheet.
fn near_line(t: &TestSheet, x: u32, y: u32) -> bool {
    let (fx, fy) = (x as f32 / t.px_per_mm, y as f32 / t.px_per_mm);
    let flap = t
        .flap_y
        .iter()
        .any(|&ly| (fy - ly).abs() < 1.0 && fx > 4.0 && fx < 86.0);
    let d = ((fx - t.ring.0).powi(2) + (fy - t.ring.1).powi(2)).sqrt();
    flap || (d - t.ring.2).abs() < 1.0 || diagonal_distance(t, fx, fy) < 1.0
}

/// Distance (mm) from (x, y) mm to the sheet's diagonal stroke.
fn diagonal_distance(t: &TestSheet, x: f32, y: f32) -> f32 {
    let ((ax, ay), (bx, by)) = t.diagonal;
    let (vx, vy) = (bx - ax, by - ay);
    let k = (((x - ax) * vx + (y - ay) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    ((x - ax - k * vx).powi(2) + (y - ay - k * vy).powi(2)).sqrt()
}

/// Width (px) of a line crossing (cx, cy) mm, measured along the unit
/// direction (dx, dy): its darkness summed across the cut (in 0.25 px
/// steps) as a share of the line's full darkness, so soft edge pixels count
/// by how much of them is line.
fn width_along(img: &RgbaImage, t: &TestSheet, cx: f32, cy: f32, dx: f32, dy: f32) -> f32 {
    let p = t.px_per_mm;
    let at = |s: f32| {
        let (x, y) = (cx * p + dx * s, cy * p + dy * s);
        // Mean of a 3 px run along the line (perpendicular to the cut).
        let (ex, ey) = (-dy, dx);
        (-1i32..=1)
            .map(|k| {
                l_at(
                    img,
                    (x + ex * k as f32).round() as u32,
                    (y + ey * k as f32).round() as u32,
                )[0]
            })
            .sum::<f32>()
            / 3.0
    };
    let paint = (at(-1.5 * p) + at(1.5 * p)) / 2.0;
    let line = (-40..=40)
        .map(|k| at(k as f32 * 0.25))
        .fold(f32::MAX, f32::min);
    (-80..=80)
        .map(|k| ((paint - at(k as f32 * 0.25)) / (paint - line)).clamp(0.0, 1.0))
        .sum::<f32>()
        * 0.25
}

#[test]
fn paint_is_kept_and_grain_smoothed_but_light_lines_stay() {
    let (img, t) = enhance::test_sheet();
    let no_smooth = Settings {
        smooth: 0.0,
        ..Settings::default()
    };
    let out = enhance::enhance(&img, &no_smooth, t.px_per_mm, None);
    let mut changed = 0;
    for (x, y, p) in img.enumerate_pixels() {
        if near_line(&t, x, y) {
            continue;
        }
        let q = out.get_pixel(x, y);
        if p != q && delta_e(lab([p[0], p[1], p[2]]), lab([q[0], q[1], q[2]])) >= 1.0 {
            changed += 1;
        }
    }
    assert_eq!(changed, 0, "paint away from lines changed");
    // Full smoothing: grain halves, the paint keeps its colour.
    let full = Settings {
        smooth: 1.0,
        ..Settings::default()
    };
    let out = enhance::enhance(&img, &full, t.px_per_mm, None);
    let stats = |img: &RgbaImage| {
        let (x0, y0, x1, y1) = (px(&t, 8.0), px(&t, 26.0), px(&t, 25.0), px(&t, 34.0));
        let mut ls = Vec::new();
        let mut mean = [0.0f32; 3];
        for y in y0..y1 {
            for x in x0..x1 {
                let l = l_at(img, x, y);
                ls.push(l[0]);
                for c in 0..3 {
                    mean[c] += l[c];
                }
            }
        }
        let n = ls.len() as f32;
        let m = ls.iter().sum::<f32>() / n;
        let sd = (ls.iter().map(|v| (v - m).powi(2)).sum::<f32>() / n).sqrt();
        (sd, [mean[0] / n, mean[1] / n, mean[2] / n])
    };
    let (sd0, m0) = stats(&img);
    let (sd1, m1) = stats(&out);
    assert!(sd1 <= 0.5 * sd0, "grain {sd1:.2} vs {sd0:.2}");
    assert!(delta_e(m0, m1) < 2.0, "paint moved ΔE {}", delta_e(m0, m1));
    // The white line on dark green.
    for x in [40.0, 45.0, 50.0] {
        let (cx, cy) = (px(&t, x), px(&t, t.white_y));
        let d = delta_e(l_at(&img, cx, cy), l_at(&out, cx, cy));
        assert!(d < 2.0, "white line at {x} mm changed ΔE {d:.1}");
    }
}

#[test]
fn gaps_shorter_than_the_setting_close_and_longer_ones_stay() {
    let (img, t) = enhance::test_sheet();
    let y = t.flap_y[0];
    let darkness = |img: &RgbaImage, x: f32| paint_l(img, &t, x, y) - line_l(img, &t, x, y);
    let s = Settings::default(); // 0.5 mm
    let out = enhance::enhance(&img, &s, t.px_per_mm, None);
    let line = darkness(&out, 12.0);
    let (a, b) = t.gaps[0]; // 0.3 mm
    let mut x = a;
    while x < b {
        assert!(
            darkness(&out, x) >= 0.7 * line,
            "0.3 mm gap at {x:.2} mm: {:.1} vs {line:.1}",
            darkness(&out, x)
        );
        x += 0.05;
    }
    let (a, b) = t.gaps[2]; // 1.2 mm > 1.5 × 0.5 mm
    let mid = (a + b) / 2.0;
    assert!(darkness(&out, mid) < 0.3 * line, "1.2 mm gap stays open");
    // 0 mm: nothing is filled.
    let none = Settings { gaps_mm: 0.0, ..s };
    let out = enhance::enhance(&img, &none, t.px_per_mm, None);
    let (a, b) = t.gaps[0];
    assert!(
        darkness(&out, (a + b) / 2.0) < 0.3 * line,
        "no gap filled at 0 mm"
    );
}

/// Width (px) of the line at (x, y): pixels darker than halfway between the
/// paint and the line.
fn width(img: &RgbaImage, t: &TestSheet, x: f32, y: f32) -> usize {
    let paint = paint_l(img, t, x, y);
    let line = line_l(img, t, x, y);
    let mid = (paint + line) / 2.0;
    let (cx, cy) = (px(t, x), px(t, y));
    (cy - 12..=cy + 12)
        .filter(|&yy| {
            (cx - 1..=cx + 1)
                .map(|xx| l_at(img, xx, yy)[0])
                .sum::<f32>()
                / 3.0
                < mid
        })
        .count()
}

#[test]
fn thickness_widens_lines() {
    let (img, t) = enhance::test_sheet();
    let one = enhance::enhance(&img, &Settings::default(), t.px_per_mm, None);
    let two = enhance::enhance(
        &img,
        &Settings {
            thickness: 2.0,
            ..Settings::default()
        },
        t.px_per_mm,
        None,
    );
    for x in [12.0, 48.0, 75.0] {
        let y = if x < 60.0 { t.flap_y[0] } else { t.flap_y[1] };
        let (w0, w1, w2) = (
            width(&img, &t, x, y),
            width(&one, &t, x, y),
            width(&two, &t, x, y),
        );
        assert!(w1.abs_diff(w0) <= 1, "1.0× at {x}: {w1} vs {w0}");
        let ratio = w2 as f32 / w1 as f32;
        assert!((1.6..=2.4).contains(&ratio), "2.0× at {x}: {w2} vs {w1}");
    }
}

/// Thickness is the same in every direction: the insignia ring (all
/// angles) and the 45° stroke widen like the straight flap lines.
#[test]
fn thickness_is_the_same_for_curves_and_diagonals() {
    let (img, t) = enhance::test_sheet();
    let one = enhance::enhance(&img, &Settings::default(), t.px_per_mm, None);
    let two = enhance::enhance(
        &img,
        &Settings {
            thickness: 2.0,
            ..Settings::default()
        },
        t.px_per_mm,
        None,
    );
    let (cx, cy, r) = t.ring;
    // Each angle: the mean over nine cuts within ±4° (one rasterised cut is
    // too coarse for a 3 px line).
    let ring_width = |img: &RgbaImage, deg: f32| {
        (-4..=4)
            .map(|k| {
                let a = (deg + k as f32).to_radians();
                let (c, s) = (a.cos(), a.sin());
                width_along(img, &t, cx + r * c, cy + r * s, c, s)
            })
            .sum::<f32>()
            / 9.0
    };
    for deg in [0.0f32, 30.0, 45.0, 60.0, 90.0, 135.0, 225.0, 315.0] {
        let (w1, w2) = (ring_width(&one, deg), ring_width(&two, deg));
        let ratio = w2 / w1;
        assert!(
            (1.6..=2.4).contains(&ratio),
            "ring at {deg}°: {w2:.2} vs {w1:.2}"
        );
    }
    let ((ax, ay), (bx, by)) = t.diagonal;
    let (mx, my) = ((ax + bx) / 2.0, (ay + by) / 2.0);
    let n = std::f32::consts::FRAC_1_SQRT_2;
    let (w0, w1, w2) = (
        width_along(&img, &t, mx, my, n, -n),
        width_along(&one, &t, mx, my, n, -n),
        width_along(&two, &t, mx, my, n, -n),
    );
    assert!((w1 - w0).abs() <= 1.0, "diagonal at 1.0×: {w1} vs {w0}");
    let ratio = w2 / w1;
    assert!((1.6..=2.4).contains(&ratio), "diagonal: {w2} vs {w1}");
}

/// A 30 × 30 mm light-green sheet (300 dpi, light grain) with one faded
/// 0.25 mm line through the centre at `deg`.
fn line_sheet(deg: f32) -> (RgbaImage, TestSheet) {
    let (_, mut t) = enhance::test_sheet();
    let p = t.px_per_mm;
    let n = (30.0 * p) as u32;
    let paint = lab([104, 132, 84]);
    let k = (paint[0] - 10.0) / paint[0];
    let line = rgb([paint[0] - 10.0, paint[1] * k, paint[2] * k]);
    let base = rgb(paint);
    let (c, sn) = (deg.to_radians().cos(), deg.to_radians().sin());
    let mid = 15.0 * p;
    let img = RgbaImage::from_fn(n, n, |x, y| {
        let (fx, fy) = (x as f32 + 0.5 - mid, y as f32 + 0.5 - mid);
        let d = (fx * sn - fy * c).abs();
        let col = if d < 0.125 * p { line } else { base };
        let g = ((x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) % 5) as i16 - 2;
        let v = |v: u8| (v as i16 + g).clamp(0, 255) as u8;
        image::Rgba([v(col[0]), v(col[1]), v(col[2]), 255])
    });
    t.diagonal = ((15.0, 15.0), (15.0 + c, 15.0 + sn));
    (img, t)
}

/// Mean width of the line in `img` across its direction `deg`, over cuts
/// along it (mm offsets from the centre).
fn mean_width_at(img: &RgbaImage, t: &TestSheet, deg: f32, cx: f32, cy: f32) -> f32 {
    let (c, s) = (deg.to_radians().cos(), deg.to_radians().sin());
    let cuts = [-4.0f32, -2.0, -1.0, 0.0, 1.0, 2.0, 4.0];
    cuts.iter()
        .map(|&o| width_along(img, t, cx + c * o, cy + s * o, -s, c))
        .sum::<f32>()
        / cuts.len() as f32
}

/// Thickness on lines alone (as when only they are inside an area): every
/// angle widens by the factor, 1.5× and 2.0×.
#[test]
fn thickness_is_the_same_at_every_angle_for_isolated_lines() {
    for deg in [0.0f32, 15.0, 30.0, 45.0, 60.0, 75.0, 90.0, 120.0, 165.0] {
        let (img, t) = line_sheet(deg);
        let run = |f: f32| {
            let s = Settings {
                thickness: f,
                ..Settings::default()
            };
            let out = enhance::enhance(&img, &s, t.px_per_mm, None);
            mean_width_at(&out, &t, deg, 15.0, 15.0)
        };
        let w0 = mean_width_at(&img, &t, deg, 15.0, 15.0);
        let (w1, w15, w2) = (run(1.0), run(1.5), run(2.0));
        assert!((w1 - w0).abs() <= 1.0, "{deg}° at 1.0×: {w1:.2} vs {w0:.2}");
        assert!(
            (1.6..=2.4).contains(&(w2 / w1)),
            "{deg}° at 2.0×: {w2:.2} vs {w1:.2}"
        );
        assert!(
            (1.3..=1.75).contains(&(w15 / w1)),
            "{deg}° at 1.5×: {w15:.2} vs {w1:.2}"
        );
    }
}

/// The insignia ring and the diagonal enhanced on their own (an area
/// around each only) widen by the factor too.
#[test]
fn thickness_holds_for_the_ring_and_diagonal_alone() {
    let (sheet, t) = enhance::test_sheet();
    let p = t.px_per_mm;
    let crop = |x0: f32, y0: f32, x1: f32, y1: f32| {
        image::imageops::crop_imm(
            &sheet,
            (x0 * p) as u32,
            (y0 * p) as u32,
            ((x1 - x0) * p) as u32,
            ((y1 - y0) * p) as u32,
        )
        .to_image()
    };
    let two = Settings {
        thickness: 2.0,
        ..Settings::default()
    };
    // Ring, cropped 2 mm around it (coordinates shift by the crop origin).
    let (cx, cy, r) = t.ring;
    let (ox, oy) = (cx - r - 2.0, cy - r - 2.0);
    let ring = crop(ox, oy, cx + r + 2.0, cy + r + 2.0);
    let one = enhance::enhance(&ring, &Settings::default(), p, None);
    let dbl = enhance::enhance(&ring, &two, p, None);
    for deg in (0..360).step_by(30) {
        let a = (deg as f32).to_radians();
        let width = |img: &RgbaImage| {
            (-4..=4)
                .map(|k| {
                    let b = a + (k as f32).to_radians();
                    let (c, s) = (b.cos(), b.sin());
                    width_along(img, &t, cx - ox + r * c, cy - oy + r * s, c, s)
                })
                .sum::<f32>()
                / 9.0
        };
        let ratio = width(&dbl) / width(&one);
        assert!(
            (1.6..=2.4).contains(&ratio),
            "ring alone at {deg}°: {ratio:.2}"
        );
    }
    // Diagonal, cropped 2 mm around it.
    let ((ax, ay), (bx, by)) = t.diagonal;
    let (ox, oy) = (ax - 2.0, ay - 2.0);
    let diag = crop(ox, oy, bx + 2.0, by + 2.0);
    let one = enhance::enhance(&diag, &Settings::default(), p, None);
    let dbl = enhance::enhance(&diag, &two, p, None);
    let (mx, my) = ((ax + bx) / 2.0 - ox, (ay + by) / 2.0 - oy);
    let ratio = mean_width_at(&dbl, &t, 45.0, mx, my) / mean_width_at(&one, &t, 45.0, mx, my);
    assert!((1.6..=2.4).contains(&ratio), "diagonal alone: {ratio:.2}");
}

#[test]
fn enhanced_edges_are_crisp() {
    let (img, t) = enhance::test_sheet();
    let out = enhance::enhance(&img, &Settings::default(), t.px_per_mm, None);
    for x in [12.0, 48.0, 75.0] {
        let y = if x < 60.0 { t.flap_y[0] } else { t.flap_y[1] };
        let paint = paint_l(&out, &t, x, y);
        let line = line_l(&out, &t, x, y);
        let (cx, cy) = (px(&t, x), px(&t, y));
        // Pixels between 20 % and 80 % of the way from paint to line, per side.
        let between = |range: Vec<u32>| {
            range
                .into_iter()
                .filter(|&yy| {
                    let v = (cx - 1..=cx + 1)
                        .map(|xx| l_at(&out, xx, yy)[0])
                        .sum::<f32>()
                        / 3.0;
                    let k = (paint - v) / (paint - line);
                    (0.2..0.8).contains(&k)
                })
                .count()
        };
        let above = between((cy - 8..cy).collect());
        let below = between((cy + 1..=cy + 8).collect());
        assert!(above <= 2 && below <= 2, "at {x} mm: {above} / {below} px");
    }
}

/// Release benchmark: `cargo test --release --test enhance -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_enhance_a3_page() {
    let (sheet, t) = enhance::test_sheet();
    let (w, h) = ((297.0 * t.px_per_mm) as u32, (420.0 * t.px_per_mm) as u32);
    let page = RgbaImage::from_fn(w, h, |x, y| {
        *sheet.get_pixel(x % sheet.width(), y % sheet.height())
    });
    let start = std::time::Instant::now();
    let _ = enhance::enhance(
        &page,
        &Settings {
            thickness: 1.5,
            ..Settings::default()
        },
        t.px_per_mm,
        None,
    );
    let ms = start.elapsed().as_secs_f64() * 1e3;
    eprintln!("enhance: A3 page {ms:.0} ms");
    if !cfg!(debug_assertions) {
        assert!(ms < 3000.0, "A3 page {ms:.0} ms");
    }
}
