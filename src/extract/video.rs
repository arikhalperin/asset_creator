//! Whole-video extraction: per-frame keying and detection, perceptual-hash
//! dedup, tracking into objects, and grouping poses into animations.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::JoinHandle;

use image::RgbaImage;

use super::detect::{self, DetectParams};
use super::keying::{self, KeyParams};
use crate::document::PixelRect;
use crate::video::{VideoError, VideoSource};

// ---------------------------------------------------------------------------
// Perceptual hash and the same-appearance rule

/// A 64-bit DCT perceptual hash and which of its bits are reliable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PHash {
    pub bits: u64,
    /// Bits whose coefficient is clearly away from the median (on flat or
    /// symmetric shapes many coefficients sit at the median, where
    /// compression noise would flip them).
    pub confident: u64,
}

impl PHash {
    /// Differing bits among those reliable in both hashes.
    pub fn distance(&self, other: &PHash) -> u32 {
        ((self.bits ^ other.bits) & self.confident & other.confident).count_ones()
    }
}

/// Perceptual hash: the crop over neutral gray, grayscale, 32×32 bilinear,
/// 2-D DCT, the 8×8 lowest frequencies against their median; a bit is
/// reliable when its coefficient differs from the median by more than a
/// quarter of the mean absolute deviation.
pub fn phash(image: &RgbaImage) -> PHash {
    let gray = image::GrayImage::from_fn(image.width(), image.height(), |x, y| {
        let p = image.get_pixel(x, y);
        let a = p[3] as f32 / 255.0;
        let l = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
        image::Luma([(l * a + 128.0 * (1.0 - a)).round() as u8])
    });
    let small = image::imageops::resize(&gray, 32, 32, image::imageops::FilterType::Triangle);
    let v: Vec<f32> = small.pixels().map(|p| p[0] as f32).collect();
    // Separable DCT-II, keeping the first 8 rows/columns.
    let cos: Vec<f32> = (0..8 * 32)
        .map(|i| {
            let (k, n) = (i / 32, i % 32);
            (std::f32::consts::PI / 32.0 * (n as f32 + 0.5) * k as f32).cos()
        })
        .collect();
    let mut rows = [[0f32; 8]; 32]; // per row y, DCT over x
    for (y, row) in rows.iter_mut().enumerate() {
        for (k, out) in row.iter_mut().enumerate() {
            *out = (0..32).map(|x| v[y * 32 + x] * cos[k * 32 + x]).sum();
        }
    }
    let mut coeffs = [0f32; 64];
    for ky in 0..8 {
        for kx in 0..8 {
            coeffs[ky * 8 + kx] = (0..32).map(|y| rows[y][kx] * cos[ky * 32 + y]).sum();
        }
    }
    let mut sorted = coeffs;
    sorted.sort_by(f32::total_cmp);
    let median = (sorted[31] + sorted[32]) / 2.0;
    let spread = coeffs.iter().map(|c| (c - median).abs()).sum::<f32>() / 64.0;
    let (mut bits, mut confident) = (0u64, 0u64);
    for (i, c) in coeffs.iter().enumerate() {
        if *c > median {
            bits |= 1 << i;
        }
        if (c - median).abs() > 0.25 * spread {
            confident |= 1 << i;
        }
    }
    PHash { bits, confident }
}

/// Hash distance (reliable bits) up to which crops may be the same
/// appearance. A fast prefilter: shape and colour decide (compression moves
/// flat or symmetric shapes' hashes by up to ~12 bits).
pub const HASH_BITS: u32 = 16;
/// Smallest width/height difference always tolerated between cores (px).
pub const SIZE_SLACK: u32 = 3;
/// Width/height difference tolerated as a fraction of the larger core.
pub const SIZE_SLACK_FRACTION: f32 = 0.12;
/// Largest shape mismatch (see [`compare`]).
pub const SHAPE_MISMATCH: f32 = 0.04;
/// Largest interior colour difference (fraction of 255), measured on
/// colours averaged over 3×3 neighbourhoods so fine textures, which 4:2:0
/// compression smears, compare by their local average.
pub const COLOR_DIFF: f32 = 0.12;
/// Pieces of a crop smaller than this fraction of its largest piece are
/// left out of comparisons (compression fringe attached by Merge Distance).
pub const CORE_FRACTION: f32 = 0.1;
/// Alignment search radius around the core centres (px).
const ALIGN: i64 = 2;

/// One object crop in one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Crop {
    pub frame: u64,
    pub rect: PixelRect,
    pub image: RgbaImage,
    pub hash: PHash,
    /// The core: opaque pixels (alpha ≥ 128) of the largest 8-connected piece
    /// and of pieces at least `CORE_FRACTION` of its size; row-major.
    core: Vec<bool>,
    /// The core dilated by 1 px.
    near: Vec<bool>,
    /// Core bounding box (x0, y0, x1, y1), exclusive ends.
    core_box: (i64, i64, i64, i64),
    /// RGB averaged over each pixel's 3×3 neighbourhood of core pixels.
    smooth: Vec<[f32; 3]>,
}

