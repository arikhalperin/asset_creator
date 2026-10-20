//! Generates the whole-video extraction fixture in tests/fixtures/video:
//! `UPDATE_FIXTURES=1 cargo test --test video_extract_fixtures -- --ignored`
//! (needs the ffmpeg CLI with libx264).
//!
//! `walk.mp4`: 200 frames, 640×360, 30 fps, flat dark-blue background; a
//! static potion, and a skeleton that cycles an 8-pose walk (a new pose every
//! 3 frames) while moving right 1 px per frame. The generator checks that the
//! eight poses differ clearly from each other.
//!
//! `shapes.mp4`: flat-coloured static shapes and a thin-limbed 4-pose figure
//! (see `render_shapes_frame`), encoded at crf 18 — the hard case for dedup.
//! `moving.mp4`: the same scene with sub-pixel motion at crf 28.
//! `textured.mp4`: a finely striped 5-pose figure and a striped crate, crf 28.

use std::path::PathBuf;
use std::process::Command;

use image::{Rgba, RgbaImage};

pub const FRAMES: u32 = 200;
pub const SIZE: (u32, u32) = (640, 360);
const BG: [u8; 3] = [28, 36, 70];
const BONE: [f32; 3] = [232.0, 226.0, 205.0];
const DARK: [f32; 3] = [70.0, 60.0, 55.0];

/// Distance from point p to segment ab.
fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    ((p.0 - a.0 - t * vx).powi(2) + (p.1 - a.1 - t * vy).powi(2)).sqrt()
}

/// The skeleton in pose `k` (0…7) at local coordinates (origin top-left of a
/// 60×110 box): Some(color) when covered.
fn skeleton(k: u32, x: f32, y: f32) -> Option<[f32; 3]> {
    let phase = k as f32 / 8.0 * std::f32::consts::TAU;
    let hip = (30.0, 62.0);
    let shoulder = (30.0, 34.0);
    let limb = |angle: f32, len: f32, from: (f32, f32)| {
        (from.0 + angle.sin() * len, from.1 + angle.cos() * len)
    };
    // Skull with dark eye sockets.
    let skull = ((x - 30.0).powi(2) + (y - 14.0).powi(2)).sqrt() <= 12.0;
    if skull {
        let eye = ((x - 25.0).powi(2) + (y - 13.0).powi(2)).sqrt() <= 3.0
            || ((x - 35.0).powi(2) + (y - 13.0).powi(2)).sqrt() <= 3.0;
        return Some(if eye { DARK } else { BONE });
    }
    // Spine and ribcage (dark gaps between ribs).
    if (x - 30.0).abs() <= 3.0 && (24.0..=64.0).contains(&y) {
        return Some(BONE);
    }
    if (x - 30.0).abs() <= 13.0 && (34.0..=56.0).contains(&y) {
        let rib = ((y - 34.0) as i32 / 4) % 2 == 0;
        return Some(if rib { BONE } else { DARK });
    }
    // Limbs: left leg and left arm straight, right leg and right arm bent at
    // the knee / elbow, each with its own phase, so all eight poses differ.
    let (s, c) = (phase.sin(), phase.cos());
    let knee = limb(0.5 * c, 24.0, hip);
    let elbow = limb(0.45 * s, 16.0, shoulder);
    let parts = [
        (shoulder, limb(-0.45 * c, 28.0, shoulder), 3.0),
        (shoulder, elbow, 3.0),
        (elbow, limb(0.45 * s - 0.6, 14.0, elbow), 3.0),
        (hip, limb(0.5 * s, 44.0, hip), 3.5),
        (hip, knee, 3.5),
        (knee, limb(0.5 * c + 0.5, 22.0, knee), 3.5),
    ];
    for (a, b, r) in parts {
        if seg_dist((x, y), a, b) <= r {
            return Some(BONE);
        }
    }
    None
}

/// A potion (local 40×56 box).
fn potion(x: f32, y: f32) -> Option<[f32; 3]> {
    let body = ((x - 20.0) / 19.0).powi(2) + ((y - 36.0) / 19.0).powi(2) <= 1.0;
    let neck = (x - 20.0).abs() <= 5.0 && (6.0..=20.0).contains(&y);
    let cork = (x - 20.0).abs() <= 6.0 && (1.0..6.0).contains(&y);
    if cork {
        Some([170.0, 120.0, 70.0])
    } else if body || neck {
        let shade = 1.0 - ((x - 14.0).powi(2) + (y - 30.0).powi(2)).sqrt() / 60.0;
        Some([
            200.0 * shade + 30.0,
            40.0 * shade + 20.0,
            60.0 * shade + 30.0,
        ])
    } else {
        None
    }
}

