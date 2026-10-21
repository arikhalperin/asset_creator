//! Repeating patterns: tile-period detection by autocorrelation, seamless
//! tiles, the seam metric, and tiled repetition.

use std::sync::Arc;

use image::{Rgba, RgbaImage};
use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

use crate::document::PixelRect;

/// Below this confidence (normalized autocorrelation at the period) the
/// region is not treated as repeating.
pub const MIN_CONFIDENCE: f32 = 0.7;
/// A period is accepted only where the image shifted by it differs by at
/// most this fraction of the difference between unrelated pixels.
pub const MAX_SHIFT_DIFFERENCE: f32 = 0.25;
/// A period's decorrelation must be at most this fraction of its
/// neighbourhood's (see `period`).
const DIP: f32 = 0.3;
/// Regions whose luminance varies less than this (standard deviation) are
/// flat: nothing repeats.
const MIN_CONTRAST: f32 = 2.0;
/// A seam ratio at or below this is seamless.
pub const SEAMLESS_RATIO: f32 = 1.15;
/// Default Make Seamless blend width (fraction of the tile).
pub const DEFAULT_BLEND: f32 = 0.25;

/// Result of [`detect`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detection {
    /// Whether a period repeating at least twice per axis was found.
    pub found: bool,
    /// Tile period (width, height); the whole region when not found.
    pub period: (u32, u32),
    /// Normalized autocorrelation at the period (0–1; lower axis).
    pub confidence: f32,
    /// Proposed tile, relative to the analyzed image.
    pub tile: PixelRect,
}

/// Luminance composited over neutral gray, mean removed.
fn luminance(img: &RgbaImage) -> Vec<f32> {
    let mut lum: Vec<f32> = img
        .pixels()
        .map(|p| {
            let a = p[3] as f32 / 255.0;
            let l = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
            l * a + 128.0 * (1.0 - a)
        })
        .collect();
    let mean = lum.iter().sum::<f32>() / lum.len().max(1) as f32;
    lum.iter_mut().for_each(|v| *v -= mean);
    lum
}

/// In-place 2-D FFT of a `w` × `h` row-major buffer.
fn fft2(
    buf: &mut [Complex32],
    w: usize,
    h: usize,
    row: &Arc<dyn Fft<f32>>,
    col: &Arc<dyn Fft<f32>>,
) {
    row.process(buf);
    let mut column = vec![Complex32::default(); h];
    for x in 0..w {
        for y in 0..h {
            column[y] = buf[y * w + x];
        }
        col.process(&mut column);
        for y in 0..h {
            buf[y * w + x] = column[y];
        }
    }
}

/// The smallest size ≥ `n` with only the prime factors 2, 3, and 5 (fast
/// FFT sizes without padding all the way to a power of two).
fn fft_size(n: usize) -> usize {
    (n..)
        .find(|&m| {
            let mut m = m;
            for f in [2, 3, 5] {
                while m % f == 0 {
                    m /= f;
                }
            }
            m == 1
        })
        .unwrap_or(n)
}

/// Normalized autocorrelation along both axes: `(horizontal[dx], vertical[dy])`
/// for lags `0..=w/2` and `0..=h/2`, each divided by its overlap and by the
/// zero-lag value.
fn axis_autocorrelation(lum: &[f32], w: usize, h: usize) -> (Vec<f32>, Vec<f32>) {
    let pw = fft_size(2 * w);
    let ph = fft_size(2 * h);
    let mut buf = vec![Complex32::default(); pw * ph];
    for y in 0..h {
        for x in 0..w {
            buf[y * pw + x] = Complex32::new(lum[y * w + x], 0.0);
        }
    }
    let mut planner = FftPlanner::new();
    let (row, col) = (planner.plan_fft_forward(pw), planner.plan_fft_forward(ph));
    fft2(&mut buf, pw, ph, &row, &col);
    buf.iter_mut()
        .for_each(|c| *c = Complex32::new(c.norm_sqr(), 0.0));
    let (row, col) = (planner.plan_fft_inverse(pw), planner.plan_fft_inverse(ph));
    fft2(&mut buf, pw, ph, &row, &col);
    let zero = buf[0].re / (w * h) as f32;
    if zero <= f32::EPSILON {
        return (vec![0.0; w / 2 + 1], vec![0.0; h / 2 + 1]);
    }
    // One lag past half the extent, so the last allowed lag (two repeats)
    // can be checked against its right neighbour.
    let horizontal = (0..=(w / 2 + 1).min(w - 1))
        .map(|dx| buf[dx].re / ((w - dx) * h) as f32 / zero)
        .collect();
    let vertical = (0..=(h / 2 + 1).min(h - 1))
        .map(|dy| buf[dy * pw].re / (w * (h - dy)) as f32 / zero)
        .collect();
    (horizontal, vertical)
}