impl Crop {
    pub fn new(frame: u64, rect: PixelRect, image: RgbaImage) -> Self {
        let hash = phash(&image);
        let (w, h) = image.dimensions();
        let pieces = detect::label(w, h, |x, y| image.get_pixel(x, y)[3] >= 128, true);
        let largest = pieces.sizes.iter().skip(1).copied().max().unwrap_or(0);
        let keep_from = ((largest as f32 * CORE_FRACTION).ceil() as u32).max(1);
        let core: Vec<bool> = pieces
            .label
            .iter()
            .map(|&l| l != 0 && pieces.sizes[l as usize] >= keep_from)
            .collect();
        let (wi, hi) = (w as i64, h as i64);
        let mut near = vec![false; core.len()];
        let mut core_box = (wi, hi, 0, 0);
        for y in 0..hi {
            for x in 0..wi {
                if !core[(y * wi + x) as usize] {
                    continue;
                }
                core_box = (
                    core_box.0.min(x),
                    core_box.1.min(y),
                    core_box.2.max(x + 1),
                    core_box.3.max(y + 1),
                );
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx >= 0 && ny >= 0 && nx < wi && ny < hi {
                            near[(ny * wi + nx) as usize] = true;
                        }
                    }
                }
            }
        }
        if core_box.0 >= core_box.2 {
            core_box = (0, 0, wi, hi);
        }
        let mut smooth = vec![[0f32; 3]; core.len()];
        for y in 0..hi {
            for x in 0..wi {
                let (mut sum, mut n) = ([0f32; 3], 0f32);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx >= 0 && ny >= 0 && nx < wi && ny < hi && core[(ny * wi + nx) as usize]
                        {
                            let p = image.get_pixel(nx as u32, ny as u32);
                            for c in 0..3 {
                                sum[c] += p[c] as f32;
                            }
                            n += 1.0;
                        }
                    }
                }
                if n > 0.0 {
                    smooth[(y * wi + x) as usize] = sum.map(|v| v / n);
                }
            }
        }
        Self {
            frame,
            rect,
            image,
            hash,
            core,
            near,
            core_box,
            smooth,
        }
    }

    fn at(&self, mask: &[bool], x: i64, y: i64) -> bool {
        let (w, h) = (self.image.width() as i64, self.image.height() as i64);
        x >= 0 && y >= 0 && x < w && y < h && mask[(y * w + x) as usize]
    }

    fn core_size(&self) -> (u32, u32) {
        (
            (self.core_box.2 - self.core_box.0) as u32,
            (self.core_box.3 - self.core_box.1) as u32,
        )
    }

    /// Core pixel whose four neighbours are core too (away from edges, where
    /// chroma subsampling bleeds).
    fn interior(&self, x: i64, y: i64) -> bool {
        self.at(&self.core, x, y)
            && self.at(&self.core, x - 1, y)
            && self.at(&self.core, x + 1, y)
            && self.at(&self.core, x, y - 1)
            && self.at(&self.core, x, y + 1)
    }
}