/// Supersampled (3×3) draw of `shape` at offset (ox, oy) over `img`.
fn draw(
    img: &mut RgbaImage,
    ox: i32,
    oy: i32,
    w: u32,
    h: u32,
    shape: impl Fn(f32, f32) -> Option<[f32; 3]>,
) {
    for ly in 0..h {
        for lx in 0..w {
            let mut acc = [0f32; 3];
            let mut hits = 0;
            for sy in 0..3 {
                for sx in 0..3 {
                    let (fx, fy) = (
                        lx as f32 + (sx as f32 + 0.5) / 3.0,
                        ly as f32 + (sy as f32 + 0.5) / 3.0,
                    );
                    if let Some(c) = shape(fx, fy) {
                        hits += 1;
                        for k in 0..3 {
                            acc[k] += c[k];
                        }
                    }
                }
            }
            if hits == 0 {
                continue;
            }
            let (px, py) = (ox + lx as i32, oy + ly as i32);
            if px < 0 || py < 0 || px >= img.width() as i32 || py >= img.height() as i32 {
                continue;
            }
            let a = hits as f32 / 9.0;
            let under = *img.get_pixel(px as u32, py as u32);
            let p = [0, 1, 2]
                .map(|k| (acc[k] / hits as f32 * a + under[k] as f32 * (1.0 - a)).round() as u8);
            img.put_pixel(
                px as u32,
                py as u32,
                Rgba([p[0], p[1], p[2], under[3].max((a * 255.0) as u8)]),
            );
        }
    }
}

/// A thin-limbed figure in pose `k` of `n` (local 50×75 box).
fn figure(k: u32, n: u32, x: f32, y: f32) -> Option<[f32; 3]> {
    let body = [250.0, 200.0, 40.0];
    if ((x - 25.0).powi(2) + (y - 10.0).powi(2)).sqrt() <= 8.0 {
        return Some([120.0, 60.0, 20.0]);
    }
    if (x - 25.0).abs() <= 5.0 && (18.0..=45.0).contains(&y) {
        return Some(body);
    }
    let ph = k as f32 / n as f32 * std::f32::consts::TAU;
    let (shoulder, hip) = ((25.0, 22.0), (25.0, 45.0));
    let limb = |a: f32, l: f32, f: (f32, f32)| (f.0 + a.sin() * l, f.1 + a.cos() * l);
    let parts = [
        (shoulder, limb(1.2 * ph.sin() + 1.0, 20.0, shoulder)),
        (shoulder, limb(-1.0 - 0.8 * ph.cos(), 18.0, shoulder)),
        (hip, limb(0.6 * ph.sin(), 26.0, hip)),
        (hip, limb(-0.6 * ph.cos(), 26.0, hip)),
    ];
    parts
        .iter()
        .any(|&(a, b)| seg_dist((x, y), a, b) <= 3.0)
        .then_some(body)
}

/// `shapes.mp4`: green background; a red disc, a red square, and a blue
/// diamond (flat colours, symmetric) stay still while a thin-limbed figure
/// cycles 4 poses (a new one every 4 frames) moving left 1 px per frame.
pub fn render_shapes_frame(t: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(SIZE.0, SIZE.1, Rgba([60, 140, 70, 255]));
    draw(&mut img, 60, 60, 36, 36, |x, y| {
        (((x - 18.0).powi(2) + (y - 18.0).powi(2)).sqrt() <= 17.0).then_some([200.0, 40.0, 50.0])
    });
    draw(&mut img, 180, 60, 36, 36, |x, y| {
        ((2.0..34.0).contains(&x) && (2.0..34.0).contains(&y)).then_some([205.0, 45.0, 45.0])
    });
    draw(&mut img, 560, 280, 30, 36, |x, y| {
        ((x - 15.0).abs() / 15.0 + (y - 18.0).abs() / 18.0 <= 1.0).then_some([40.0, 80.0, 230.0])
    });
    draw(&mut img, 500 - t as i32, 200, 50, 75, |x, y| {
        figure((t / 4) % 4, 4, x, y)
    });
    img
}

/// Encodes `frames` (PNG sequence in a folder) to `out` with libx264.
fn encode(frames: &std::path::Path, crf: &str, out: &std::path::Path) {
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-framerate",
            "30",
            "-i",
        ])
        .arg(frames.join("%03d.png"))
        .args([
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", crf, "-g", "30",
        ])
        .arg(out)
        .status()
        .expect("ffmpeg CLI");
    assert!(status.success());
}

/// `moving.mp4`: like `shapes.mp4`, but the figure moves 1.5 px per frame
/// (sub-pixel positions) and the clip is encoded at crf 28 — heavy
/// compression, the hardest case for recognising repeated poses.
pub fn render_moving_frame(t: u32) -> RgbaImage {
    let mut img = render_shapes_frame_without_figure();
    let x = 500.0 - 1.5 * t as f32;
    let (ox, frac) = (x.floor() as i32, x - x.floor());
    draw(&mut img, ox, 200, 51, 75, |px, py| {
        figure((t / 4) % 4, 4, px - frac, py)
    });
    img
}

