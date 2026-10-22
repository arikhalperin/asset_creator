//! Enhance Lines: thin dark lines (insignia outlines, panel and flap lines,
//! rivets) are found against their local background and made darker,
//! unbroken, and optionally bolder; print grain in the paint around them can
//! be smoothed. Works on CIELAB lightness with square morphology, scaled by
//! the image's pixels per millimetre.

use image::{GrayImage, Rgba, RgbaImage};

use crate::recolor::{lab, rgb, LabCache};

/// Darkness below the local background (L*) that makes a pixel a line.
pub const LINE_DARKNESS: f32 = 4.0;
/// Background radius: dark features thinner than about twice this are lines.
const BACKGROUND_MM: f32 = 0.6;
/// Radius of the paint that smooths print grain.
const SMOOTH_MM: f32 = 0.25;
/// Radius of the line colour that fills gaps and widened lines.
const COLOUR_MM: f32 = 1.0;
/// How much of its lightness a line loses at full strength.
const DARKEST: f32 = 0.85;
/// Below this scale a 3×3 median would erase thin lines.
const MEDIAN_MIN_PX_PER_MM: f32 = 10.0;
/// L* in plane units (0–255).
const L_SCALE: f32 = 2.55;

/// The four controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Line strength, 0–1.
    pub strength: f32,
    /// Longest gap closed, mm (0–2).
    pub gaps_mm: f32,
    /// Line width factor, 1–2.
    pub thickness: f32,
    /// Grain smoothing, 0–1.
    pub smooth: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            strength: 0.6,
            gaps_mm: 0.5,
            thickness: 1.0,
            smooth: 0.3,
        }
    }
}

impl Settings {
    /// Whether nothing would change.
    pub fn is_identity(&self) -> bool {
        self.strength <= 0.0 && self.gaps_mm <= 0.0 && self.thickness <= 1.0 && self.smooth <= 0.0
    }

    /// Pixels around an area the result depends on.
    pub fn margin_px(&self, px_per_mm: f32) -> u32 {
        ((BACKGROUND_MM * 2.0 + self.gaps_mm + COLOUR_MM + 1.0) * px_per_mm).ceil() as u32 + 4
    }
}

fn px(mm: f32, px_per_mm: f32) -> usize {
    (mm * px_per_mm).round().max(0.0) as usize
}

/// The dark-line mask of `img` (255 = line).
pub fn lines(img: &RgbaImage, px_per_mm: f32) -> GrayImage {
    let (w, h) = img.dimensions();
    let planes = Planes::new(img, px_per_mm);
    GrayImage::from_raw(w, h, planes.lines).expect("size")
}

/// The lightness planes and masks one enhancement needs.
struct Planes {
    w: usize,
    h: usize,
    /// Dark lines (0/255).
    lines: Vec<u8>,
    /// Light lines (0/255): never smoothed or used as paint.
    bright: Vec<u8>,
}

