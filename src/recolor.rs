//! Recolouring paint while keeping markings: a paint palette found by
//! clustering in CIELAB, membership of pixels in each paint, and a shading
//! transfer that moves a paint's base colour while keeping every pixel's
//! difference from it (rivets, panel lines, joints, weathering).

use image::{GrayImage, Rgba, RgbaImage};

/// Paint colours closer than this (ΔE) are one swatch.
pub const MERGE_DE: f32 = 12.0;
/// Default membership tolerance (ΔE).
pub const DEFAULT_TOLERANCE: f32 = 18.0;
/// Pixels darker than this (L*) are ink, not paint.
pub const INK_L: f32 = 18.0;
/// Pixels this close to the paper colour (ΔE) are paper, not paint.
pub const PAPER_DE: f32 = 6.0;
/// Clusters smaller than this share of the painted pixels are dropped.
pub const MIN_SHARE: f32 = 0.005;
/// A cluster below this share whose colour (a*, b*) is within `SHADE_AB` of
/// a bigger cluster is that paint's shading, not a paint.
const SHADE_SHARE: f32 = 0.05;
const SHADE_AB: f32 = 12.0;
const MAX_SAMPLES: usize = 200_000;
/// A pixel belongs to the paint around it only when that paint's members
/// fill at least this share of its surroundings.
const AROUND_SHARE: f32 = 0.35;
/// Membership marker for paper, ink and transparent pixels.
const NOT_PAINT: u8 = u8::MAX - 1;

/// Distance in the a*/b* plane, with chroma scaled to the lightness so a
/// darker shade of a paint (lower chroma) still matches.
fn chroma_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let scale = |c: [f32; 3]| (c[1] / c[0].max(1.0), c[2] / c[0].max(1.0));
    let (a1, b1) = scale(a);
    let (a2, b2) = scale(b);
    ((a1 - a2).powi(2) + (b1 - b2).powi(2)).sqrt() * 50.0
}
const K: usize = 8;