/// Mean absolute luminance difference between the image and itself shifted
/// by `lag` along one axis (over the overlap).
fn shift_difference(lum: &[f32], w: usize, h: usize, lag: usize, vertical: bool) -> f32 {
    let mut sum = 0.0;
    if vertical {
        for y in 0..h - lag {
            let (a, b) = (
                &lum[y * w..(y + 1) * w],
                &lum[(y + lag) * w..(y + lag + 1) * w],
            );
            sum += a.iter().zip(b).map(|(p, q)| (p - q).abs()).sum::<f32>();
        }
        sum / ((h - lag) * w) as f32
    } else {
        for y in 0..h {
            let row = &lum[y * w..(y + 1) * w];
            sum += row[..w - lag]
                .iter()
                .zip(&row[lag..])
                .map(|(p, q)| (p - q).abs())
                .sum::<f32>();
        }
        sum / ((w - lag) * h) as f32
    }
}

/// The fundamental period along one axis, or `None`: the smallest lag past
/// the central peak (at least 4 px, at most half the extent) that
/// - is a local autocorrelation maximum of at least [`MIN_CONFIDENCE`],
/// - is a sharp peak: its decorrelation `1 − r` is at most [`DIP`] of the
///   median decorrelation of the lags around it (half to one and a half
///   times it), so ripples of smooth content are rejected, and
/// - really repeats: the image shifted by it differs by at most
///   [`MAX_SHIFT_DIFFERENCE`] of the difference between unrelated pixels.
///
/// Returns the lag and its autocorrelation.
fn period(
    profile: &[f32],
    lum: &[f32],
    w: usize,
    h: usize,
    vertical: bool,
) -> Option<(usize, f32)> {
    let n = profile.len();
    let mut start = 1;
    while start + 1 < n && profile[start + 1] < profile[start] {
        start += 1;
    }
    let sigma = (lum.iter().map(|v| v * v).sum::<f32>() / lum.len() as f32).sqrt();
    // Mean |a − b| for two unrelated samples of the image.
    let unrelated = 1.128 * sigma * std::f32::consts::SQRT_2;
    // At least two repeats: the period is at most half the extent.
    let extent = if vertical { h } else { w };
    let last = (extent / 2).min(n.saturating_sub(2));
    // Peaks need a neighbour on both sides (an end lag on a rising slope is
    // not a peak).
    let candidates = (start.max(4)..=last).filter(|&p| {
        profile[p] >= MIN_CONFIDENCE && profile[p] >= profile[p - 1] && profile[p] >= profile[p + 1]
    });
    for p in candidates.take(64) {
        let mut around: Vec<f32> = (start.max(2).max(p / 2)..=(p + p / 2).min(n - 1))
            .filter(|&l| l + 1 < p || l > p + 1)
            .map(|l| 1.0 - profile[l])
            .collect();
        if around.len() < 2 {
            continue;
        }
        around.sort_by(f32::total_cmp);
        let median = around[around.len() / 2];
        if 1.0 - profile[p] > DIP * median {
            continue;
        }
        let (lag, diff) = (p - 1..=(p + 1).min(last))
            .map(|lag| (lag, shift_difference(lum, w, h, lag, vertical)))
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        if diff <= MAX_SHIFT_DIFFERENCE * unrelated {
            return Some((lag, profile[lag]));
        }
    }
    None
}

/// Mean absolute luminance difference between columns `a` and `b` (or rows).
fn line_difference(lum: &[f32], w: usize, h: usize, a: usize, b: usize, vertical: bool) -> f32 {
    if vertical {
        (0..w)
            .map(|x| (lum[a * w + x] - lum[b * w + x]).abs())
            .sum::<f32>()
            / w as f32
    } else {
        (0..h)
            .map(|y| (lum[y * w + a] - lum[y * w + b]).abs())
            .sum::<f32>()
            / h as f32
    }
}