impl Planes {
    fn new(img: &RgbaImage, px_per_mm: f32) -> Self {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let raw = img.as_raw();
        let mut l = vec![0u8; w * h];
        par_rows(&mut l, w, |y0, band| {
            let mut cache = LabCache::new();
            for (k, v) in band.iter_mut().enumerate() {
                let i = (y0 * w + k) * 4;
                *v = (cache.lab([raw[i], raw[i + 1], raw[i + 2]])[0] * L_SCALE)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        });
        // A 3×3 median keeps 0.25 mm lines and drops grain and halftone dots;
        // a 3×3 mean of it calms the rest of the grain. Darkness is measured
        // on that same smooth copy (so paint edges and light lines, where it
        // ramps, are not dark lines), and a line pixel must be dark itself.
        // (At low resolution 3 px is wider than a line: no median then.)
        let med = if px_per_mm >= MEDIAN_MIN_PX_PER_MM {
            median3(&l, w, h)
        } else {
            l
        };
        let smooth = box_mean(&med, w, h, 1);
        let rb = px(BACKGROUND_MM, px_per_mm).max(1);
        let closed = morph(&morph(&smooth, w, h, rb, true), w, h, rb, false);
        let opened = morph(&morph(&smooth, w, h, rb, false), w, h, rb, true);
        let thr = (LINE_DARKNESS * L_SCALE).round() as i16;
        let mut lines = vec![0u8; w * h];
        let mut bright = vec![0u8; w * h];
        for i in 0..w * h {
            if raw[i * 4 + 3] < 128 {
                continue;
            }
            let dark = closed[i] as i16 - smooth[i] as i16;
            if dark >= thr && closed[i] as i16 - med[i] as i16 >= thr {
                lines[i] = 255;
            } else if smooth[i] as i16 - opened[i] as i16 >= thr {
                bright[i] = 255;
            }
        }
        // Lone specks (grain that got through) are not lines.
        let lines = despeckle(&lines, w, h);
        Self {
            w,
            h,
            lines,
            bright,
        }
    }
}

/// Enhances `img` (`px_per_mm` its scale); pixels where `allowed` is 0 are
/// unchanged. Lines get darker (their own hue), gaps up to `gaps_mm` close,
/// lines widen by `thickness`, and the paint's grain is smoothed.
pub fn enhance(
    img: &RgbaImage,
    settings: &Settings,
    px_per_mm: f32,
    allowed: Option<&GrayImage>,
) -> RgbaImage {
    if settings.is_identity() {
        return img.clone();
    }
    let planes = Planes::new(img, px_per_mm);
    let (w, h) = (planes.w, planes.h);
    let found = &planes.lines;
    // Gaps closed, then widened.
    let rg = px(settings.gaps_mm / 2.0, px_per_mm);
    let closed = if rg > 0 {
        morph(&morph(found, w, h, rg, true), w, h, rg, false)
    } else {
        found.clone()
    };
    let drawn = if settings.thickness > 1.0 {
        widen(&closed, w, h, settings.thickness)
    } else {
        closed
    };
    let raw = img.as_raw();
    // Colour for new line pixels: the line colour around them.
    let added: Vec<u8> = (0..w * h)
        .map(|i| u8::from(drawn[i] > 0 && found[i] == 0))
        .collect();
    let line_colour = if added.contains(&1) {
        masked_mean(raw, found, &added, w, h, px(COLOUR_MM, px_per_mm).max(1))
    } else {
        Vec::new()
    };
    // Paint (not lines, light lines, or transparent) for grain smoothing.
    let paint: Vec<u8> = if settings.smooth > 0.0 {
        let bright = morph(&planes.bright, w, h, 1, true);
        (0..w * h)
            .map(|i| u8::from(drawn[i] == 0 && bright[i] == 0 && raw[i * 4 + 3] >= 128) * 255)
            .collect()
    } else {
        Vec::new()
    };
    let smoothed = if settings.smooth > 0.0 {
        masked_mean(raw, &paint, &paint, w, h, px(SMOOTH_MM, px_per_mm).max(1))
    } else {
        Vec::new()
    };
    let f = 1.0 - DARKEST * settings.strength.clamp(0.0, 1.0);
    let m = settings.smooth.clamp(0.0, 1.0);
    let mut out = img.clone();
    let pixels: &mut [u8] = &mut out;
    par_rows(pixels, w * 4, |y0, band| {
        let mut cache = LabCache::new();
        for (k, p) in band.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let i = y0 * w + k;
            if allowed.is_some_and(|a| a.as_raw()[i] == 0) {
                continue;
            }
            if drawn[i] > 0 {
                let base = if found[i] > 0 {
                    if f >= 1.0 {
                        continue;
                    }
                    cache.lab([p[0], p[1], p[2]])
                } else {
                    let c = line_colour[i];
                    if c[3] == 0 {
                        continue;
                    }
                    lab([c[0], c[1], c[2]])
                };
                let c = rgb([base[0] * f, base[1] * f, base[2] * f]);
                // Widened edge pixels are partly covered by the line.
                let k = drawn[i] as f32 / 255.0;
                for ch in 0..3 {
                    p[ch] = (p[ch] as f32 + (c[ch] as f32 - p[ch] as f32) * k).round() as u8;
                }
            } else if m > 0.0 && paint[i] > 0 {
                let s = smoothed[i];
                if s[3] == 0 {
                    continue;
                }
                for c in 0..3 {
                    p[c] = (p[c] as f32 + (s[c] as f32 - p[c] as f32) * m).round() as u8;
                }
            }
        }
    });
    out
}

