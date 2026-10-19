//! Generates the extraction fixtures in tests/fixtures/extract:
//! `UPDATE_FIXTURES=1 cargo test --test extract_fixtures -- --ignored`.
//!
//! Sprite sheets (flat, near-flat, transparent background) with each sprite's
//! ground-truth bounds, and synthetic photos with ground-truth object masks.
//! Sprites are anti-aliased with 2×2 supersampling and every sprite color is
//! at least 50 % away from the background in some channel, so every covered
//! pixel survives keying at the default tolerance.

use std::path::PathBuf;

use image::{GrayImage, Luma, Rgba, RgbaImage};

type Shape = fn(f32, f32) -> bool;

/// Shapes in a unit square (x, y in 0…1).
fn shapes() -> Vec<(&'static str, Shape)> {
    fn disc(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> bool {
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    }
    vec![
        ("circle", |x, y| disc(x, y, 0.5, 0.5, 0.48)),
        ("square", |x, y| {
            (0.05..0.95).contains(&x) && (0.05..0.95).contains(&y)
        }),
        ("triangle", |x, y| {
            (0.05..=0.95).contains(&y) && (x - 0.5).abs() <= (y - 0.05) * 0.5
        }),
        ("diamond", |x, y| (x - 0.5).abs() + (y - 0.5).abs() <= 0.47),
        ("potion", |x, y| {
            disc(x, y, 0.5, 0.65, 0.32) || ((x - 0.5).abs() < 0.1 && (0.08..0.4).contains(&y))
        }),
        ("chalice", |x, y| {
            ((0.05..=0.45).contains(&y) && disc(x, y, 0.5, 0.05, 0.42))
                || ((x - 0.5).abs() < 0.06 && (0.4..0.85).contains(&y))
                || ((x - 0.5).abs() < 0.3 && (0.85..0.95).contains(&y))
        }),
        ("skull", |x, y| {
            disc(x, y, 0.5, 0.42, 0.38) || ((x - 0.5).abs() < 0.22 && (0.6..0.92).contains(&y))
        }),
        ("cross", |x, y| {
            ((x - 0.5).abs() < 0.12 && (0.05..0.95).contains(&y))
                || ((y - 0.4).abs() < 0.12 && (0.15..0.85).contains(&x))
        }),
        ("crescent", |x, y| {
            disc(x, y, 0.5, 0.5, 0.45) && !disc(x, y, 0.68, 0.42, 0.36)
        }),
        ("sword", |x, y| {
            ((x - 0.5).abs() < 0.035 && (0.03..0.72).contains(&y))
                || ((y - 0.74).abs() < 0.035 && (0.3..0.7).contains(&x))
                || ((x - 0.5).abs() < 0.05 && (0.74..0.97).contains(&y))
        }),
        ("spikes", |x, y| {
            let tooth = (x * 5.0).fract();
            ((0.8..=0.95).contains(&y) && (0.02..0.98).contains(&x))
                || ((0.02..0.98).contains(&x) && y < 0.8 && y >= 0.1 + (tooth - 0.5).abs() * 1.4)
        }),
        ("arc", |x, y| {
            disc(x, y, 0.5, 0.5, 0.46)
                && !disc(x, y, 0.5, 0.5, 0.3)
                && !(x > 0.5 && (y - 0.5).abs() < 0.15)
        }),
        ("key", |x, y| {
            (disc(x, y, 0.25, 0.5, 0.2) && !disc(x, y, 0.25, 0.5, 0.0))
                || ((y - 0.5).abs() < 0.05 && (0.4..0.95).contains(&x))
                || ((x - 0.85).abs() < 0.04 && (0.5..0.68).contains(&y))
        }),
        ("gem", |x, y| {
            let (dx, dy) = ((x - 0.5).abs(), (y - 0.5).abs());
            dy <= 0.43 && dx <= 0.47 && dx * 0.5 + dy <= 0.47
        }),
        ("heart", |x, y| {
            disc(x, y, 0.3, 0.33, 0.22)
                || disc(x, y, 0.7, 0.33, 0.22)
                || (y >= 0.33 && (x - 0.5).abs() <= (0.95 - y) * 0.8)
        }),
        ("arrow", |x, y| {
            ((y - 0.5).abs() < 0.07 && (0.05..0.7).contains(&x))
                || ((0.6..=0.95).contains(&x) && (y - 0.5).abs() <= (0.95 - x) * 1.1)
        }),
        // Two parts 2 px apart in a 40 px sprite: merged into one object.
        ("sparkle", |x, y| {
            disc(x, y, 0.35, 0.6, 0.3) || disc(x, y, 0.62, 0.17, 0.1)
        }),
        ("coin", |x, y| disc(x, y, 0.5, 0.5, 0.48)),
        ("bar", |x, y| {
            (0.0..1.0).contains(&x) && (0.35..0.65).contains(&y)
        }),
        ("ring", |x, y| {
            disc(x, y, 0.5, 0.5, 0.47) && !disc(x, y, 0.5, 0.5, 0.25)
        }),
    ]
}

const SHEET: (u32, u32) = (520, 400);
const CELL: (u32, u32) = (104, 100);
const BG: [u8; 3] = [255, 0, 255];

/// Sprite colors: far from magenta (green ≥ 140 or a dark color).
fn color(i: usize) -> ([u8; 3], [u8; 3]) {
    let palette = [
        [240, 200, 40],
        [60, 200, 90],
        [30, 40, 70],
        [230, 230, 230],
        [90, 160, 220],
        [200, 150, 60],
        [20, 120, 40],
        [250, 250, 120],
        [120, 220, 200],
        [40, 20, 20],
    ];
    let base = palette[i % palette.len()];
    let dark = base.map(|c| (c as u32 * 6 / 10) as u8);
    (base, dark)
}

fn far_from_bg(c: [u8; 3]) -> bool {
    (0..3).any(|k| (c[k] as i32 - BG[k] as i32).abs() >= 128)
}

/// Renders the sheet: (coverage 0…1 per pixel, straight color per pixel,
/// ground-truth boxes).
#[allow(clippy::type_complexity)]
fn render_sheet(with_ring: bool) -> (Vec<f32>, Vec<[u8; 3]>, Vec<(String, [u32; 4])>) {
    let (w, h) = SHEET;
    let mut coverage = vec![0f32; (w * h) as usize];
    let mut colors = vec![BG; (w * h) as usize];
    let mut truth = Vec::new();
    for (i, (name, shape)) in shapes().into_iter().enumerate() {
        if name == "ring" && !with_ring {
            continue;
        }
        let (col, row) = (i as u32 % 5, i as u32 / 5);
        // Size varies per sprite; "coin" is small, "sparkle" exactly 40 px.
        let size = match name {
            "coin" => 14,
            "sparkle" => 40,
            _ => 48 + (i as u32 * 7) % 40,
        };
        let (ox, oy) = (
            col * CELL.0 + (CELL.0 - size) / 2,
            row * CELL.1 + (CELL.1 - size) / 2,
        );
        let (base, dark) = color(i);
        assert!(
            far_from_bg(base) && far_from_bg(dark),
            "{name} color too close to the background"
        );
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..size {
            for x in 0..size {
                let mut hits = 0;
                for (sx, sy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                    if shape((x as f32 + sx) / size as f32, (y as f32 + sy) / size as f32) {
                        hits += 1;
                    }
                }
                if hits == 0 {
                    continue;
                }
                let (px, py) = (ox + x, oy + y);
                let i = (py * w + px) as usize;
                coverage[i] = hits as f32 / 4.0;
                // Shading: darker toward the bottom right.
                let t = (x + y) as f32 / (2 * size) as f32;
                colors[i] = [0, 1, 2]
                    .map(|k| (base[k] as f32 * (1.0 - t) + dark[k] as f32 * t).round() as u8);
                x0 = x0.min(px);
                y0 = y0.min(py);
                x1 = x1.max(px + 1);
                y1 = y1.max(py + 1);
            }
        }
        truth.push((name.to_owned(), [x0, y0, x1 - x0, y1 - y0]));
    }
    (coverage, colors, truth)
}

fn noise(x: u32, y: u32, k: u32) -> i32 {
    let mut v = (x.wrapping_mul(73_856_093)
        ^ y.wrapping_mul(19_349_663)
        ^ k.wrapping_mul(83_492_791)) as u64;
    v = v.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    ((v >> 40) % 11) as i32 - 5
}

fn write_json(path: &PathBuf, truth: &[(String, [u32; 4])], background: Option<[u8; 3]>) {
    let sprites: Vec<serde_json::Value> = truth
        .iter()
        .map(|(n, b)| serde_json::json!({"name": n, "x": b[0], "y": b[1], "w": b[2], "h": b[3]}))
        .collect();
    let json = serde_json::json!({"background": background, "sprites": sprites});
    std::fs::write(path, serde_json::to_string_pretty(&json).unwrap() + "\n").unwrap();
}

/// A photo-like scene: textured background, a shaded object with a soft
/// shadow, a slight blur and sensor noise. Returns (photo, truth, click).
fn render_photo(kind: &str) -> (RgbaImage, GrayImage, (u32, u32)) {
    let (w, h) = (640u32, 480u32);
    let ss = 3;
    // Object membership in image coordinates.
    let inside = |x: f32, y: f32| -> Option<[f32; 3]> {
        let disc = |cx: f32, cy: f32, rx: f32, ry: f32| {
            ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0
        };
        match kind {
            "potion" => {
                if disc(330.0, 290.0, 95.0, 110.0) {
                    let shade = 1.0 - ((x - 300.0).powi(2) + (y - 250.0).powi(2)).sqrt() / 260.0;
                    Some([
                        60.0 * shade + 20.0,
                        190.0 * shade + 20.0,
                        110.0 * shade + 20.0,
                    ])
                } else if (x - 330.0).abs() < 26.0 && (120.0..190.0).contains(&y) {
                    Some([230.0, 215.0, 175.0]) // pale cork
                } else {
                    None
                }
            }
            "chalice" => {
                let cup = (110.0..=230.0).contains(&y) && disc(320.0, 110.0, 110.0, 120.0);
                let stem = (x - 320.0).abs() < 14.0 && (225.0..360.0).contains(&y);
                let base = disc(320.0, 375.0, 80.0, 22.0);
                (cup || stem || base).then(|| {
                    let shade = 0.6 + 0.4 * (1.0 - ((x - 290.0).abs() / 120.0).min(1.0));
                    [235.0 * shade, 185.0 * shade, 50.0 * shade]
                })
            }
            _ => {
                // Skull: cranium + jaw, small dark eye sockets inside.
                let head = disc(320.0, 220.0, 95.0, 100.0)
                    || ((x - 320.0).abs() < 55.0 && (280.0..360.0).contains(&y));
                if !head {
                    return None;
                }
                if disc(290.0, 235.0, 6.0, 6.0) || disc(350.0, 235.0, 6.0, 6.0) {
                    Some([45.0, 35.0, 30.0])
                } else {
                    let shade = 0.75 + 0.25 * (1.0 - ((y - 180.0).abs() / 180.0).min(1.0));
                    Some([235.0 * shade, 225.0 * shade, 200.0 * shade])
                }
            }
        }
    };
    let background = |x: f32, y: f32| -> [f32; 3] {
        match kind {
            // Wooden table below a plain wall.
            "potion" => {
                if y > 300.0 {
                    let grain = ((y * 0.35 + (x * 0.02).sin() * 6.0).sin() * 0.5 + 0.5) * 30.0;
                    [120.0 + grain, 75.0 + grain * 0.6, 40.0]
                } else {
                    [150.0, 140.0, 165.0]
                }
            }
            // Dark stone blotches.
            "chalice" => {
                let v = 70.0
                    + ((x * 0.05).sin() * (y * 0.07).cos()) * 18.0
                    + ((x * 0.013 + y * 0.021).sin()) * 12.0;
                [v, v * 0.95, v * 1.05]
            }
            // Blue-gray cloth with a diagonal weave.
            _ => {
                let weave = (((x + y) * 0.25).sin() * 0.5 + 0.5) * 20.0;
                [60.0 + weave, 80.0 + weave, 110.0 + weave]
            }
        }
    };
    let shadow_center = match kind {
        "potion" => (345.0, 395.0),
        "chalice" => (335.0, 395.0),
        _ => (335.0, 360.0),
    };
    let mut photo = RgbaImage::new(w, h);
    let mut truth = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0f32; 3];
            let mut hits = 0;
            for sy in 0..ss {
                for sx in 0..ss {
                    let (fx, fy) = (
                        x as f32 + (sx as f32 + 0.5) / ss as f32,
                        y as f32 + (sy as f32 + 0.5) / ss as f32,
                    );
                    let c = match inside(fx, fy) {
                        Some(c) => {
                            hits += 1;
                            c
                        }
                        None => {
                            let mut c = background(fx, fy);
                            let d = ((fx - shadow_center.0) / 120.0).powi(2)
                                + ((fy - shadow_center.1) / 30.0).powi(2);
                            let dim = 1.0 - 0.45 * (-d).exp();
                            for v in &mut c {
                                *v *= dim;
                            }
                            c
                        }
                    };
                    for k in 0..3 {
                        acc[k] += c[k];
                    }
                }
            }
            let n = (ss * ss) as f32;
            let p = [0, 1, 2].map(|k| {
                (acc[k] / n + noise(x, y, k as u32) as f32)
                    .round()
                    .clamp(0.0, 255.0) as u8
            });
            photo.put_pixel(x, y, Rgba([p[0], p[1], p[2], 255]));
            truth.put_pixel(x, y, Luma([if hits * 2 >= ss * ss { 255 } else { 0 }]));
        }
    }
    // Slight lens blur.
    let photo = image::imageops::blur(&photo, 0.7);
    let click = match kind {
        "potion" => (330, 300),
        "chalice" => (320, 160),
        _ => (320, 200),
    };
    (photo, truth, click)
}