/// The tile origin along one axis: where the wrap-around edge (last line
/// against first) differs least, so the tile is as seamless as possible.
fn phase(lum: &[f32], w: usize, h: usize, p: usize, vertical: bool) -> usize {
    let extent = if vertical { h } else { w };
    (0..p.min(extent - p + 1))
        .min_by(|&a, &b| {
            let da = line_difference(lum, w, h, a + p - 1, a, vertical);
            let db = line_difference(lum, w, h, b + p - 1, b, vertical);
            da.total_cmp(&db)
        })
        .unwrap_or(0)
}

/// Detects the tile period of a repeating image and proposes the minimal
/// tile (smallest axis-aligned translation period). Needs at least two
/// repeats per axis; otherwise proposes the whole image.
pub fn detect(img: &RgbaImage) -> Detection {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let whole = Detection {
        found: false,
        period: (img.width(), img.height()),
        confidence: 0.0,
        tile: PixelRect::new(0, 0, img.width(), img.height()),
    };
    if w < 8 || h < 8 {
        return whole;
    }
    let lum = luminance(img);
    let contrast = (lum.iter().map(|v| v * v).sum::<f32>() / lum.len() as f32).sqrt();
    if contrast < MIN_CONTRAST {
        return whole;
    }
    let (horizontal, vertical) = axis_autocorrelation(&lum, w, h);
    let (Some((pw, cx)), Some((ph, cy))) = (
        period(&horizontal, &lum, w, h, false),
        period(&vertical, &lum, w, h, true),
    ) else {
        return whole;
    };
    let confidence = cx.min(cy).clamp(0.0, 1.0);
    if confidence < MIN_CONFIDENCE {
        return Detection {
            confidence,
            ..whole
        };
    }
    let ox = phase(&lum, w, h, pw, false);
    let oy = phase(&lum, w, h, ph, true);
    Detection {
        found: true,
        period: (pw as u32, ph as u32),
        confidence,
        tile: PixelRect::new(ox as u32, oy as u32, pw as u32, ph as u32),
    }
}

/// Seam metric: mean absolute RGB difference across the wrap-around edges
/// (last column against first, last row against first) divided by the mean
/// difference between adjacent interior columns and rows. About 1 or less
/// is seamless; a visible seam is well above 1.
pub fn seam_ratio(tile: &RgbaImage) -> f32 {
    let (w, h) = tile.dimensions();
    if w < 2 || h < 2 {
        return 1.0;
    }
    let diff = |a: &Rgba<u8>, b: &Rgba<u8>| -> f32 {
        (0..3)
            .map(|c| (a[c] as f32 - b[c] as f32).abs())
            .sum::<f32>()
            / 3.0
    };
    let mut seam = 0.0;
    for y in 0..h {
        seam += diff(tile.get_pixel(w - 1, y), tile.get_pixel(0, y));
    }
    for x in 0..w {
        seam += diff(tile.get_pixel(x, h - 1), tile.get_pixel(x, 0));
    }
    seam /= (w + h) as f32;
    let mut inner = 0.0;
    let mut count = 0usize;
    for y in 0..h {
        for x in 0..w {
            let p = tile.get_pixel(x, y);
            if x + 1 < w {
                inner += diff(p, tile.get_pixel(x + 1, y));
                count += 1;
            }
            if y + 1 < h {
                inner += diff(p, tile.get_pixel(x, y + 1));
                count += 1;
            }
        }
    }
    inner /= count as f32;
    if inner <= f32::EPSILON {
        return if seam <= f32::EPSILON {
            1.0
        } else {
            f32::INFINITY
        };
    }
    seam / inner
}

/// The tile repeated `columns` × `rows` times.
pub fn repeat(tile: &RgbaImage, columns: u32, rows: u32) -> RgbaImage {
    let (w, h) = tile.dimensions();
    RgbaImage::from_fn(w * columns.max(1), h * rows.max(1), |x, y| {
        *tile.get_pixel(x % w, y % h)
    })
}

/// Floating-point RGBA image, row-major.
struct Planes {
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

impl Planes {
    fn from(img: &RgbaImage) -> Self {
        Self {
            w: img.width() as usize,
            h: img.height() as usize,
            px: img
                .pixels()
                .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32])
                .collect(),
        }
    }

    fn to_image(&self) -> RgbaImage {
        RgbaImage::from_fn(self.w as u32, self.h as u32, |x, y| {
            let p = self.px[y as usize * self.w + x as usize];
            Rgba(p.map(|v| v.round().clamp(0.0, 255.0) as u8))
        })
    }

    fn transposed(&self) -> Self {
        let mut px = vec![[0.0; 4]; self.px.len()];
        for y in 0..self.h {
            for x in 0..self.w {
                px[x * self.h + y] = self.px[y * self.w + x];
            }
        }
        Self {
            w: self.h,
            h: self.w,
            px,
        }
    }
}