/// Widens the strokes of a 0/255 mask to `factor` × their typical width,
/// the same in every direction: the typical width comes from the Euclidean
/// distance of the strokes' centre pixels to their edge, and pixels within
/// the extra half-width (Euclidean) are added with their coverage (0–255).
fn widen(mask: &[u8], w: usize, h: usize, factor: f32) -> Vec<u8> {
    let dist = distance_sq(w, h, |i| mask[i] > 0);
    let width = stroke_width(mask, &dist);
    let extra = (factor - 1.0) * width / 2.0;
    if extra <= 0.0 {
        return mask.to_vec();
    }
    // A pixel d px from the stroke spans (d − 1, d] beyond its edge; the
    // widened stroke covers the part of that within `extra` (a ≤ 1 px soft
    // edge, so fractional widths and every angle come out right).
    dist.iter()
        .zip(mask)
        .map(|(&d, &m)| {
            if m > 0 {
                255
            } else {
                ((extra + 1.0 - d.sqrt()).clamp(0.0, 1.0) * 255.0).round() as u8
            }
        })
        .collect()
}

/// Typical stroke width (px) of a mask, the same at every angle: its area
/// over its length, the length from how much area a Euclidean dilation adds
/// between radius 2 and 3 (2 × length per px of radius for thin strokes).
/// `dist` is the squared distance of each pixel to the mask.
fn stroke_width(mask: &[u8], dist: &[f32]) -> f32 {
    let area = mask.iter().filter(|&&v| v > 0).count() as f32;
    let within = |r2: f32| dist.iter().filter(|&&d| d <= r2).count() as f32;
    let length = (within(9.0) - within(4.0)) / 2.0;
    if area == 0.0 || length <= 0.0 {
        return 0.0;
    }
    (area / length).max(1.0)
}

/// Squared Euclidean distance of every pixel to the nearest pixel where
/// `feature` holds (Felzenszwalb–Huttenlocher, separable and exact).
fn distance_sq(w: usize, h: usize, feature: impl Fn(usize) -> bool + Sync) -> Vec<f32> {
    const FAR: f32 = 1.0e10;
    let mut f: Vec<f32> = vec![0.0; w * h];
    par_rows(&mut f, w, |y0, band| {
        for (k, v) in band.iter_mut().enumerate() {
            *v = if feature(y0 * w + k) { 0.0 } else { FAR };
        }
    });
    let rows = dt_rows(&f, w);
    let t = transpose(&rows, w, h);
    let cols = dt_rows(&t, h);
    transpose(&cols, h, w)
}

/// The 1-D squared distance transform of every row (`row_len` long).
fn dt_rows(src: &[f32], row_len: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; src.len()];
    par_rows(&mut out, row_len, |y0, band| {
        let n = row_len;
        let (mut v, mut z) = (vec![0usize; n], vec![0.0f32; n + 1]);
        for (k, row) in band.chunks_exact_mut(n).enumerate() {
            let f = &src[(y0 + k) * n..(y0 + k + 1) * n];
            let mut j = 0usize;
            v[0] = 0;
            z[0] = f32::NEG_INFINITY;
            z[1] = f32::INFINITY;
            for q in 1..n {
                loop {
                    let p = v[j];
                    let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32))
                        / (2.0 * (q as f32 - p as f32));
                    // z[0] is −∞, so this stops at the first parabola.
                    if s <= z[j] {
                        j -= 1;
                        continue;
                    }
                    j += 1;
                    v[j] = q;
                    z[j] = s;
                    z[j + 1] = f32::INFINITY;
                    break;
                }
            }
            let mut j = 0usize;
            for (q, d) in row.iter_mut().enumerate() {
                while z[j + 1] < q as f32 {
                    j += 1;
                }
                let p = v[j];
                let dq = q as f32 - p as f32;
                *d = dq * dq + f[p];
            }
        }
    });
    out
}