/// Shape mismatch and colour difference of two crops' cores, at the
/// alignment (core centres matched, then searched ±2 px) with the least
/// shape mismatch.
///
/// Shape mismatch: the fraction of core pixels of either crop with no core
/// pixel within 1 px in the other — edges that wobble by a pixel do not
/// count, a limb that moved does. Colour difference: the mean absolute
/// difference (0…1) of 3×3-averaged RGB over pixels interior to both cores
/// (0 when none).
pub fn compare(a: &Crop, b: &Crop) -> (f32, f32) {
    let centre = |c: &Crop| {
        (
            (c.core_box.0 + c.core_box.2) as f32 / 2.0,
            (c.core_box.1 + c.core_box.3) as f32 / 2.0,
        )
    };
    let (ca, cb) = (centre(a), centre(b));
    let (bx, by) = ((ca.0 - cb.0).round() as i64, (ca.1 - cb.1).round() as i64);
    let (aw, ah) = (a.image.width() as i64, a.image.height() as i64);
    let (bw, bh) = (b.image.width() as i64, b.image.height() as i64);
    let mut best = (f32::INFINITY, (bx, by));
    for dy in by - ALIGN..=by + ALIGN {
        for dx in bx - ALIGN..=bx + ALIGN {
            // Point (x, y) of a corresponds to (x - dx, y - dy) of b.
            let (mut bad, mut total) = (0u32, 0u32);
            for y in 0..ah {
                for x in 0..aw {
                    if a.core[(y * aw + x) as usize] {
                        total += 1;
                        bad += !b.at(&b.near, x - dx, y - dy) as u32;
                    }
                }
            }
            for y in 0..bh {
                for x in 0..bw {
                    if b.core[(y * bw + x) as usize] {
                        total += 1;
                        bad += !a.at(&a.near, x + dx, y + dy) as u32;
                    }
                }
            }
            let shape = bad as f32 / total.max(1) as f32;
            if shape < best.0 {
                best = (shape, (dx, dy));
            }
        }
    }
    // Colour at the shape's alignment, refined by ±1 px so fine textures
    // are compared in phase; the largest of the four quadrants' differences
    // (so a partly recoloured copy is not averaged away).
    let (sx, sy) = best.1;
    let mid = (
        (a.core_box.0 + a.core_box.2) / 2,
        (a.core_box.1 + a.core_box.3) / 2,
    );
    let mut colour = f32::INFINITY;
    for dy in sy - 1..=sy + 1 {
        for dx in sx - 1..=sx + 1 {
            let mut cells = [(0f64, 0u32); 4];
            for y in 0..ah {
                for x in 0..aw {
                    if a.interior(x, y) && b.interior(x - dx, y - dy) {
                        let p = a.smooth[(y * aw + x) as usize];
                        let q = b.smooth[((y - dy) * bw + (x - dx)) as usize];
                        let d = (0..3)
                            .map(|c| (p[c] as f64 - q[c] as f64).abs())
                            .sum::<f64>()
                            / 3.0;
                        let cell = (x >= mid.0) as usize + 2 * (y >= mid.1) as usize;
                        cells[cell].0 += d;
                        cells[cell].1 += 1;
                    }
                }
            }
            let total: u32 = cells.iter().map(|c| c.1).sum();
            if total == 0 {
                continue;
            }
            // Quadrants with too few interior pixels are folded into the whole.
            let enough = (total / 10).max(8);
            let whole = cells.iter().map(|c| c.0).sum::<f64>() / total as f64;
            let worst = cells
                .iter()
                .filter(|c| c.1 >= enough)
                .map(|c| c.0 / c.1 as f64)
                .fold(whole, f64::max);
            colour = colour.min((worst / 255.0) as f32);
        }
    }
    (best.0, if colour.is_finite() { colour } else { 0.0 })
}

/// Shape mismatch of two crops (see [`compare`]).
pub fn shape_mismatch(a: &Crop, b: &Crop) -> f32 {
    compare(a, b).0
}

/// Interior colour difference of two crops (see [`compare`]).
pub fn color_difference(a: &Crop, b: &Crop) -> f32 {
    compare(a, b).1
}

/// Distance between two crops in units of the fixed rule: the larger of
/// shape mismatch / `SHAPE_MISMATCH` and colour difference / `COLOR_DIFF`;
/// infinite when the core sizes or the hashes are too far apart. At most 1
/// means the same appearance under the fixed rule.
pub fn distance(a: &Crop, b: &Crop) -> f32 {
    let ((aw, ah), (bw, bh)) = (a.core_size(), b.core_size());
    let slack =
        |p: u32, q: u32| SIZE_SLACK.max((p.max(q) as f32 * SIZE_SLACK_FRACTION).round() as u32);
    if aw.abs_diff(bw) > slack(aw, bw) || ah.abs_diff(bh) > slack(ah, bh) {
        return f32::INFINITY;
    }
    if a.hash.distance(&b.hash) > HASH_BITS {
        return f32::INFINITY;
    }
    let (shape, colour) = compare(a, b);
    (shape / SHAPE_MISMATCH).max(colour / COLOR_DIFF)
}

/// Whether two crops show the same appearance under the fixed rule.
pub fn same_appearance(a: &Crop, b: &Crop) -> bool {
    distance(a, b) <= 1.0
}

/// Objects whose median core is smaller than this on its short side (px)
/// may merge poses beyond the fixed rule (see [`group_poses`]).
pub const SMALL_SPRITE: f32 = 32.0;
/// Micro-groups join crops closer than this (fixed-rule units).
const TIGHT: f32 = 0.4;
/// A jump in merge distances of at least this ratio separates compression
/// noise (below) from pose differences (above).
const GAP_RATIO: f32 = 1.5;