fn distance(a: [f32; 4], b: [f32; 4]) -> f32 {
    (0..4).map(|c| (a[c] - b[c]).abs()).sum()
}

/// Minimum-error cut through `cost` (`rows` × `band`, row-major): one
/// position per row, moving at most one column per row, the same position in
/// the first and last row (so the result stays seamless along the other axis).
/// Positions are limited to `lo..=hi`.
fn cyclic_cut(cost: &[f32], rows: usize, band: usize, lo: usize, hi: usize) -> Vec<usize> {
    // The shared first/last position: where both of those rows match best.
    let last = (rows - 1) * band;
    let start = (lo..=hi)
        .min_by(|&a, &b| (cost[a] + cost[last + a]).total_cmp(&(cost[b] + cost[last + b])))
        .unwrap_or(lo);
    let mut acc = vec![f32::INFINITY; band];
    acc[start] = cost[start];
    let mut from = vec![0usize; rows * band];
    for r in 1..rows {
        let mut next = vec![f32::INFINITY; band];
        for c in lo..=hi {
            let mut m = (acc[c], c);
            if c > lo && acc[c - 1] < m.0 {
                m = (acc[c - 1], c - 1);
            }
            if c < hi && acc[c + 1] < m.0 {
                m = (acc[c + 1], c + 1);
            }
            next[c] = m.0 + cost[r * band + c];
            from[r * band + c] = m.1;
        }
        acc = next;
    }
    // The last row comes back to the start.
    let mut path = vec![start; rows];
    for r in (1..rows).rev() {
        path[r - 1] = from[r * band + path[r]];
    }
    path
}

/// Makes the image seamless along x (rows are processed independently, so it
/// keeps any seamlessness along y only where the cut is the same at the
/// first and last row, which [`cyclic_cut`] ensures).
fn seamless_x(src: &Planes, blend: f32) -> Planes {
    let (w, h) = (src.w, src.h);
    if w < 2 {
        return Planes {
            w,
            h,
            px: src.px.clone(),
        };
    }
    if w < 6 {
        // Too narrow for patch bands: offsetting by half alone makes the
        // wrap edge two neighbouring columns.
        let px = (0..w * h)
            .map(|i| src.px[(i / w) * w + (i % w + w / 2) % w])
            .collect();
        return Planes { w, h, px };
    }
    // Shift so the new wrap edge is the smoothest column pair near the middle.
    let col_diff = |a: usize, b: usize| -> f32 {
        (0..h)
            .map(|y| distance(src.px[y * w + a], src.px[y * w + b]))
            .sum()
    };
    let shift = (w / 4).max(1)..=(3 * w / 4).min(w - 1);
    let s = shift
        .min_by(|&a, &b| col_diff(a - 1, a).total_cmp(&col_diff(b - 1, b)))
        .unwrap_or(w / 2);
    let shifted = |x: usize, y: usize| src.px[y * w + (x + s) % w];
    let band = ((blend.clamp(0.05, 0.5) * w as f32 / 2.0).round() as usize).clamp(1, w / 2 - 1);
    let feather = (band / 4).clamp(1, 3);
    let (lo, hi) = (feather, (band - 1).saturating_sub(feather));
    let mut out = src.px.clone();
    // Left band: shifted content up to the cut, original after it.
    // Right band (mirrored): original up to the cut, shifted after it.
    for right in [false, true] {
        let col = |i: usize| if right { w - 1 - i } else { i };
        let mut cost = vec![0.0; h * band];
        for y in 0..h {
            for i in 0..band {
                let x = col(i);
                cost[y * band + i] = distance(shifted(x, y), src.px[y * w + x]);
            }
        }
        let cut = if lo <= hi {
            cyclic_cut(&cost, h, band, lo, hi)
        } else {
            // A band of 1–2 columns: all of it shifted content.
            vec![band; h]
        };
        for y in 0..h {
            for i in 0..band {
                let x = col(i);
                let t = ((cut[y] as f32 + 0.5 - i as f32) / (2.0 * feather as f32) + 0.5)
                    .clamp(0.0, 1.0);
                let a = shifted(x, y);
                let b = src.px[y * w + x];
                out[y * w + x] = std::array::from_fn(|c| a[c] * t + b[c] * (1.0 - t));
            }
        }
    }
    Planes { w, h, px: out }
}