/// Line pixels with fewer than two line neighbours are dropped.
fn despeckle(mask: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = mask.to_vec();
    par_rows(&mut out, w, |y0, band| {
        for (k, v) in band.iter_mut().enumerate() {
            if *v == 0 {
                continue;
            }
            let (x, y) = (k % w, y0 + k / w);
            let mut n = 0;
            for yy in y.saturating_sub(1)..(y + 2).min(h) {
                for xx in x.saturating_sub(1)..(x + 2).min(w) {
                    n += usize::from(mask[yy * w + xx] > 0);
                }
            }
            if n < 3 {
                *v = 0;
            }
        }
    });
    out
}

/// 3×3 median (edges clamped).
fn median3(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    par_rows(&mut out, w, |y0, band| {
        for (k, v) in band.iter_mut().enumerate() {
            let (x, y) = (k % w, y0 + k / w);
            let mut n = [0u8; 9];
            let mut j = 0;
            for dy in [-1i64, 0, 1] {
                let yy = (y as i64 + dy).clamp(0, h as i64 - 1) as usize;
                for dx in [-1i64, 0, 1] {
                    let xx = (x as i64 + dx).clamp(0, w as i64 - 1) as usize;
                    n[j] = src[yy * w + xx];
                    j += 1;
                }
            }
            n.sort_unstable();
            *v = n[4];
        }
    });
    out
}

/// Square box mean (radius `r`, edges averaged over what is inside).
fn box_mean(src: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    if r == 0 {
        return src.to_vec();
    }
    let rows = box_rows(src, w, h, r);
    let t = transpose(&rows, w, h);
    let cols = box_rows(&t, h, w, r);
    transpose(&cols, h, w)
}

fn box_rows(src: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    par_rows(&mut out, w, |y0, band| {
        for (k, row) in band.chunks_exact_mut(w).enumerate() {
            let line = &src[(y0 + k) * w..(y0 + k + 1) * w];
            let (mut sum, mut n) = (0u32, 0u32);
            for v in line.iter().take(r.min(w)) {
                sum += *v as u32;
                n += 1;
            }
            for x in 0..w {
                if x + r < w {
                    sum += line[x + r] as u32;
                    n += 1;
                }
                if x > r {
                    sum -= line[x - r - 1] as u32;
                    n -= 1;
                }
                row[x] = ((sum + n / 2) / n) as u8;
            }
        }
    });
    out
}

/// Square grey dilation (`max`) or erosion of a plane, radius `r`
/// (van Herk / Gil–Werman: constant time per pixel).
fn morph(src: &[u8], w: usize, h: usize, r: usize, max: bool) -> Vec<u8> {
    if r == 0 {
        return src.to_vec();
    }
    let rows = morph_rows(src, w, h, r, max);
    let t = transpose(&rows, w, h);
    let cols = morph_rows(&t, h, w, r, max);
    transpose(&cols, h, w)
}