/// `textured.mp4`: a 5-pose figure with fine black/white stripes (2 px)
/// moving 1.3 px per frame at crf 28, next to a static striped crate —
/// fine textures that 4:2:0 compression smears.
pub fn render_textured_frame(t: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(SIZE.0, SIZE.1, Rgba([90, 60, 120, 255]));
    let stripe = |y: f32| {
        if (y as i32 / 2) % 2 == 0 {
            [245.0, 245.0, 245.0]
        } else {
            [20.0, 20.0, 20.0]
        }
    };
    draw(&mut img, 80, 220, 48, 48, |x, y| {
        ((3.0..45.0).contains(&x) && (3.0..45.0).contains(&y)).then(|| stripe(x + y))
    });
    let x = 520.0 - 1.3 * t as f32;
    let (ox, frac) = (x.floor() as i32, x - x.floor());
    draw(&mut img, ox, 150, 51, 75, |px, py| {
        figure((t / 4) % 5, 5, px - frac, py).map(|_| stripe(py))
    });
    img
}

fn render_shapes_frame_without_figure() -> RgbaImage {
    let mut img = RgbaImage::from_pixel(SIZE.0, SIZE.1, Rgba([60, 140, 70, 255]));
    draw(&mut img, 60, 60, 36, 36, |x, y| {
        (((x - 18.0).powi(2) + (y - 18.0).powi(2)).sqrt() <= 17.0).then_some([200.0, 40.0, 50.0])
    });
    draw(&mut img, 180, 60, 36, 36, |x, y| {
        ((2.0..34.0).contains(&x) && (2.0..34.0).contains(&y)).then_some([205.0, 45.0, 45.0])
    });
    draw(&mut img, 560, 280, 30, 36, |x, y| {
        ((x - 15.0).abs() / 15.0 + (y - 18.0).abs() / 18.0 <= 1.0).then_some([40.0, 80.0, 230.0])
    });
    img
}

pub fn pose_of_frame(t: u32) -> u32 {
    (t / 3) % 8
}

pub fn render_frame(t: u32) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(SIZE.0, SIZE.1, Rgba([BG[0], BG[1], BG[2], 255]));
    draw(&mut img, 80, 230, 40, 56, potion);
    let k = pose_of_frame(t);
    draw(&mut img, 200 + t as i32, 180, 60, 110, |x, y| {
        skeleton(k, x, y)
    });
    img
}

#[test]
#[ignore]
fn generate_video_extraction_fixture() {
    if std::env::var_os("UPDATE_FIXTURES").is_none() {
        return;
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/video");
    let frames = tempfile::tempdir().unwrap();
    for t in 0..FRAMES {
        render_frame(t)
            .save(frames.path().join(format!("{t:03}.png")))
            .unwrap();
    }
    encode(frames.path(), "14", &dir.join("walk.mp4"));
    let shapes = tempfile::tempdir().unwrap();
    for t in 0..FRAMES {
        render_shapes_frame(t)
            .save(shapes.path().join(format!("{t:03}.png")))
            .unwrap();
    }
    encode(shapes.path(), "18", &dir.join("shapes.mp4"));
    let moving = tempfile::tempdir().unwrap();
    for t in 0..FRAMES {
        render_moving_frame(t)
            .save(moving.path().join(format!("{t:03}.png")))
            .unwrap();
    }
    encode(moving.path(), "28", &dir.join("moving.mp4"));
    let textured = tempfile::tempdir().unwrap();
    for t in 0..FRAMES {
        render_textured_frame(t)
            .save(textured.path().join(format!("{t:03}.png")))
            .unwrap();
    }
    encode(textured.path(), "28", &dir.join("textured.mp4"));
    // Every pair of poses (rendered alone) must differ clearly (mean
    // difference ≥ 6 %), or the fixture is useless.
    let mut poses = Vec::new();
    for k in 0..8 {
        let mut img = RgbaImage::new(60, 110);
        draw(&mut img, 0, 0, 60, 110, |x, y| skeleton(k, x, y));
        poses.push(img);
    }
    for i in 0..8 {
        for j in i + 1..8 {
            let d: f64 = poses[i]
                .pixels()
                .zip(poses[j].pixels())
                .map(|(a, b)| {
                    (0..4)
                        .map(|c| (a[c] as f64 - b[c] as f64).abs())
                        .sum::<f64>()
                        / 4.0
                })
                .sum::<f64>()
                / poses[i]
                    .pixels()
                    .zip(poses[j].pixels())
                    .filter(|(a, b)| a[3] > 0 || b[3] > 0)
                    .count() as f64
                / 255.0;
            assert!(
                d >= 0.06,
                "poses {i} and {j} differ by only {:.1} %",
                d * 100.0
            );
        }
    }
}