/// Groups one object's crops (in frame order) into poses, adapting to how
/// much compression noise this object shows: crops are first gathered into
/// tight micro-groups; the micro-groups are merged by average linkage up to
/// the fixed rule (distance 1) and, for small sprites (median core under
/// `SMALL_SPRITE` px on its short side) whose merge distances show a clear
/// jump above 1, on up to that gap. Tiny groups (fewer than 3 crops and under 5 %) are folded into
/// their nearest group when within the fixed rule. Returns each pose's first
/// crop index, in frame order.
pub fn group_poses(crops: &[Crop]) -> Vec<usize> {
    if crops.is_empty() {
        return Vec::new();
    }
    // Micro-groups (first member represents), trying the last match first.
    let mut micro: Vec<Vec<usize>> = Vec::new();
    let mut last: Option<usize> = None;
    for i in 0..crops.len() {
        let order: Vec<usize> = last
            .into_iter()
            .chain((0..micro.len()).filter(|&g| Some(g) != last))
            .collect();
        let found = order
            .into_iter()
            .find(|&g| distance(&crops[micro[g][0]], &crops[i]) <= TIGHT);
        match found {
            Some(g) => {
                micro[g].push(i);
                last = Some(g);
            }
            None => {
                micro.push(vec![i]);
                last = Some(micro.len() - 1);
            }
        }
    }
    let r = micro.len();
    if r == 1 {
        return vec![0];
    }
    let d: Vec<Vec<f32>> = (0..r)
        .map(|i| {
            (0..r)
                .map(|j| {
                    if i == j {
                        0.0
                    } else {
                        distance(&crops[micro[i][0]], &crops[micro[j][0]]).min(1e6)
                    }
                })
                .collect()
        })
        .collect();
    let d = &d;
    // Average linkage until `limit`; returns groups of micro indices and the
    // merge heights.
    let agglomerate = |limit: f32| -> (Vec<Vec<usize>>, Vec<f32>) {
        let mut groups: Vec<Vec<usize>> = (0..r).map(|i| vec![i]).collect();
        let mut heights = Vec::new();
        loop {
            let mut best: Option<(f32, usize, usize)> = None;
            for i in 0..groups.len() {
                for j in i + 1..groups.len() {
                    // Average linkage (finite pairs; any size-gated pair
                    // keeps the groups apart).
                    let pairs: Vec<f32> = groups[i]
                        .iter()
                        .flat_map(|&a| groups[j].iter().map(move |&b| d[a][b]))
                        .collect();
                    let h = if pairs.iter().any(|p| *p >= 1e6) {
                        1e6
                    } else {
                        pairs.iter().sum::<f32>() / pairs.len() as f32
                    };
                    if best.is_none_or(|(bh, _, _)| h < bh) {
                        best = Some((h, i, j));
                    }
                }
            }
            match best {
                Some((h, i, j)) if h <= limit => {
                    let g = groups.remove(j);
                    groups[i].extend(g);
                    heights.push(h);
                }
                _ => return (groups, heights),
            }
        }
    };
    let (_, mut heights) = agglomerate(1e5);
    heights.sort_by(f32::total_cmp);
    // The fixed rule merges up to 1. Small sprites, where a one-pixel
    // compression fringe is a large share of the shape, may merge further up
    // to a clear jump above 1; larger sprites keep the fixed rule, so a smooth
    // series of poses (each step just over the rule) is never collapsed.
    let mut sides: Vec<u32> = crops
        .iter()
        .map(|c| c.core_size().0.min(c.core_size().1))
        .collect();
    sides.sort_unstable();
    let small = (sides[sides.len() / 2] as f32) < SMALL_SPRITE;
    let mut cut = 1.0f32;
    let mut below = TIGHT;
    for &h in heights.iter().filter(|_| small) {
        if h > 1.0 && h >= GAP_RATIO * below {
            cut = (below * h).sqrt().max(1.0);
            break;
        }
        below = below.max(h);
    }
    let (mut groups, _) = agglomerate(cut);
    // Fold tiny groups into their nearest group within the fixed rule.
    let size = |g: &Vec<usize>| g.iter().map(|&m| micro[m].len()).sum::<usize>();
    let tiny = |n: usize| n < 3 && (n as f32) < 0.05 * crops.len() as f32;
    while let Some(t) = (0..groups.len()).find(|&i| tiny(size(&groups[i]))) {
        let nearest = (0..groups.len())
            .filter(|&j| j != t)
            .map(|j| {
                let h = groups[t]
                    .iter()
                    .flat_map(|&a| groups[j].iter().map(move |&b| d[a][b]))
                    .fold(f32::INFINITY, f32::min);
                (h, j)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        match nearest {
            Some((h, j)) if h <= 1.0 => {
                let g = groups.remove(t);
                let j = if j > t { j - 1 } else { j };
                groups[j].extend(g);
            }
            _ => break,
        }
    }
    let mut firsts: Vec<usize> = groups
        .iter()
        .map(|g| {
            g.iter()
                .map(|&m| micro[m][0])
                .min()
                .expect("non-empty group")
        })
        .collect();
    firsts.sort_unstable();
    firsts
}

// ---------------------------------------------------------------------------
// Per-frame detection

/// Keyed pieces smaller than this (pixels) are compression specks, removed
/// before detection so Merge Distance does not attach them to objects.
pub const SPECK: u32 = 5;

/// Keys `frame` automatically and returns its objects as tight crops.
pub fn detect_frame(index: u64, frame: &RgbaImage, params: &DetectParams) -> Vec<Crop> {
    let mut keyed = keying::key(frame, &KeyParams::auto(frame));
    let (w, h) = keyed.dimensions();
    let pieces = detect::label(w, h, |x, y| keyed.get_pixel(x, y)[3] > 0, true);
    for (i, p) in keyed.pixels_mut().enumerate() {
        let l = pieces.label[i] as usize;
        if l != 0 && pieces.sizes[l] < SPECK {
            *p = image::Rgba([0, 0, 0, 0]);
        }
    }
    detect::detect(&keyed, params)
        .into_iter()
        .filter_map(|p| {
            super::tight_crop_region(&keyed, &p.mask, p.rect)
                .map(|(image, rect)| Crop::new(index, rect, image))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tracking and clustering

/// 16-bin RGB histogram of opaque pixels (each channel sums to 1).
fn histogram(image: &RgbaImage) -> [[f32; 16]; 3] {
    let mut h = [[0f32; 16]; 3];
    let mut n = 0f32;
    for p in image.pixels().filter(|p| p[3] >= 128) {
        for c in 0..3 {
            h[c][p[c] as usize / 16] += 1.0;
        }
        n += 1.0;
    }
    for channel in &mut h {
        for v in channel.iter_mut() {
            *v /= n.max(1.0);
        }
    }
    h
}

fn histogram_difference(a: &[[f32; 16]; 3], b: &[[f32; 16]; 3]) -> f32 {
    let mut s = 0.0;
    for c in 0..3 {
        for i in 0..16 {
            s += (a[c][i] - b[c][i]).abs();
        }
    }
    s / 6.0
}

/// An object found across frames.
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    /// Distinct appearances, ordered by first frame.
    pub poses: Vec<Crop>,
    /// Frames the object was seen in (ascending, unique).
    pub frames: Vec<u64>,
}

impl Cluster {
    pub fn is_animation(&self) -> bool {
        self.poses.len() > 1
    }

    pub fn first_frame(&self) -> u64 {
        self.frames.first().copied().unwrap_or(0)
    }

    /// Adds `crop` as a new pose unless it repeats one.
    fn add_appearance(&mut self, crop: Crop) {
        if !self.poses.iter().any(|p| same_appearance(p, &crop)) {
            self.poses.push(crop);
            self.poses.sort_by_key(|p| p.frame);
        }
    }

    fn shares_appearance(&self, other: &Cluster) -> bool {
        self.poses
            .iter()
            .any(|a| other.poses.iter().any(|b| same_appearance(a, b)))
    }

    /// Merges clusters: poses deduplicated and ordered by first appearance.
    pub fn merge(clusters: impl IntoIterator<Item = Cluster>) -> Cluster {
        let mut all: Vec<Crop> = Vec::new();
        let mut frames = Vec::new();
        for c in clusters {
            all.extend(c.poses);
            frames.extend(c.frames);
        }
        all.sort_by_key(|p| p.frame);
        frames.sort_unstable();
        frames.dedup();
        let mut merged = Cluster {
            poses: Vec::new(),
            frames,
        };
        for crop in all {
            merged.add_appearance(crop);
        }
        merged
    }
}

struct Track {
    /// Every detection of the object, in frame order.
    crops: Vec<Crop>,
    last: Crop,
    hist: [[f32; 16]; 3],
    /// Processed-frame step it was last seen in.
    step: usize,
}

/// Whether `d` can continue a track last seen as `t`.
fn continues(t: &Track, d: &Crop, hist: &[[f32; 16]; 3]) -> Option<f32> {
    let (a, b) = (t.last.rect, d.rect);
    let overlap = a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom();
    let center = |r: PixelRect| {
        (
            r.x as f32 + r.width as f32 / 2.0,
            r.y as f32 + r.height as f32 / 2.0,
        )
    };
    let (ca, cb) = (center(a), center(b));
    let dist = ((ca.0 - cb.0).powi(2) + (ca.1 - cb.1).powi(2)).sqrt();
    let reach = a.width.max(a.height).max(b.width).max(b.height) as f32;
    let ratio = |p: u32, q: u32| p as f32 / q.max(1) as f32;
    let sized = (0.5..=2.0).contains(&ratio(b.width, a.width))
        && (0.5..=2.0).contains(&ratio(b.height, a.height));
    ((overlap || dist <= reach) && sized && histogram_difference(&t.hist, hist) <= 0.35)
        .then_some(dist)
}

/// One object's crops (frame order) as a cluster: adaptive poses, frames seen.
fn cluster_of(crops: &[Crop]) -> Cluster {
    let mut frames: Vec<u64> = crops.iter().map(|c| c.frame).collect();
    frames.dedup();
    Cluster {
        poses: group_poses(crops)
            .into_iter()
            .map(|i| crops[i].clone())
            .collect(),
        frames,
    }
}

/// Groups per-frame detections (in frame order) into clusters.
pub fn cluster(frames: Vec<Vec<Crop>>) -> Vec<Cluster> {
    let mut tracks: Vec<Track> = Vec::new();
    for (step, detections) in frames.into_iter().enumerate() {
        let hists: Vec<[[f32; 16]; 3]> = detections.iter().map(|d| histogram(&d.image)).collect();
        // All candidate pairs with tracks seen in the previous step, closest first.
        let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
        for (ti, t) in tracks.iter().enumerate() {
            if step == 0 || t.step != step - 1 {
                continue;
            }
            for (di, d) in detections.iter().enumerate() {
                if let Some(dist) = continues(t, d, &hists[di]) {
                    pairs.push((dist, ti, di));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut taken_t = vec![false; tracks.len()];
        let mut taken_d = vec![false; detections.len()];
        let mut assign: Vec<Option<usize>> = vec![None; detections.len()];
        for (_, ti, di) in pairs {
            if !taken_t[ti] && !taken_d[di] {
                taken_t[ti] = true;
                taken_d[di] = true;
                assign[di] = Some(ti);
            }
        }
        for ((d, hist), track) in detections.into_iter().zip(hists).zip(assign) {
            match track {
                Some(ti) => {
                    let t = &mut tracks[ti];
                    t.crops.push(d.clone());
                    t.last = d;
                    t.hist = hist;
                    t.step = step;
                }
                None => tracks.push(Track {
                    crops: vec![d.clone()],
                    last: d,
                    hist,
                    step,
                }),
            }
        }
    }
    // Poses per object, then link objects that share an appearance and
    // regroup their combined crops.
    let mut objects: Vec<(Vec<Crop>, Cluster)> = tracks
        .into_iter()
        .map(|t| {
            let cluster = cluster_of(&t.crops);
            (t.crops, cluster)
        })
        .collect();
    loop {
        let mut joined = false;
        'outer: for i in 0..objects.len() {
            for j in i + 1..objects.len() {
                if objects[i].1.shares_appearance(&objects[j].1) {
                    let (b, _) = objects.remove(j);
                    let (mut a, _) = objects.remove(i);
                    a.extend(b);
                    a.sort_by_key(|c| c.frame);
                    let cluster = cluster_of(&a);
                    objects.insert(i, (a, cluster));
                    joined = true;
                    break 'outer;
                }
            }
        }
        if !joined {
            break;
        }
    }
    let mut clusters: Vec<Cluster> = objects.into_iter().map(|(_, c)| c).collect();
    clusters.sort_by_key(|c| (c.first_frame(), c.poses[0].rect.y, c.poses[0].rect.x));
    clusters
}

/// Animation frames: poses on a shared canvas (largest width × largest
/// height), horizontally centered and bottom-aligned.
pub fn align_frames(poses: &[RgbaImage]) -> Vec<RgbaImage> {
    let w = poses.iter().map(|p| p.width()).max().unwrap_or(1);
    let h = poses.iter().map(|p| p.height()).max().unwrap_or(1);
    poses
        .iter()
        .map(|p| {
            let mut canvas = RgbaImage::new(w, h);
            image::imageops::replace(
                &mut canvas,
                p,
                ((w - p.width()) / 2) as i64,
                (h - p.height()) as i64,
            );
            canvas
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Background job

#[derive(Debug, Clone, PartialEq)]
pub struct VideoExtractSpec {
    pub path: PathBuf,
    pub from: u64,
    pub to: u64,
    pub every: u64,
    pub detect: DetectParams,
}

pub struct VideoExtractJob {
    progress: Arc<AtomicU64>,
    total: u64,
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<Vec<Cluster>, String>>,
    handle: Option<JoinHandle<()>>,
}

impl VideoExtractJob {
    pub fn start(spec: VideoExtractSpec, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        let total = (spec.from..=spec.to)
            .step_by(spec.every.max(1) as usize)
            .count() as u64;
        let progress = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, result) = mpsc::channel();
        let (p, c) = (progress.clone(), cancel.clone());
        let handle = std::thread::Builder::new()
            .name("video-extract-assets".to_owned())
            .spawn(move || {
                let outcome = run(&spec, &p, &c, &*notify).map_err(|e| e.to_string());
                if !c.load(Ordering::Relaxed) {
                    let _ = tx.send(outcome);
                }
                notify();
            })
            .expect("spawn video extraction");
        Self {
            progress,
            total,
            cancel,
            result,
            handle: Some(handle),
        }
    }

    pub fn progress(&self) -> f32 {
        (self.progress.load(Ordering::Relaxed) as f32 / self.total.max(1) as f32).min(1.0)
    }

    pub fn try_result(&self) -> Option<Result<Vec<Cluster>, String>> {
        self.result.try_recv().ok()
    }

    /// Stops the run without waiting for the worker (it exits after its
    /// current frame; its result is never delivered).
    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        drop(self.handle.take());
    }
}

impl Drop for VideoExtractJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        drop(self.handle.take());
    }
}

/// Runs a whole-video extraction synchronously (the job's body; tests).
pub fn run(
    spec: &VideoExtractSpec,
    progress: &AtomicU64,
    cancel: &AtomicBool,
    notify: &dyn Fn(),
) -> Result<Vec<Cluster>, VideoError> {
    let mut source = VideoSource::open(&spec.path)?;
    let last = source.info().last_frame();
    let to = spec.to.min(last);
    let from = spec.from.min(to);
    let every = spec.every.max(1);
    let mut per_frame = Vec::new();
    let mut done = 0;
    let mut next = Some(source.frame(from)?);
    while let Some(frame) = next {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Vec::new());
        }
        if frame.index > to {
            break;
        }
        if (frame.index - from) % every == 0 {
            per_frame.push(detect_frame(frame.index, &frame.image, &spec.detect));
            done += 1;
            progress.store(done, Ordering::Relaxed);
            if done % 4 == 0 {
                notify();
            }
        }
        next = if every > 30 {
            let target = frame.index + every;
            if target > to {
                None
            } else {
                Some(source.frame(target)?)
            }
        } else {
            source.next_frame()?
        };
    }
    Ok(cluster(per_frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn blob(w: u32, h: u32, color: [u8; 3]) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            let inside = ((x as f32 - w as f32 / 2.0) / (w as f32 / 2.0)).powi(2)
                + ((y as f32 - h as f32 / 2.0) / (h as f32 / 2.0)).powi(2)
                <= 1.0;
            if inside {
                Rgba([color[0], color[1], color[2], 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        })
    }

    #[test]
    fn identical_and_noisy_copies_are_one_appearance_distinct_shapes_are_not() {
        let a = Crop::new(0, PixelRect::new(0, 0, 40, 30), blob(40, 30, [200, 40, 40]));
        let mut noisy = a.image.clone();
        for (i, p) in noisy.pixels_mut().enumerate() {
            if p[3] > 0 {
                p[0] = p[0].saturating_sub((i % 5) as u8);
            }
        }
        let b = Crop::new(9, PixelRect::new(50, 0, 40, 30), noisy);
        assert!(same_appearance(&a, &b));
        let tall = Crop::new(1, PixelRect::new(0, 0, 30, 40), blob(30, 40, [200, 40, 40]));
        assert!(!same_appearance(&a, &tall), "different size");
        let mut half = a.image.clone();
        for (x, _, p) in half.enumerate_pixels_mut() {
            if x > 20 {
                *p = Rgba([0, 0, 0, 0]);
            }
        }
        let c = Crop::new(2, PixelRect::new(0, 0, 40, 30), half);
        assert!(!same_appearance(&a, &c), "different shape");
        assert_eq!(phash(&a.image), phash(&a.image.clone()));
        let blue = Crop::new(3, PixelRect::new(0, 0, 40, 30), blob(40, 30, [40, 40, 200]));
        assert!(!same_appearance(&a, &blue), "same shape, different colour");
        // A one-pixel edge wobble is the same shape.
        let mut wobble = a.image.clone();
        for y in 0..30 {
            let x = (0..40).find(|&x| wobble.get_pixel(x, y)[3] > 0);
            if let (Some(x), true) = (x, y % 2 == 0) {
                wobble.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }
        assert!(same_appearance(&a, &Crop::new(4, a.rect, wobble)));
    }

    #[test]
    fn tracking_groups_a_moving_animation_and_a_static_object() {
        // A static blob, and an object cycling 3 sizes while moving right.
        let poses = [
            blob(20, 40, [240, 240, 220]),
            blob(26, 40, [240, 240, 220]),
            blob(32, 40, [240, 240, 220]),
        ];
        let frames: Vec<Vec<Crop>> = (0..30u64)
            .map(|t| {
                let k = (t as usize / 2) % 3;
                vec![
                    Crop::new(
                        t,
                        PixelRect::new(10, 10, 30, 30),
                        blob(30, 30, [200, 30, 40]),
                    ),
                    Crop::new(
                        t,
                        PixelRect::new(100 + t as u32 * 2, 10, poses[k].width(), 40),
                        poses[k].clone(),
                    ),
                ]
            })
            .collect();
        let clusters = cluster(frames);
        assert_eq!(clusters.len(), 2);
        let still = clusters.iter().find(|c| !c.is_animation()).unwrap();
        assert_eq!(still.frames.len(), 30);
        let anim = clusters.iter().find(|c| c.is_animation()).unwrap();
        let widths: Vec<u32> = anim.poses.iter().map(|p| p.image.width()).collect();
        assert_eq!(
            widths,
            vec![20, 26, 32],
            "poses in first-appearance order, loop collapsed"
        );
        let merged = Cluster::merge(clusters.clone());
        assert_eq!(merged.poses.len(), 4);
        assert_eq!(merged.frames.len(), 30);
    }

    /// Noise within a pose (one-pixel edge wobble) stays one pose; different
    /// poses stay apart; a static object stays one pose.
    #[test]
    fn adaptive_grouping_absorbs_noise_and_keeps_poses() {
        let pose = |w: u32, wobble: bool| {
            let mut img = blob(w, 40, [230, 230, 210]);
            if wobble {
                for y in (0..40).step_by(3) {
                    if let Some(x) = (0..w).find(|&x| img.get_pixel(x, y)[3] > 0) {
                        img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                    }
                }
            }
            img
        };
        let crops: Vec<Crop> = (0..40u64)
            .map(|t| {
                let w = [20, 30, 40][(t as usize / 2) % 3];
                Crop::new(t, PixelRect::new(0, 0, w, 40), pose(w, t % 5 == 0))
            })
            .collect();
        let poses = group_poses(&crops);
        let widths: Vec<u32> = poses.iter().map(|&i| crops[i].image.width()).collect();
        assert_eq!(widths, vec![20, 30, 40]);
        let still: Vec<Crop> = (0..30u64)
            .map(|t| Crop::new(t, PixelRect::new(0, 0, 30, 30), pose(30, t % 4 == 0)))
            .collect();
        assert_eq!(group_poses(&still), vec![0]);
    }

    /// A 48 px robot with a 1 px checker head, striped body, and four limbs
    /// swinging through a 10-pose loop: neighbouring poses differ only a
    /// little more than the fixed rule; the series is not collapsed (merging
    /// past the rule is for small sprites only).
    #[test]
    fn smooth_pose_series_on_a_large_sprite_is_not_collapsed() {
        let seg = |p: (f32, f32), a: (f32, f32), b: (f32, f32)| {
            let (vx, vy) = (b.0 - a.0, b.1 - a.1);
            let t = (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
            ((p.0 - a.0 - t * vx).powi(2) + (p.1 - a.1 - t * vy).powi(2)).sqrt()
        };
        let limb = |angle: f32, len: f32, from: (f32, f32)| {
            (from.0 + angle.sin() * len, from.1 + angle.cos() * len)
        };
        let robot = |k: u32| {
            let ph = k as f32 / 10.0 * std::f32::consts::TAU;
            let parts = [
                (
                    (28.0, 8.0),
                    limb(std::f32::consts::PI + 0.5 * ph.sin(), 7.0, (28.0, 8.0)),
                    1.2,
                ),
                ((28.0, 26.0), (28.0, 28.0), 2.0),
                (
                    (14.0, 32.0),
                    limb(-1.0 - 0.8 * ph.cos(), 20.0, (14.0, 32.0)),
                    1.6,
                ),
                (
                    (42.0, 32.0),
                    limb(1.0 + 0.8 * ph.sin(), 20.0, (42.0, 32.0)),
                    1.6,
                ),
                ((22.0, 60.0), limb(0.5 * ph.sin(), 26.0, (22.0, 60.0)), 2.0),
                ((34.0, 60.0), limb(-0.5 * ph.cos(), 26.0, (34.0, 60.0)), 2.0),
            ];
            RgbaImage::from_fn(56, 90, |x, y| {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                if (14.0..42.0).contains(&fx) && (8.0..26.0).contains(&fy) {
                    return if (x + y) % 2 == 0 {
                        Rgba([210, 210, 225, 255])
                    } else {
                        Rgba([70, 80, 120, 255])
                    };
                }
                if (12.0..44.0).contains(&fx) && (28.0..60.0).contains(&fy) {
                    return if (y / 2) % 2 == 0 {
                        Rgba([230, 120, 40, 255])
                    } else {
                        Rgba([40, 40, 40, 255])
                    };
                }
                if parts.iter().any(|&(a, b, r)| seg((fx, fy), a, b) <= r) {
                    Rgba([200, 200, 210, 255])
                } else {
                    Rgba([0, 0, 0, 0])
                }
            })
        };
        let crops: Vec<Crop> = (0..80u64)
            .map(|t| Crop::new(t, PixelRect::new(0, 0, 56, 90), robot((t / 2 % 10) as u32)))
            .collect();
        let poses = group_poses(&crops);
        let first: Vec<u64> = poses.iter().map(|&i| crops[i].frame).collect();
        // Poses 0–3 are 3–4 % apart (within the stated limit, may merge);
        // poses 4–9 are clearly apart and must all be kept. Collapsing the
        // series (as merging past the rule did) leaves only ~3 poses.
        assert!(first.len() >= 8, "{first:?}");
        for f in [8, 10, 12, 14, 16, 18] {
            assert!(first.contains(&f), "pose at frame {f} kept: {first:?}");
        }
    }

    #[test]
    fn partly_recoloured_copies_are_different_appearances() {
        let base = blob(40, 40, [200, 200, 200]);
        let mut wing = base.clone();
        for (x, y, p) in wing.enumerate_pixels_mut() {
            if x < 20 && y < 20 && p[3] > 0 {
                *p = Rgba([200, 60, 60, 255]); // one quadrant recoloured
            }
        }
        let a = Crop::new(0, PixelRect::new(0, 0, 40, 40), base);
        let b = Crop::new(1, PixelRect::new(0, 0, 40, 40), wing);
        assert!(!same_appearance(&a, &b));
    }

    #[test]
    fn frames_align_bottom_centre() {
        let frames = align_frames(&[
            RgbaImage::from_pixel(2, 2, Rgba([1, 1, 1, 255])),
            RgbaImage::new(6, 5),
        ]);
        assert!(frames.iter().all(|f| f.dimensions() == (6, 5)));
        assert_eq!(frames[0].get_pixel(2, 3)[3], 255);
        assert_eq!(frames[0].get_pixel(2, 2)[3], 0);
    }
}