fn morph_rows(src: &[u8], w: usize, h: usize, r: usize, max: bool) -> Vec<u8> {
    if r == 0 {
        return src.to_vec();
    }
    let mut out = vec![0u8; w * h];
    let op = |a: u8, b: u8| if max { a.max(b) } else { a.min(b) };
    let neutral = if max { 0 } else { 255 };
    let k = 2 * r + 1;
    par_rows(&mut out, w, |y0, band| {
        let n = w + 2 * r;
        let (mut f, mut g, mut s) = (vec![neutral; n], vec![0u8; n], vec![0u8; n]);
        for (j, row) in band.chunks_exact_mut(w).enumerate() {
            f[r..r + w].copy_from_slice(&src[(y0 + j) * w..(y0 + j + 1) * w]);
            for i in 0..n {
                g[i] = if i % k == 0 { f[i] } else { op(g[i - 1], f[i]) };
            }
            for i in (0..n).rev() {
                s[i] = if i == n - 1 || (i + 1) % k == 0 {
                    f[i]
                } else {
                    op(s[i + 1], f[i])
                };
            }
            for (x, v) in row.iter_mut().enumerate() {
                *v = op(s[x], g[x + 2 * r]);
            }
        }
    });
    out
}

fn transpose<T: Copy + Default>(src: &[T], w: usize, h: usize) -> Vec<T> {
    const B: usize = 64;
    let mut out = vec![T::default(); w * h];
    for by in (0..h).step_by(B) {
        for bx in (0..w).step_by(B) {
            for y in by..(by + B).min(h) {
                for x in bx..(bx + B).min(w) {
                    out[x * h + y] = src[y * w + x];
                }
            }
        }
    }
    out
}

/// Mean RGB of the `include` pixels in the box of radius `r` around each
/// `want` pixel (`[r, g, b, 1]`; `[.., 0]` when none or not wanted).
fn masked_mean(
    raw: &[u8],
    include: &[u8],
    want: &[u8],
    w: usize,
    h: usize,
    r: usize,
) -> Vec<[u8; 4]> {
    let mut out = vec![[0u8; 4]; w * h];
    par_rows(&mut out, w, |y0, band| {
        // Per column: sums over rows y-r..=y+r.
        let mut cols = vec![[0u32; 4]; w];
        let add_row = |cols: &mut [[u32; 4]], y: usize, remove: bool| {
            for (x, col) in cols.iter_mut().enumerate() {
                let i = y * w + x;
                if include[i] == 0 {
                    continue;
                }
                let v = [
                    raw[i * 4] as u32,
                    raw[i * 4 + 1] as u32,
                    raw[i * 4 + 2] as u32,
                    1,
                ];
                for c in 0..4 {
                    col[c] = if remove { col[c] - v[c] } else { col[c] + v[c] };
                }
            }
        };
        // Rows before y0's window, so the first step below can drop row y0-r-1.
        for y in y0.saturating_sub(r + 1)..(y0 + r).min(h) {
            add_row(&mut cols, y, false);
        }
        for (dy, row) in band.chunks_exact_mut(w).enumerate() {
            let y = y0 + dy;
            if y + r < h {
                add_row(&mut cols, y + r, false);
            }
            if y > r {
                add_row(&mut cols, y - r - 1, true);
            }
            let mut sum = [0u32; 4];
            for col in cols.iter().take(r.min(w)) {
                for c in 0..4 {
                    sum[c] += col[c];
                }
            }
            for x in 0..w {
                if x + r < w {
                    for c in 0..4 {
                        sum[c] += cols[x + r][c];
                    }
                }
                if x > r {
                    for c in 0..4 {
                        sum[c] -= cols[x - r - 1][c];
                    }
                }
                if want[y * w + x] == 0 || sum[3] == 0 {
                    continue;
                }
                let n = sum[3];
                row[x] = [
                    ((sum[0] + n / 2) / n) as u8,
                    ((sum[1] + n / 2) / n) as u8,
                    ((sum[2] + n / 2) / n) as u8,
                    1,
                ];
            }
        }
    });
    out
}

/// Runs `f(first_row, rows)` over bands of whole rows (`row_len` items each),
/// one band per core.
fn par_rows<T: Send>(data: &mut [T], row_len: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    if row_len == 0 || data.is_empty() {
        return;
    }
    let rows = data.len() / row_len;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let per = rows.div_ceil(threads).max(16);
    std::thread::scope(|scope| {
        for (k, band) in data.chunks_mut(per * row_len).enumerate() {
            let f = &f;
            scope.spawn(move || f(k * per, band));
        }
    });
}