/// sRGB → CIELAB (D65).
pub fn lab(rgb: [u8; 3]) -> [f32; 3] {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(rgb[0]), lin(rgb[1]), lin(rgb[2]));
    let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
    let z = (0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83;
    let f = |t: f32| {
        if t > 0.008_856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIELAB (D65) → sRGB, clamped.
pub fn rgb(lab: [f32; 3]) -> [u8; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;
    let inv = |t: f32| {
        let t3 = t * t * t;
        if t3 > 0.008_856 {
            t3
        } else {
            (t - 16.0 / 116.0) / 7.787
        }
    };
    let (x, y, z) = (inv(fx) * 0.950_47, inv(fy), inv(fz) * 1.088_83);
    let r = 3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z;
    let g = -0.969_266 * x + 1.876_010_8 * y + 0.041_556 * z;
    let b = 0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z;
    let enc = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let v = if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [enc(r), enc(g), enc(b)]
}

/// Whether a CIELAB colour is inside the sRGB gamut.
fn in_gamut(lab: [f32; 3]) -> bool {
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;
    let inv = |t: f32| {
        let t3 = t * t * t;
        if t3 > 0.008_856 {
            t3
        } else {
            (t - 16.0 / 116.0) / 7.787
        }
    };
    let (x, y, z) = (inv(fx) * 0.950_47, inv(fy), inv(fz) * 1.088_83);
    let r = 3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z;
    let g = -0.969_266 * x + 1.876_010_8 * y + 0.041_556 * z;
    let b = 0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z;
    let ok = |c: f32| (-0.001..=1.001).contains(&c);
    ok(r) && ok(g) && ok(b)
}

/// CIELAB → sRGB keeping the lightness: out-of-gamut colours lose chroma
/// (not lightness), so shading contrast survives.
pub fn rgb_keep_lightness(lab: [f32; 3]) -> [u8; 3] {
    let l = lab[0].clamp(0.0, 100.0);
    if in_gamut([l, lab[1], lab[2]]) {
        return rgb([l, lab[1], lab[2]]);
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..12 {
        let mid = (lo + hi) / 2.0;
        if in_gamut([l, lab[1] * mid, lab[2] * mid]) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    rgb([l, lab[1] * lo, lab[2] * lo])
}

/// ΔE*ab.
pub fn delta_e(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// One paint colour of the model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swatch {
    pub lab: [f32; 3],
    pub rgb: [u8; 3],
    /// Share of the painted pixels (0–1).
    pub share: f32,
}

/// The paper colour: the most common light (L* > 80) opaque colour, if
/// light pixels make up at least 1 % of the opaque ones.
pub fn paper_of(img: &RgbaImage) -> Option<[u8; 3]> {
    let mut counts = std::collections::HashMap::<[u8; 3], u32>::new();
    let (mut opaque, mut light) = (0u32, 0u32);
    let (w, h) = img.dimensions();
    for y in (0..h).step_by(3) {
        for x in (0..w).step_by(3) {
            let p = img.get_pixel(x, y);
            if p[3] < 128 {
                continue;
            }
            opaque += 1;
            if lab([p[0], p[1], p[2]])[0] > 80.0 {
                light += 1;
                *counts
                    .entry([p[0] & 0xF8, p[1] & 0xF8, p[2] & 0xF8])
                    .or_default() += 1;
            }
        }
    }
    if opaque == 0 || light * 100 < opaque {
        return None;
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(c, _)| [c[0] | 4, c[1] | 4, c[2] | 4])
}

/// Whether a pixel is paint (not paper, not ink, not transparent).
fn is_paint(p: &Rgba<u8>, l: [f32; 3], paper: Option<[f32; 3]>) -> bool {
    p[3] >= 128 && l[0] >= INK_L && paper.is_none_or(|paper| delta_e(l, paper) >= PAPER_DE)
}

/// The paint colours of `images` (most common first): k-means in CIELAB
/// on a sample, merged within `MERGE_DE`, small clusters dropped. `paper`
/// is the paper colour (excluded), if any.
pub fn palette(images: &[&RgbaImage], paper: Option<[u8; 3]>) -> Vec<Swatch> {
    let paper = paper.map(lab);
    let total: usize = images
        .iter()
        .map(|i| i.width() as usize * i.height() as usize)
        .sum();
    let stride = (total / (MAX_SAMPLES * 4)).max(1);
    let mut samples: Vec<[f32; 3]> = Vec::new();
    let mut k = 0usize;
    for img in images {
        for p in img.pixels() {
            k += 1;
            if !k.is_multiple_of(stride) {
                continue;
            }
            let l = lab([p[0], p[1], p[2]]);
            if is_paint(p, l, paper) {
                samples.push(l);
            }
        }
    }
    if samples.is_empty() {
        return Vec::new();
    }
    if samples.len() > MAX_SAMPLES {
        let step = samples.len() as f32 / MAX_SAMPLES as f32;
        samples = (0..MAX_SAMPLES)
            .map(|i| samples[(i as f32 * step) as usize])
            .collect();
    }
    // k-means++ seeding (deterministic: farthest-point after the mean).
    let mean = {
        let n = samples.len() as f32;
        let s = samples
            .iter()
            .fold([0.0f32; 3], |a, c| [a[0] + c[0], a[1] + c[1], a[2] + c[2]]);
        [s[0] / n, s[1] / n, s[2] / n]
    };
    let mut centers = vec![*samples
        .iter()
        .min_by(|a, b| delta_e(**a, mean).total_cmp(&delta_e(**b, mean)))
        .expect("samples")];
    while centers.len() < K {
        let far = samples
            .iter()
            .max_by(|a, b| {
                let da = centers
                    .iter()
                    .map(|c| delta_e(**a, *c))
                    .fold(f32::MAX, f32::min);
                let db = centers
                    .iter()
                    .map(|c| delta_e(**b, *c))
                    .fold(f32::MAX, f32::min);
                da.total_cmp(&db)
            })
            .copied()
            .expect("samples");
        if centers.iter().any(|c| delta_e(*c, far) < 1.0) {
            break;
        }
        centers.push(far);
    }
    let mut counts = vec![0usize; centers.len()];
    for _ in 0..15 {
        let mut sums = vec![[0.0f64; 3]; centers.len()];
        counts.iter_mut().for_each(|c| *c = 0);
        for s in &samples {
            let j = nearest(&centers, *s).0;
            counts[j] += 1;
            for c in 0..3 {
                sums[j][c] += s[c] as f64;
            }
        }
        for (j, c) in centers.iter_mut().enumerate() {
            if counts[j] > 0 {
                let n = counts[j] as f64;
                *c = [
                    (sums[j][0] / n) as f32,
                    (sums[j][1] / n) as f32,
                    (sums[j][2] / n) as f32,
                ];
            }
        }
    }
    // Merge near clusters (weighted), drop small ones.
    let mut clusters: Vec<([f32; 3], usize)> = centers
        .into_iter()
        .zip(counts)
        .filter(|(_, n)| *n > 0)
        .collect();
    clusters.sort_by_key(|c| std::cmp::Reverse(c.1));
    let mut merged: Vec<([f32; 3], usize)> = Vec::new();
    for (c, n) in clusters {
        if let Some(m) = merged.iter_mut().find(|m| delta_e(m.0, c) < MERGE_DE) {
            let t = (m.1 + n) as f32;
            m.0 = [
                (m.0[0] * m.1 as f32 + c[0] * n as f32) / t,
                (m.0[1] * m.1 as f32 + c[1] * n as f32) / t,
                (m.0[2] * m.1 as f32 + c[2] * n as f32) / t,
            ];
            m.1 += n;
        } else {
            merged.push((c, n));
        }
    }
    // Small clusters that are only a lighter/darker shade of a bigger paint
    // (rivets, panel lines, weathering) belong to that paint.
    let total_n = samples.len() as f32;
    let mut i = merged.len();
    while i > 0 {
        i -= 1;
        let (c, n) = merged[i];
        if n as f32 / total_n >= SHADE_SHARE {
            continue;
        }
        let host = merged
            .iter()
            .enumerate()
            .find(|(j, (h, hn))| *j != i && *hn > n && chroma_distance(*h, c) < SHADE_AB);
        if let Some((j, _)) = host {
            merged[j].1 += n;
            merged.remove(i);
        }
    }
    let total = samples.len() as f32;
    let mut out: Vec<Swatch> = merged
        .into_iter()
        .map(|(c, n)| Swatch {
            lab: c,
            rgb: rgb(c),
            share: n as f32 / total,
        })
        .filter(|s| s.share >= MIN_SHARE)
        .collect();
    out.sort_by(|a, b| b.share.total_cmp(&a.share));
    out
}

fn nearest(centers: &[[f32; 3]], p: [f32; 3]) -> (usize, f32) {
    centers
        .iter()
        .enumerate()
        .map(|(i, c)| (i, delta_e(*c, p)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0, f32::MAX))
}

/// A recolour: the palette, the paper colour (never recoloured), a new
/// colour per swatch (`None` = keep), membership tolerance (ΔE), and detail
/// strength (0–1.5).
#[derive(Debug, Clone, PartialEq)]
pub struct Recolor {
    pub swatches: Vec<Swatch>,
    pub paper: Option<[u8; 3]>,
    pub targets: Vec<Option<[u8; 3]>>,
    pub tolerance: f32,
    pub detail: f32,
}

impl Recolor {
    pub fn new(swatches: Vec<Swatch>, paper: Option<[u8; 3]>) -> Self {
        Self {
            targets: vec![None; swatches.len()],
            swatches,
            paper,
            tolerance: DEFAULT_TOLERANCE,
            detail: 1.0,
        }
    }

    /// Whether anything would change.
    pub fn is_identity(&self) -> bool {
        self.targets.iter().all(Option::is_none)
    }
}

/// The swatch each pixel belongs to (`u8::MAX`: none). Paper, ink and
/// transparent pixels never belong. Pixels within the tolerance of a
/// swatch are its own members; every other pixel — a rivet or panel line
/// inside a painted area, too — belongs to the swatch whose own members
/// fill most of its surroundings (box of radius `r` px), when they fill at
/// least `AROUND_SHARE` of it. So outlines, blends of ink and paper, and a
/// darker shade of one paint inside another stay with their surroundings.
pub fn membership(
    img: &RgbaImage,
    swatches: &[Swatch],
    paper: Option<[u8; 3]>,
    tolerance: f32,
    r: usize,
) -> Vec<u8> {
    let centers: Vec<[f32; 3]> = swatches.iter().map(|s| s.lab).collect();
    let (w, h) = (img.width() as usize, img.height() as usize);
    if centers.is_empty() {
        return vec![u8::MAX; w * h];
    }
    let paper = paper.map(lab);
    let raw = img.as_raw();
    // Each pixel's own swatch.
    let mut own = vec![u8::MAX; w * h];
    par_chunks(&mut own, |start, chunk| {
        let mut cache = LabCache::new();
        for (k, m) in chunk.iter_mut().enumerate() {
            let i = start + k;
            let p = Rgba([raw[i * 4], raw[i * 4 + 1], raw[i * 4 + 2], raw[i * 4 + 3]]);
            let l = cache.lab([p[0], p[1], p[2]]);
            if !is_paint(&p, l, paper) {
                *m = NOT_PAINT;
                continue;
            }
            let (s, d) = nearest(&centers, l);
            if d <= tolerance {
                *m = s as u8;
            }
        }
    });
    // Then each paint pixel joins the paint whose members fill most of its
    // surroundings, if they fill enough of them.
    let k = centers.len();
    let mut out = own.clone();
    let mut rows: Vec<&mut [u8]> = out.chunks_mut(w).collect();
    par_chunks(&mut rows, |y0, chunk| {
        // Per column: member counts per swatch in rows y-r..=y+r.
        let mut cols = vec![[0u32; K]; w];
        let add_row = |cols: &mut [[u32; K]], y: usize, remove: bool| {
            for (x, col) in cols.iter_mut().enumerate() {
                let s = own[y * w + x] as usize;
                if s < k {
                    col[s] = if remove { col[s] - 1 } else { col[s] + 1 };
                }
            }
        };
        // Rows before y0's window, so the first step below can drop row y0-r-1.
        for y in y0.saturating_sub(r + 1)..(y0 + r).min(h) {
            add_row(&mut cols, y, false);
        }
        for (dy, row) in chunk.iter_mut().enumerate() {
            let y = y0 + dy;
            if y + r < h {
                add_row(&mut cols, y + r, false);
            }
            if y > r {
                add_row(&mut cols, y - r - 1, true);
            }
            let rows_in = (y + r).min(h - 1) + 1 - y.saturating_sub(r);
            let mut sum = [0u32; K];
            for col in cols.iter().take(r.min(w)) {
                for s in 0..k {
                    sum[s] += col[s];
                }
            }
            for x in 0..w {
                if x + r < w {
                    for s in 0..k {
                        sum[s] += cols[x + r][s];
                    }
                }
                if x > r {
                    for s in 0..k {
                        sum[s] -= cols[x - r - 1][s];
                    }
                }
                if row[x] == NOT_PAINT {
                    continue;
                }
                let cols_in = (x + r).min(w - 1) + 1 - x.saturating_sub(r);
                let (best, n) = (0..k).map(|s| (s, sum[s])).max_by_key(|&(_, n)| n).unwrap();
                row[x] = if n as f32 >= AROUND_SHARE * (rows_in * cols_in) as f32 {
                    best as u8
                } else {
                    u8::MAX
                };
            }
        }
    });
    for m in &mut out {
        if *m == NOT_PAINT {
            *m = u8::MAX;
        }
    }
    out
}

/// Runs `f(start, chunk)` over `data` split into one chunk per core.
pub(crate) fn par_chunks<T: Send>(data: &mut [T], f: impl Fn(usize, &mut [T]) + Sync) {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let size = data.len().div_ceil(threads).max(4096);
    std::thread::scope(|scope| {
        for (k, chunk) in data.chunks_mut(size).enumerate() {
            let f = &f;
            scope.spawn(move || f(k * size, chunk));
        }
    });
}

/// A small direct-mapped cache of sRGB → CIELAB conversions (paint areas
/// repeat few colours).
pub(crate) struct LabCache {
    keys: Vec<u32>,
    values: Vec<[f32; 3]>,
}

impl LabCache {
    const SIZE: usize = 1 << 14;

    pub(crate) fn new() -> Self {
        Self {
            keys: vec![u32::MAX; Self::SIZE],
            values: vec![[0.0; 3]; Self::SIZE],
        }
    }

    pub(crate) fn lab(&mut self, c: [u8; 3]) -> [f32; 3] {
        let key = (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
        let slot = (key.wrapping_mul(2_654_435_761) >> 18) as usize & (Self::SIZE - 1);
        if self.keys[slot] != key {
            self.keys[slot] = key;
            self.values[slot] = lab(c);
        }
        self.values[slot]
    }
}

/// One pixel's shading transfer: its lightness difference from the paint's
/// base is added to the new colour, and its chroma relative to the base
/// scales the new colour's chroma (a darker, duller shade stays a darker,
/// duller shade of the new paint); both × `detail`.
pub fn transfer(base: [f32; 3], own: [f32; 3], new: [f32; 3], detail: f32) -> [f32; 3] {
    let chroma = |c: [f32; 3]| (c[1] * c[1] + c[2] * c[2]).sqrt();
    let ratio = if chroma(base) > 2.0 {
        (chroma(own) / chroma(base)).clamp(0.0, 3.0)
    } else {
        1.0
    };
    let k = 1.0 + (ratio - 1.0) * detail;
    // Hue shifts of the shading (e.g. reddish rust on grey) carry over too.
    let hue = |c: [f32; 3], r: f32| [c[1] - base[1] * r, c[2] - base[2] * r];
    let shift = hue(own, ratio);
    [
        new[0] + (own[0] - base[0]) * detail,
        new[1] * k + shift[0] * detail,
        new[2] * k + shift[1] * detail,
    ]
}

/// What a recolour of one image needs that does not depend on the new
/// colours or the detail strength: each pixel's CIELAB colour and swatch.
/// Reusing it makes previews of colour and detail changes cheap.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    lab: Vec<[f32; 3]>,
    member: Vec<u8>,
    /// The tolerance it was made with.
    pub tolerance: f32,
}

/// Prepares `img` for [`apply_prepared`] (see [`membership`]).
pub fn prepare(
    img: &RgbaImage,
    swatches: &[Swatch],
    paper: Option<[u8; 3]>,
    tolerance: f32,
    r: usize,
) -> Prepared {
    let member = membership(img, swatches, paper, tolerance, r);
    let raw = img.as_raw();
    let mut labs = vec![[0.0f32; 3]; member.len()];
    par_chunks(&mut labs, |start, chunk| {
        let mut cache = LabCache::new();
        for (k, l) in chunk.iter_mut().enumerate() {
            let i = (start + k) * 4;
            *l = cache.lab([raw[i], raw[i + 1], raw[i + 2]]);
        }
    });
    Prepared {
        lab: labs,
        member,
        tolerance,
    }
}

/// [`apply`] with a [`Prepared`] image (same swatches and tolerance).
pub fn apply_prepared(
    img: &RgbaImage,
    prep: &Prepared,
    rc: &Recolor,
    protect: Option<&GrayImage>,
) -> RgbaImage {
    if rc.is_identity() {
        return img.clone();
    }
    let targets: Vec<Option<[f32; 3]>> = rc.targets.iter().map(|t| t.map(lab)).collect();
    let w = img.width() as usize;
    let mut out = img.clone();
    let pixels: &mut [u8] = &mut out;
    let mut rows: Vec<&mut [u8]> = pixels.chunks_mut(w * 4).collect();
    par_chunks(&mut rows, |first_row, chunk| {
        for (k, row) in chunk.iter_mut().enumerate() {
            let y = first_row + k;
            for x in 0..w {
                let i = y * w + x;
                let s = prep.member[i];
                if s == u8::MAX {
                    continue;
                }
                let Some(Some(new)) = targets.get(s as usize) else {
                    continue;
                };
                if protect.is_some_and(|m| m.get_pixel(x as u32, y as u32)[0] > 0) {
                    continue;
                }
                let base = rc.swatches[s as usize].lab;
                let c = rgb_keep_lightness(transfer(base, prep.lab[i], *new, rc.detail));
                row[x * 4..x * 4 + 3].copy_from_slice(&c);
            }
        }
    });
    out
}

/// Applies a recolour: member pixels of mapped swatches get the new colour
/// plus their own difference from the swatch's base colour × detail (in
/// CIELAB); everything else — and every pixel set in `protect` — is
/// unchanged. `r` is the membership blur radius in pixels.
pub fn apply(img: &RgbaImage, rc: &Recolor, r: usize, protect: Option<&GrayImage>) -> RgbaImage {
    if rc.is_identity() {
        return img.clone();
    }
    let member = membership(img, &rc.swatches, rc.paper, rc.tolerance, r);
    let targets: Vec<Option<[f32; 3]>> = rc.targets.iter().map(|t| t.map(lab)).collect();
    let w = img.width() as usize;
    let mut out = img.clone();
    let pixels: &mut [u8] = &mut out;
    // Rows of 4-byte pixels, split across cores.
    let mut rows: Vec<&mut [u8]> = pixels.chunks_mut(w * 4).collect();
    par_chunks(&mut rows, |first_row, chunk| {
        let mut cache = LabCache::new();
        for (k, row) in chunk.iter_mut().enumerate() {
            let y = first_row + k;
            for x in 0..w {
                let s = member[y * w + x];
                if s == u8::MAX {
                    continue;
                }
                let Some(Some(new)) = targets.get(s as usize) else {
                    continue;
                };
                if protect.is_some_and(|m| m.get_pixel(x as u32, y as u32)[0] > 0) {
                    continue;
                }
                let p = &mut row[x * 4..x * 4 + 4];
                let base = rc.swatches[s as usize].lab;
                let own = cache.lab([p[0], p[1], p[2]]);
                let c = rgb_keep_lightness(transfer(base, own, *new, rc.detail));
                p[..3].copy_from_slice(&c);
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lab_round_trips() {
        for c in [
            [0, 0, 0],
            [255, 255, 255],
            [107, 115, 71],
            [204, 184, 133],
            [12, 200, 99],
        ] {
            let back = rgb(lab(c));
            for k in 0..3 {
                assert!(
                    (back[k] as i32 - c[k] as i32).abs() <= 1,
                    "{c:?} → {back:?}"
                );
            }
        }
        assert!((lab([255, 255, 255])[0] - 100.0).abs() < 0.1);
    }

    /// Two painted panels with dark rivets on white paper.
    fn sheet() -> RgbaImage {
        let olive = Rgba([107, 115, 71, 255]);
        let sand = Rgba([204, 184, 133, 255]);
        RgbaImage::from_fn(200, 100, |x, y| {
            let paint = if x < 100 { olive } else { sand };
            if !(10..90).contains(&y) || x % 100 < 10 || x % 100 >= 90 {
                Rgba([255, 255, 255, 255])
            } else if x % 10 == 5 && y % 10 == 5 {
                let f = |c: u8| (c as f32 * 0.6) as u8;
                Rgba([f(paint[0]), f(paint[1]), f(paint[2]), 255])
            } else {
                paint
            }
        })
    }

    #[test]
    fn prepared_matches_direct() {
        let img = sheet();
        let pal = palette(&[&img], paper_of(&img));
        let mut rc = Recolor::new(pal, paper_of(&img));
        rc.targets[0] = Some([200, 30, 30]);
        rc.detail = 0.7;
        let prep = prepare(&img, &rc.swatches, rc.paper, rc.tolerance, 2);
        assert_eq!(
            apply_prepared(&img, &prep, &rc, None),
            apply(&img, &rc, 2, None)
        );
    }

    /// Membership is worked out in row bands across cores; a tall striped
    /// sheet (several bands) must give every period the same rows.
    #[test]
    fn membership_is_the_same_in_every_band() {
        let olive = Rgba([107, 115, 71, 255]);
        let img = RgbaImage::from_fn(30, 9000, |x, y| {
            if x < 5 {
                Rgba([255, 255, 255, 255])
            } else if x % 10 == 7 && y % 10 == 3 {
                Rgba([60, 64, 40, 255])
            } else {
                olive
            }
        });
        let pal = palette(&[&img], paper_of(&img));
        let m = membership(&img, &pal, paper_of(&img), DEFAULT_TOLERANCE, 3);
        let row = |y: usize| &m[y * 30..(y + 1) * 30];
        for y in 10..8990 {
            assert_eq!(row(y), row(10 + y % 10), "row {y}");
        }
        assert!(
            row(13)[7..].iter().all(|&s| s == 0),
            "rivets join the paint"
        );
        assert!(
            row(13)[..5].iter().all(|&s| s == u8::MAX),
            "paper never does"
        );
    }

    #[test]
    fn the_palette_is_the_paints() {
        let pal = palette(&[&sheet()], paper_of(&sheet()));
        assert_eq!(pal.len(), 2, "{pal:?}");
        assert!(
            delta_e(pal[0].lab, lab([107, 115, 71])) < 3.0
                || delta_e(pal[1].lab, lab([107, 115, 71])) < 3.0
        );
    }

    #[test]
    fn recolour_keeps_rivets_and_other_paint() {
        let img = sheet();
        let pal = palette(&[&img], paper_of(&img));
        let olive = pal
            .iter()
            .position(|s| delta_e(s.lab, lab([107, 115, 71])) < 3.0)
            .unwrap();
        let mut rc = Recolor::new(pal, paper_of(&img));
        rc.targets[olive] = Some([40, 70, 160]);
        let out = apply(&img, &rc, 2, None);
        // Plain paint becomes the new colour.
        assert!(
            delta_e(
                lab([
                    out.get_pixel(30, 30)[0],
                    out.get_pixel(30, 30)[1],
                    out.get_pixel(30, 30)[2]
                ]),
                lab([40, 70, 160])
            ) < 3.0
        );
        // Rivets stay darker than the paint around them.
        let rivet = lab([
            out.get_pixel(25, 25)[0],
            out.get_pixel(25, 25)[1],
            out.get_pixel(25, 25)[2],
        ]);
        let around = lab([
            out.get_pixel(27, 27)[0],
            out.get_pixel(27, 27)[1],
            out.get_pixel(27, 27)[2],
        ]);
        let orig = lab([
            img.get_pixel(25, 25)[0],
            img.get_pixel(25, 25)[1],
            img.get_pixel(25, 25)[2],
        ]);
        let orig_around = lab([107, 115, 71]);
        let (now, was) = (around[0] - rivet[0], orig_around[0] - orig[0]);
        eprintln!("rivet contrast now {now} was {was}");
        assert!((now - was).abs() <= 0.1 * was.abs(), "{now} vs {was}");
        // Sand and paper untouched.
        assert_eq!(out.get_pixel(150, 50), img.get_pixel(150, 50));
        assert_eq!(out.get_pixel(5, 5), img.get_pixel(5, 5));
        // Detail 0 removes the rivets.
        rc.detail = 0.0;
        let flat = apply(&img, &rc, 2, None);
        assert_eq!(flat.get_pixel(25, 25), flat.get_pixel(27, 27));
        // Protected pixels never change.
        rc.detail = 1.0;
        let mut protect = GrayImage::new(200, 100);
        for y in 20..40 {
            for x in 20..40 {
                protect.put_pixel(x, y, image::Luma([255]));
            }
        }
        let kept = apply(&img, &rc, 2, Some(&protect));
        assert_eq!(kept.get_pixel(30, 30), img.get_pixel(30, 30));
        assert_ne!(kept.get_pixel(50, 50), img.get_pixel(50, 50));
    }
}