#[test]
#[ignore]
fn generate_extraction_fixtures() {
    if std::env::var_os("UPDATE_FIXTURES").is_none() {
        return;
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extract");
    std::fs::create_dir_all(&dir).unwrap();
    let (w, h) = SHEET;

    // Flat and near-flat sheets (no ring: its hole would be enclosed background).
    let (coverage, colors, truth) = render_sheet(false);
    for (file, noisy) in [("sheet_flat.png", false), ("sheet_noisy.png", true)] {
        let img = RgbaImage::from_fn(w, h, |x, y| {
            let i = (y * w + x) as usize;
            let a = coverage[i];
            let mut p = [0, 1, 2].map(|k| colors[i][k] as f32 * a + BG[k] as f32 * (1.0 - a));
            if noisy {
                for (k, v) in p.iter_mut().enumerate() {
                    *v += noise(x, y, k as u32) as f32 - 2.0 * (y as f32 / h as f32);
                }
            }
            let p = p.map(|v| v.round().clamp(0.0, 255.0) as u8);
            Rgba([p[0], p[1], p[2], 255])
        });
        img.save(dir.join(file)).unwrap();
    }
    write_json(&dir.join("sheet_flat.json"), &truth, Some(BG));
    write_json(&dir.join("sheet_noisy.json"), &truth, Some(BG));
    // Coverage map (for the Remove Background checks).
    GrayImage::from_fn(w, h, |x, y| {
        Luma([(coverage[(y * w + x) as usize] * 255.0).round() as u8])
    })
    .save(dir.join("sheet_coverage.png"))
    .unwrap();

    // Transparent sheet, with the ring.
    let (coverage, colors, truth) = render_sheet(true);
    RgbaImage::from_fn(w, h, |x, y| {
        let i = (y * w + x) as usize;
        let c = colors[i];
        Rgba([c[0], c[1], c[2], (coverage[i] * 255.0).round() as u8])
    })
    .save(dir.join("sheet_transparent.png"))
    .unwrap();
    write_json(&dir.join("sheet_transparent.json"), &truth, None);

    let mut photos = Vec::new();
    for kind in ["potion", "chalice", "skull"] {
        let (photo, truth, click) = render_photo(kind);
        photo.save(dir.join(format!("photo_{kind}.png"))).unwrap();
        truth
            .save(dir.join(format!("photo_{kind}_mask.png")))
            .unwrap();
        photos.push(serde_json::json!({
            "photo": format!("photo_{kind}.png"),
            "mask": format!("photo_{kind}_mask.png"),
            "click": [click.0, click.1],
        }));
    }
    std::fs::write(
        dir.join("photos.json"),
        serde_json::to_string_pretty(&serde_json::json!({ "photos": photos })).unwrap() + "\n",
    )
    .unwrap();
}