/// Pixels per millimetre for a DPI.
pub fn px_per_mm(dpi: f32) -> f32 {
    dpi / 25.4
}

/// A synthetic test sheet (300 dpi): paint with grain, faded dark lines with
/// gaps, and light lines — see the spec's Fixture. Returns the image and its
/// ground truth.
pub fn test_sheet() -> (RgbaImage, TestSheet) {
    let ppm = px_per_mm(300.0);
    let (w, h) = ((90.0 * ppm) as u32, (60.0 * ppm) as u32);
    let dark_green = [62u8, 84, 52];
    let light_green = [104u8, 132, 84];
    let tan = [176u8, 132, 96];
    let white = [235u8, 235, 225];
    let mm = |v: f32| v * ppm;
    let half = mm(0.125);
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut noise = move || {
        seed = crate::filters::splitmix64(seed);
        // Roughly normal: sum of four uniforms, σ ≈ 4 L* ≈ 4 RGB levels.
        let u: f32 = (0..4)
            .map(|k| ((seed >> (k * 16)) & 0xFFFF) as f32 / 65535.0)
            .sum();
        (u - 2.0) * 6.9
    };
    // Horizontal flap lines (y mm) with gaps (start, end mm) along them.
    let flap_y = [10.0f32, 20.0];
    let gaps = [(20.0f32, 20.3f32), (40.0, 40.6), (60.0, 61.2)];
    // Insignia ring and white line.
    let (cx, cy, rr) = (mm(45.0), mm(42.0), mm(10.0));
    let white_y = mm(55.0);
    // A diagonal stroke (45°) on tan.
    let (da, db) = ((mm(65.0), mm(30.0)), (mm(85.0), mm(50.0)));
    let img = RgbaImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let paint = if fx < mm(30.0) {
            light_green
        } else if fx < mm(60.0) {
            dark_green
        } else {
            tan
        };
        let mut c = paint;
        for &ly in &flap_y {
            let in_gap = gaps.iter().any(|&(a, b)| fx >= mm(a) && fx < mm(b));
            if (fy - mm(ly)).abs() < half && fx > mm(5.0) && fx < mm(85.0) && !in_gap {
                c = lab_shift(paint, -10.0);
            }
        }
        let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
        if (d - rr).abs() < half {
            c = lab_shift(paint, -10.0);
        }
        // Distance to the diagonal segment.
        let (vx, vy) = (db.0 - da.0, db.1 - da.1);
        let t = (((fx - da.0) * vx + (fy - da.1) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
        let dd = ((fx - da.0 - t * vx).powi(2) + (fy - da.1 - t * vy).powi(2)).sqrt();
        if dd < half {
            c = lab_shift(paint, -10.0);
        }
        if (fy - white_y).abs() < mm(0.15) && fx > mm(35.0) && fx < mm(55.0) {
            c = white;
        }
        let n = noise();
        let g = |v: u8| (v as f32 + n).round().clamp(0.0, 255.0) as u8;
        Rgba([g(c[0]), g(c[1]), g(c[2]), 255])
    });
    (
        img,
        TestSheet {
            px_per_mm: ppm,
            flap_y: flap_y.to_vec(),
            gaps: gaps.to_vec(),
            ring: (45.0, 42.0, 10.0),
            diagonal: ((65.0, 30.0), (85.0, 50.0)),
            white_y: 55.0,
        },
    )
}

/// Ground truth of [`test_sheet`] (millimetres).
#[derive(Debug, Clone)]
pub struct TestSheet {
    pub px_per_mm: f32,
    /// Flap lines: y of each (from x 5 to 85 mm, 0.25 mm wide).
    pub flap_y: Vec<f32>,
    /// Gaps along each flap line (x from, x to).
    pub gaps: Vec<(f32, f32)>,
    /// Insignia ring: centre x, y, radius.
    pub ring: (f32, f32, f32),
    /// A 45° stroke on tan (from, to), 0.25 mm wide.
    pub diagonal: ((f32, f32), (f32, f32)),
    /// White 0.3 mm line from x 35 to 55 mm on dark green.
    pub white_y: f32,
}