/// Makes a tile seamless: along each axis the content is offset so the wrap
/// edge falls on its smoothest line, and the original is patched back in
/// through a minimum-error cut near the edges, feathered over a few pixels.
/// `blend` (0.05–0.5) is the fraction of the tile used for the patch bands.
/// Same size; alpha is processed like color.
pub fn make_seamless(tile: &RgbaImage, blend: f32) -> RgbaImage {
    let planes = Planes::from(tile);
    let x = seamless_x(&planes, blend);
    seamless_x(&x.transposed(), blend).transposed().to_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bricks(w: u32, h: u32, ox: u32, oy: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            let (x, y) = (x + ox, y + oy);
            let row = (y / 10) % 2;
            let xx = (x + row * 12) % 24;
            if y % 10 < 2 || xx < 2 {
                Rgba([210, 205, 200, 255])
            } else {
                Rgba([140 + (xx * 3) as u8, 60 + (y % 10) as u8 * 4, 50, 255])
            }
        })
    }

    #[test]
    fn running_bond_period_is_two_courses() {
        for (ox, oy) in [(0, 0), (5, 3), (17, 9)] {
            let d = detect(&bricks(150, 90, ox, oy));
            assert!(d.found);
            assert_eq!(d.period, (24, 20), "offset {ox},{oy}");
            assert!(d.confidence > 0.9);
        }
    }

    #[test]
    fn noise_has_no_pattern() {
        let mut s = 7u32;
        let img = RgbaImage::from_fn(120, 100, |_, _| {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let v = (s >> 16) as u8;
            Rgba([v, v, v, 255])
        });
        let d = detect(&img);
        assert!(!d.found);
        assert_eq!(d.tile, PixelRect::new(0, 0, 120, 100));
    }

    #[test]
    fn make_seamless_keeps_size_and_closes_the_seam() {
        let img = RgbaImage::from_fn(64, 48, |x, y| Rgba([(x * 4) as u8, (y * 5) as u8, 90, 255]));
        assert!(seam_ratio(&img) > 10.0);
        let s = make_seamless(&img, DEFAULT_BLEND);
        assert_eq!(s.dimensions(), (64, 48));
        assert!(seam_ratio(&s) <= SEAMLESS_RATIO, "{}", seam_ratio(&s));
    }

    #[test]
    fn tiles_of_every_small_size_become_seamless() {
        for w in 2..16 {
            for h in [2, 5, 6, 7, 40] {
                let img = RgbaImage::from_fn(w, h, |x, y| {
                    Rgba([(x * 17) as u8, (y * 9) as u8, ((x * y) % 7 * 30) as u8, 255])
                });
                // Largest difference between horizontal neighbours.
                let bound = (0..h)
                    .flat_map(|y| (0..w - 1).map(move |x| (x, y)))
                    .flat_map(|(x, y)| {
                        let (p, q) = (img.get_pixel(x, y), img.get_pixel(x + 1, y));
                        (0..3).map(move |c| p[c].abs_diff(q[c]))
                    })
                    .max()
                    .unwrap_or(0);
                for blend in [0.05, 0.25, 0.5] {
                    let s = make_seamless(&img, blend);
                    assert_eq!(s.dimensions(), (w, h));
                    for y in 0..h {
                        let wrap = (0..3)
                            .map(|c| s.get_pixel(w - 1, y)[c].abs_diff(s.get_pixel(0, y)[c]))
                            .max()
                            .unwrap();
                        // The wrap pair is a pair of original neighbours.
                        assert!(wrap <= bound + 1, "{w}x{h} blend {blend}: {wrap} > {bound}");
                    }
                }
            }
        }
    }

    #[test]
    fn flat_and_partial_repeats_are_not_patterns() {
        let flat = RgbaImage::from_pixel(64, 64, Rgba([90, 90, 90, 255]));
        assert!(!detect(&flat).found);
        // 1.5 repeats of a 24 × 20 bond per axis.
        let d = detect(&bricks(36, 30, 3, 2));
        assert!(!d.found, "{d:?}");
        assert_eq!(d.tile, PixelRect::new(0, 0, 36, 30));
    }

    #[test]
    fn repeat_tiles_exactly() {
        let t = bricks(24, 20, 0, 0);
        let r = repeat(&t, 3, 2);
        assert_eq!(r.dimensions(), (72, 40));
        assert!(r
            .enumerate_pixels()
            .all(|(x, y, p)| p == t.get_pixel(x % 24, y % 20)));
    }
}