/// `c` with its lightness moved by `dl` L* (same hue, chroma scaled).
fn lab_shift(c: [u8; 3], dl: f32) -> [u8; 3] {
    let l = lab(c);
    let k = (l[0] + dl) / l[0];
    rgb([l[0] + dl, l[1] * k, l[2] * k])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morphology_matches_brute_force() {
        let (w, h) = (37usize, 23usize);
        let src: Vec<u8> = (0..w * h)
            .map(|i| (crate::filters::splitmix64(i as u64) & 0xFF) as u8)
            .collect();
        for r in [1usize, 2, 5] {
            for max in [true, false] {
                let got = morph(&src, w, h, r, max);
                for y in 0..h {
                    for x in 0..w {
                        let mut want = if max { 0 } else { 255 };
                        for yy in y.saturating_sub(r)..(y + r + 1).min(h) {
                            for xx in x.saturating_sub(r)..(x + r + 1).min(w) {
                                let v = src[yy * w + xx];
                                want = if max { want.max(v) } else { want.min(v) };
                            }
                        }
                        assert_eq!(got[y * w + x], want, "r {r} max {max} at {x},{y}");
                    }
                }
            }
        }
    }

    #[test]
    fn dark_lines_are_found_and_light_ones_are_not() {
        let (img, t) = test_sheet();
        let m = lines(&img, t.px_per_mm);
        let at =
            |x: f32, y: f32| m.get_pixel((x * t.px_per_mm) as u32, (y * t.px_per_mm) as u32)[0];
        assert_eq!(at(10.0, t.flap_y[0]), 255, "flap line");
        assert_eq!(at(t.ring.0 + t.ring.2, t.ring.1), 255, "insignia ring");
        assert_eq!(at(45.0, t.white_y), 0, "white line is not a dark line");
        assert_eq!(at(10.0, 30.0), 0, "plain paint");
        let found = m.pixels().filter(|p| p[0] > 0).count() as f32;
        let area = m.width() as f32 * m.height() as f32;
        assert!(found / area < 0.05, "few false lines: {}", found / area);
    }

    /// At low resolution (5 px/mm) a 1 px line is found along its whole
    /// length (no median erasing it).
    #[test]
    fn thin_lines_at_low_resolution_are_found_whole() {
        let img = RgbaImage::from_fn(120, 60, |x, y| {
            if y == 30 && (10..110).contains(&x) {
                Rgba([90, 90, 90, 255])
            } else {
                Rgba([240, 240, 235, 255])
            }
        });
        let m = lines(&img, 5.0);
        assert!((12..108).all(|x| m.get_pixel(x, 30)[0] > 0));
        assert!(m.get_pixel(60, 20)[0] == 0);
    }

    #[test]
    fn distances_match_brute_force() {
        let (w, h) = (23usize, 17usize);
        let mask: Vec<bool> = (0..w * h)
            .map(|i| crate::filters::splitmix64(i as u64).is_multiple_of(11))
            .collect();
        let d = distance_sq(w, h, |i| mask[i]);
        for y in 0..h {
            for x in 0..w {
                let mut best = f32::MAX;
                for yy in 0..h {
                    for xx in 0..w {
                        if mask[yy * w + xx] {
                            let (dx, dy) = (x as f32 - xx as f32, y as f32 - yy as f32);
                            best = best.min(dx * dx + dy * dy);
                        }
                    }
                }
                assert_eq!(d[y * w + x], best, "at {x},{y}");
            }
        }
    }

    #[test]
    fn identity_settings_change_nothing() {
        let (img, t) = test_sheet();
        let s = Settings {
            strength: 0.0,
            gaps_mm: 0.0,
            thickness: 1.0,
            smooth: 0.0,
        };
        assert!(enhance(&img, &s, t.px_per_mm, None) == img);
    }
}
