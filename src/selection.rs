//! Selection masks: tool shapes, combine modes, and mask operations.
//!
//! Masks are 8-bit `GrayImage`s of the document size (0 = unselected,
//! 255 = fully selected). All functions here are pure and window-independent.
//! Point coordinates are in image pixels, where pixel `(x, y)` covers
//! `[x, x+1) × [y, y+1)` and its center is `(x + 0.5, y + 0.5)`.

use image::{GrayImage, Luma, RgbaImage};

use crate::document::PixelRect;

/// How a new selection shape combines with the existing selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CombineMode {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}

impl CombineMode {
    pub const ALL: [CombineMode; 4] = [Self::Replace, Self::Add, Self::Subtract, Self::Intersect];

    pub fn label(self) -> &'static str {
        match self {
            Self::Replace => "Replace",
            Self::Add => "Add",
            Self::Subtract => "Subtract",
            Self::Intersect => "Intersect",
        }
    }

    /// The mode chosen by modifiers held at gesture start, falling back to `default`.
    pub fn from_modifiers(shift: bool, alt: bool, default: CombineMode) -> Self {
        match (shift, alt) {
            (true, true) => Self::Intersect,
            (true, false) => Self::Add,
            (false, true) => Self::Subtract,
            (false, false) => default,
        }
    }
}

/// `None` when the mask selects nothing.
pub fn normalize(mask: GrayImage) -> Option<GrayImage> {
    mask.as_raw().iter().any(|v| *v != 0).then_some(mask)
}

/// Combines `shape` with the existing selection. Returns `None` when the result is empty.
pub fn combine(
    existing: Option<&GrayImage>,
    shape: &GrayImage,
    mode: CombineMode,
) -> Option<GrayImage> {
    let mut out = shape.clone();
    match (mode, existing) {
        (CombineMode::Replace, _) => {}
        (CombineMode::Add, Some(a)) => {
            for (o, a) in out.iter_mut().zip(a.iter()) {
                *o = (*o).max(*a);
            }
        }
        (CombineMode::Add, None) => {}
        (CombineMode::Subtract, Some(a)) => {
            for (o, a) in out.iter_mut().zip(a.iter()) {
                *o = ((*a as u32 * (255 - *o as u32)) as f32 / 255.0).round() as u8;
            }
        }
        (CombineMode::Intersect, Some(a)) => {
            for (o, a) in out.iter_mut().zip(a.iter()) {
                *o = (*o).min(*a);
            }
        }
        (CombineMode::Subtract | CombineMode::Intersect, None) => return None,
    }
    normalize(out)
}

pub fn select_all(width: u32, height: u32) -> GrayImage {
    GrayImage::from_pixel(width, height, Luma([255]))
}

/// Inverts the selection; no selection inverts to everything.
pub fn invert(existing: Option<&GrayImage>, width: u32, height: u32) -> Option<GrayImage> {
    match existing {
        Some(mask) => {
            let mut out = mask.clone();
            out.iter_mut().for_each(|v| *v = 255 - *v);
            normalize(out)
        }
        None => Some(select_all(width, height)),
    }
}

/// Bounding box of non-zero pixels.
pub fn bounds(mask: &GrayImage) -> Option<PixelRect> {
    let (w, h) = mask.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (y, row) in mask.as_raw().chunks_exact(w as usize).enumerate() {
        if let Some(first) = row.iter().position(|v| *v != 0) {
            let last = row.iter().rposition(|v| *v != 0).unwrap_or(first);
            x0 = x0.min(first as u32);
            x1 = x1.max(last as u32 + 1);
            y0 = y0.min(y as u32);
            y1 = y as u32 + 1;
        }
    }
    (x1 > x0).then(|| PixelRect::new(x0, y0, x1 - x0, y1 - y0))
}

/// Pixels whose centers lie inside the rectangle spanned by `a` and `b`.
pub fn rect_mask(width: u32, height: u32, a: (f32, f32), b: (f32, f32)) -> GrayImage {
    let (min_x, max_x) = (a.0.min(b.0), a.0.max(b.0));
    let (min_y, max_y) = (a.1.min(b.1), a.1.max(b.1));
    let mut mask = GrayImage::new(width, height);
    let range = |lo: f32, hi: f32, n: u32| {
        // centers c = i + 0.5 with lo <= c <= hi
        let start = (lo - 0.5).ceil().max(0.0) as u32;
        let end = ((hi - 0.5).floor() + 1.0).clamp(0.0, n as f32) as u32;
        start..end.max(start)
    };
    for y in range(min_y, max_y, height) {
        for x in range(min_x, max_x, width) {
            mask.put_pixel(x, y, Luma([255]));
        }
    }
    mask
}

const SUBSAMPLES: u32 = 4;

/// Runs `inside(px, py)` on sample points of every pixel in `area`:
/// a 4×4 grid when anti-aliased (coverage 0–255), else the pixel center (0/255).
fn rasterize(
    width: u32,
    height: u32,
    area: Option<PixelRect>,
    antialias: bool,
    inside: impl Fn(f32, f32) -> bool,
) -> GrayImage {
    let mut mask = GrayImage::new(width, height);
    let Some(area) = area.and_then(|a| a.intersect(PixelRect::new(0, 0, width, height))) else {
        return mask;
    };
    let n = if antialias { SUBSAMPLES } else { 1 };
    let total = n * n;
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let mut hits = 0;
            for j in 0..n {
                for i in 0..n {
                    let px = x as f32 + (i as f32 + 0.5) / n as f32;
                    let py = y as f32 + (j as f32 + 0.5) / n as f32;
                    if inside(px, py) {
                        hits += 1;
                    }
                }
            }
            if hits > 0 {
                mask.put_pixel(
                    x,
                    y,
                    Luma([((hits * 255) as f32 / total as f32).round() as u8]),
                );
            }
        }
    }
    mask
}

/// Pixels covering `[min, max]` (float bounds), as a pixel rectangle.
fn covering_rect(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Option<PixelRect> {
    if !(min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite()) {
        return None;
    }
    let x0 = min_x.floor().max(0.0) as u32;
    let y0 = min_y.floor().max(0.0) as u32;
    let x1 = max_x.ceil().max(0.0) as u32;
    let y1 = max_y.ceil().max(0.0) as u32;
    (x1 > x0 && y1 > y0).then(|| PixelRect::new(x0, y0, x1 - x0, y1 - y0))
}

/// The ellipse inscribed in the rectangle spanned by `a` and `b`.
pub fn ellipse_mask(
    width: u32,
    height: u32,
    a: (f32, f32),
    b: (f32, f32),
    antialias: bool,
) -> GrayImage {
    let (min_x, max_x) = (a.0.min(b.0), a.0.max(b.0));
    let (min_y, max_y) = (a.1.min(b.1), a.1.max(b.1));
    let (rx, ry) = ((max_x - min_x) / 2.0, (max_y - min_y) / 2.0);
    if rx <= 0.0 || ry <= 0.0 {
        return GrayImage::new(width, height);
    }
    let (cx, cy) = (min_x + rx, min_y + ry);
    rasterize(
        width,
        height,
        covering_rect(min_x, min_y, max_x, max_y),
        antialias,
        |x, y| {
            let dx = (x - cx) / rx;
            let dy = (y - cy) / ry;
            dx * dx + dy * dy <= 1.0
        },
    )
}

/// The closed polygon's interior by the even-odd rule. Fewer than three
/// distinct points select nothing.
pub fn polygon_mask(width: u32, height: u32, points: &[(f32, f32)], antialias: bool) -> GrayImage {
    let mut distinct: Vec<(f32, f32)> = Vec::with_capacity(points.len());
    for p in points {
        if distinct.last() != Some(p) {
            distinct.push(*p);
        }
    }
    if distinct.len() > 1 && distinct.first() == distinct.last() {
        distinct.pop();
    }
    if distinct.len() < 3 {
        return GrayImage::new(width, height);
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in &distinct {
        min_x = min_x.min(*x);
        min_y = min_y.min(*y);
        max_x = max_x.max(*x);
        max_y = max_y.max(*y);
    }
    rasterize(
        width,
        height,
        covering_rect(min_x, min_y, max_x, max_y),
        antialias,
        |x, y| {
            let mut inside = false;
            let mut j = distinct.len() - 1;
            for i in 0..distinct.len() {
                let (xi, yi) = distinct[i];
                let (xj, yj) = distinct[j];
                if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                    inside = !inside;
                }
                j = i;
            }
            inside
        },
    )
}

/// Pixels of `layer` within `tolerance` (max RGBA channel difference) of the
/// seed pixel's color; contiguous limits to the 4-connected region.
pub fn magic_wand(
    layer: &RgbaImage,
    seed: (u32, u32),
    tolerance: u8,
    contiguous: bool,
) -> GrayImage {
    let (w, h) = layer.dimensions();
    let mut mask = GrayImage::new(w, h);
    if seed.0 >= w || seed.1 >= h {
        return mask;
    }
    let target = layer.get_pixel(seed.0, seed.1).0;
    let matches = |x: u32, y: u32| {
        let p = layer.get_pixel(x, y).0;
        (0..4).all(|c| p[c].abs_diff(target[c]) <= tolerance)
    };
    if !contiguous {
        for (x, y, m) in mask.enumerate_pixels_mut() {
            if matches(x, y) {
                *m = Luma([255]);
            }
        }
        return mask;
    }
    let mut stack = vec![seed];
    mask.put_pixel(seed.0, seed.1, Luma([255]));
    while let Some((x, y)) = stack.pop() {
        let neighbors = [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ];
        for (nx, ny) in neighbors {
            if nx < w && ny < h && mask.get_pixel(nx, ny)[0] == 0 && matches(nx, ny) {
                mask.put_pixel(nx, ny, Luma([255]));
                stack.push((nx, ny));
            }
        }
    }
    mask
}

/// Half-widths of a Euclidean disk of `radius`, indexed by row offset `|dy|`.
fn disk_half_widths(radius: u32) -> Vec<u32> {
    let r2 = (radius * radius) as i64;
    (0..=radius as i64)
        .map(|dy| (((r2 - dy * dy) as f64).sqrt().floor()) as u32)
        .collect()
}

/// Max (grow) or min (shrink) of the mask over a disk. Samples outside the
/// document read as `pad`; only the selection's bounding box plus `radius`
/// is processed, since everything else is unchanged.
fn disk_filter(mask: &GrayImage, radius: u32, grow: bool) -> Option<GrayImage> {
    let (w, h) = mask.dimensions();
    let bbox = bounds(mask)?;
    if radius == 0 {
        return Some(mask.clone());
    }
    let pad: u8 = if grow { 0 } else { 255 };
    let r = radius as i64;
    // Region whose output can change.
    let region = if grow {
        let x0 = (bbox.x as i64 - r).max(0) as u32;
        let y0 = (bbox.y as i64 - r).max(0) as u32;
        let x1 = (bbox.right() as i64 + r).min(w as i64) as u32;
        let y1 = (bbox.bottom() as i64 + r).min(h as i64) as u32;
        PixelRect::new(x0, y0, x1 - x0, y1 - y0)
    } else {
        bbox
    };
    // Padded input covering region ± radius.
    let pw = region.width as i64 + 2 * r;
    let ph = region.height as i64 + 2 * r;
    let ox = region.x as i64 - r;
    let oy = region.y as i64 - r;
    let mut input = vec![0u8; (pw * ph) as usize];
    for py in 0..ph {
        for px in 0..pw {
            let (x, y) = (ox + px, oy + py);
            input[(py * pw + px) as usize] = if x >= 0 && y >= 0 && x < w as i64 && y < h as i64 {
                mask.get_pixel(x as u32, y as u32)[0]
            } else {
                pad
            };
        }
    }
    let pick = |a: u8, b: u8| if grow { a.max(b) } else { a.min(b) };
    let identity = if grow { 0u8 } else { 255u8 };
    let half = disk_half_widths(radius);
    let (rw, rh) = (region.width as usize, region.height as usize);
    let mut out = vec![identity; rw * rh];
    let mut row_filtered = vec![identity; (pw * ph) as usize];
    let mut done = std::collections::HashSet::new();
    for &hw in &half {
        if !done.insert(hw) {
            continue;
        }
        // Horizontal min/max over [x-hw, x+hw] for every padded row.
        for py in 0..ph as usize {
            let row = &input[py * pw as usize..(py + 1) * pw as usize];
            for x in r as usize..r as usize + rw {
                let lo = x - hw as usize;
                let hi = (x + hw as usize).min(pw as usize - 1);
                row_filtered[py * pw as usize + x] =
                    row[lo..=hi].iter().copied().fold(identity, pick);
            }
        }
        // Combine rows whose disk half-width is `hw`.
        for (dy, _) in half.iter().enumerate().filter(|(_, v)| **v == hw) {
            for sign in [-1i64, 1] {
                if dy == 0 && sign == 1 {
                    continue;
                }
                let d = sign * dy as i64;
                for y in 0..rh {
                    let py = (y as i64 + r + d) as usize;
                    for x in 0..rw {
                        let v = row_filtered[py * pw as usize + x + r as usize];
                        let o = &mut out[y * rw + x];
                        *o = pick(*o, v);
                    }
                }
            }
        }
    }
    let mut result = mask.clone();
    for y in 0..rh {
        for x in 0..rw {
            result.put_pixel(
                region.x + x as u32,
                region.y + y as u32,
                Luma([out[y * rw + x]]),
            );
        }
    }
    normalize(result)
}

/// Dilates the selection by a Euclidean disk of `radius` pixels.
pub fn grow(mask: &GrayImage, radius: u32) -> Option<GrayImage> {
    disk_filter(mask, radius, true)
}

/// Erodes the selection by a Euclidean disk of `radius` pixels; the canvas
/// edge does not erode it.
pub fn shrink(mask: &GrayImage, radius: u32) -> Option<GrayImage> {
    disk_filter(mask, radius, false)
}

/// Gaussian blur of the mask with σ = radius / 2 (kernel truncated at 3σ,
/// clamped at the document edges).
pub fn feather(mask: &GrayImage, radius: u32) -> Option<GrayImage> {
    let (w, h) = mask.dimensions();
    let bbox = bounds(mask)?;
    if radius == 0 {
        return Some(mask.clone());
    }
    let sigma = radius as f32 / 2.0;
    let k = (3.0 * sigma).ceil() as i64;
    let weights: Vec<f32> = (-k..=k)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = weights.iter().sum();
    let weights: Vec<f32> = weights.iter().map(|v| v / total).collect();

    let clamp_x = |x: i64| x.clamp(0, w as i64 - 1) as u32;
    let clamp_y = |y: i64| y.clamp(0, h as i64 - 1) as u32;
    // Output region: bbox ± k. Horizontal pass rows: region rows ± k.
    let rx0 = (bbox.x as i64 - k).max(0);
    let rx1 = (bbox.right() as i64 + k).min(w as i64);
    let ry0 = (bbox.y as i64 - k).max(0);
    let ry1 = (bbox.bottom() as i64 + k).min(h as i64);
    let hy0 = (ry0 - k).max(0);
    let hy1 = (ry1 + k).min(h as i64);
    let hw = (rx1 - rx0) as usize;
    let mut horizontal = vec![0f32; hw * (hy1 - hy0) as usize];
    for y in hy0..hy1 {
        for x in rx0..rx1 {
            let mut sum = 0.0;
            for (i, wgt) in weights.iter().enumerate() {
                sum += wgt * mask.get_pixel(clamp_x(x + i as i64 - k), y as u32)[0] as f32;
            }
            horizontal[(y - hy0) as usize * hw + (x - rx0) as usize] = sum;
        }
    }
    let h_at = |x: i64, y: i64| {
        let y = clamp_y(y) as i64;
        if y < hy0 || y >= hy1 {
            0.0
        } else {
            horizontal[(y - hy0) as usize * hw + (x - rx0) as usize]
        }
    };
    let mut result = mask.clone();
    for y in ry0..ry1 {
        for x in rx0..rx1 {
            let mut sum = 0.0;
            for (i, wgt) in weights.iter().enumerate() {
                sum += wgt * h_at(x, y + i as i64 - k);
            }
            result.put_pixel(
                x as u32,
                y as u32,
                Luma([sum.round().clamp(0.0, 255.0) as u8]),
            );
        }
    }
    normalize(result)
}

/// A straight piece of the selection boundary, in image pixel corner coordinates.
pub type Segment = ((u32, u32), (u32, u32));

/// Boundary between selected (≥ 128) and unselected pixels (or the document
/// edge), merged into maximal horizontal and vertical segments.
pub fn outline(mask: &GrayImage) -> Vec<Segment> {
    let (w, h) = mask.dimensions();
    let selected = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && x < w as i64
            && y < h as i64
            && mask.get_pixel(x as u32, y as u32)[0] >= 128
    };
    let mut segments = Vec::new();
    // Horizontal edges at y between rows y-1 and y.
    for y in 0..=h as i64 {
        let mut start: Option<u32> = None;
        for x in 0..=w as i64 {
            let edge = x < w as i64 && selected(x, y - 1) != selected(x, y);
            match (edge, start) {
                (true, None) => start = Some(x as u32),
                (false, Some(s)) => {
                    segments.push(((s, y as u32), (x as u32, y as u32)));
                    start = None;
                }
                _ => {}
            }
        }
    }
    // Vertical edges at x between columns x-1 and x.
    for x in 0..=w as i64 {
        let mut start: Option<u32> = None;
        for y in 0..=h as i64 {
            let edge = y < h as i64 && selected(x - 1, y) != selected(x, y);
            match (edge, start) {
                (true, None) => start = Some(y as u32),
                (false, Some(s)) => {
                    segments.push(((x as u32, s), (x as u32, y as u32)));
                    start = None;
                }
                _ => {}
            }
        }
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn selected(mask: &GrayImage) -> Vec<(u32, u32)> {
        mask.enumerate_pixels()
            .filter(|(_, _, v)| v[0] == 255)
            .map(|(x, y, _)| (x, y))
            .collect()
    }

    fn count(mask: &GrayImage, pred: impl Fn(u8) -> bool) -> usize {
        mask.iter().filter(|v| pred(**v)).count()
    }

    #[test]
    fn rect_selects_pixel_centers_inside() {
        let m = rect_mask(10, 8, (2.2, 1.0), (5.6, 3.5));
        // centers 2.5..5.5 → x 2..=5; centers 1.5..3.5 → y 1..=3 (3.5 inclusive)
        let sel = selected(&m);
        assert_eq!(sel.len(), 4 * 3);
        assert!(sel.contains(&(2, 1)) && sel.contains(&(5, 3)));
        assert!(!sel.contains(&(6, 1)) && !sel.contains(&(2, 4)));
        assert_eq!(count(&m, |v| v != 0 && v != 255), 0, "hard edges");
        // Drag direction does not matter.
        assert_eq!(rect_mask(10, 8, (5.6, 3.5), (2.2, 1.0)), m);
        // Clipped to the document.
        let clipped = rect_mask(10, 8, (-5.0, -5.0), (3.0, 100.0));
        assert_eq!(selected(&clipped).len(), 3 * 8);
        // Too small to contain a center.
        assert!(normalize(rect_mask(10, 8, (2.6, 2.6), (3.4, 3.4))).is_none());
    }

    #[test]
    fn ellipse_hard_and_antialiased() {
        let hard = ellipse_mask(20, 20, (0.0, 0.0), (20.0, 20.0), false);
        assert_eq!(count(&hard, |v| v != 0 && v != 255), 0);
        assert_eq!(hard.get_pixel(10, 10)[0], 255);
        assert_eq!(hard.get_pixel(0, 0)[0], 0);
        assert_eq!(hard.get_pixel(0, 10)[0], 255); // center (0.5, 10.5) inside
                                                   // Area close to π r².
        let area = count(&hard, |v| v == 255) as f32;
        assert!((area - std::f32::consts::PI * 100.0).abs() < 12.0, "{area}");

        let soft = ellipse_mask(20, 20, (0.0, 0.0), (20.0, 20.0), true);
        assert_eq!(soft.get_pixel(10, 10)[0], 255);
        assert_eq!(soft.get_pixel(0, 0)[0], 0);
        assert!(
            count(&soft, |v| v != 0 && v != 255) > 20,
            "partial edge pixels"
        );
        let coverage: f32 = soft.iter().map(|v| *v as f32 / 255.0).sum();
        assert!(
            (coverage - std::f32::consts::PI * 100.0).abs() < 3.0,
            "{coverage}"
        );
        // Degenerate drag selects nothing.
        assert!(normalize(ellipse_mask(20, 20, (3.0, 3.0), (3.0, 9.0), true)).is_none());
    }

    #[test]
    fn polygon_triangle_and_even_odd() {
        let tri = [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)];
        let hard = polygon_mask(10, 10, &tri, false);
        assert_eq!(hard.get_pixel(0, 0)[0], 255);
        assert_eq!(hard.get_pixel(9, 9)[0], 0);
        assert_eq!(hard.get_pixel(4, 4)[0], 255); // center (4.5,4.5): 4.5+4.5 < 10
        assert_eq!(hard.get_pixel(5, 5)[0], 0);
        let soft = polygon_mask(10, 10, &tri, true);
        let coverage: f32 = soft.iter().map(|v| *v as f32 / 255.0).sum();
        // Samples landing exactly on the diagonal count as outside (~1.25 px² low).
        assert!((coverage - 50.0).abs() < 1.5, "{coverage}");
        assert!(count(&soft, |v| v != 0 && v != 255) >= 9);

        // Pentagram: the center pentagon is outside under even-odd.
        let star: Vec<(f32, f32)> = (0..5)
            .map(|i| {
                let a = std::f32::consts::FRAC_PI_2 + i as f32 * 4.0 * std::f32::consts::PI / 5.0;
                (50.0 + 40.0 * a.cos(), 50.0 - 40.0 * a.sin())
            })
            .collect();
        let m = polygon_mask(100, 100, &star, false);
        assert_eq!(m.get_pixel(50, 50)[0], 0, "even-odd hole");
        assert_eq!(m.get_pixel(50, 15)[0], 255, "star point");

        // Fewer than three distinct points.
        assert!(normalize(polygon_mask(
            10,
            10,
            &[(1.0, 1.0), (5.0, 5.0), (5.0, 5.0)],
            true
        ))
        .is_none());
    }

    fn two_regions() -> RgbaImage {
        // Left and right red regions separated by a black barrier column.
        RgbaImage::from_fn(9, 4, |x, _| {
            if x == 4 {
                Rgba([0, 0, 0, 255])
            } else if x < 4 {
                Rgba([200, 0, 0, 255])
            } else {
                Rgba([210, 5, 0, 255])
            }
        })
    }

    #[test]
    fn magic_wand_contiguous_and_global() {
        let img = two_regions();
        let contiguous = magic_wand(&img, (1, 1), 32, true);
        assert_eq!(selected(&contiguous).len(), 4 * 4);
        assert!(selected(&contiguous).iter().all(|(x, _)| *x < 4));
        let global = magic_wand(&img, (1, 1), 32, false);
        assert_eq!(selected(&global).len(), 8 * 4);
        // Tolerance below the 10-level difference keeps the right side out.
        assert_eq!(selected(&magic_wand(&img, (1, 1), 9, false)).len(), 16);
        // Alpha counts as a channel.
        let mut alpha = img.clone();
        alpha.put_pixel(0, 0, Rgba([200, 0, 0, 100]));
        assert_eq!(magic_wand(&alpha, (1, 1), 32, true).get_pixel(0, 0)[0], 0);
        // Hard edges; outside clicks select nothing.
        assert_eq!(count(&global, |v| v != 0 && v != 255), 0);
        assert!(normalize(magic_wand(&img, (20, 0), 32, true)).is_none());
    }

    #[test]
    fn magic_wand_is_four_connected() {
        let img = RgbaImage::from_fn(3, 3, |x, y| {
            if (x == 1) ^ (y == 1) {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });
        // Corners only touch the center diagonally.
        assert_eq!(selected(&magic_wand(&img, (0, 0), 0, true)), vec![(0, 0)]);
    }

    #[test]
    fn combine_modes() {
        let a = GrayImage::from_raw(4, 1, vec![0, 255, 255, 128]).unwrap();
        let b = GrayImage::from_raw(4, 1, vec![255, 255, 0, 128]).unwrap();
        assert_eq!(
            combine(Some(&a), &b, CombineMode::Replace)
                .unwrap()
                .into_raw(),
            vec![255, 255, 0, 128]
        );
        assert_eq!(
            combine(Some(&a), &b, CombineMode::Add).unwrap().into_raw(),
            vec![255, 255, 255, 128]
        );
        assert_eq!(
            combine(Some(&a), &b, CombineMode::Subtract)
                .unwrap()
                .into_raw(),
            vec![0, 0, 255, 64]
        );
        assert_eq!(
            combine(Some(&a), &b, CombineMode::Intersect)
                .unwrap()
                .into_raw(),
            vec![0, 255, 0, 128]
        );
        // No existing selection.
        assert_eq!(combine(None, &b, CombineMode::Add).unwrap(), b);
        assert!(combine(None, &b, CombineMode::Subtract).is_none());
        assert!(combine(None, &b, CombineMode::Intersect).is_none());
        // Subtracting everything clears.
        assert!(combine(Some(&a), &select_all(4, 1), CombineMode::Subtract).is_none());
    }

    #[test]
    fn modifiers_pick_mode() {
        let d = CombineMode::Replace;
        assert_eq!(CombineMode::from_modifiers(false, false, d), d);
        assert_eq!(
            CombineMode::from_modifiers(false, false, CombineMode::Add),
            CombineMode::Add
        );
        assert_eq!(
            CombineMode::from_modifiers(true, false, d),
            CombineMode::Add
        );
        assert_eq!(
            CombineMode::from_modifiers(false, true, d),
            CombineMode::Subtract
        );
        assert_eq!(
            CombineMode::from_modifiers(true, true, d),
            CombineMode::Intersect
        );
    }

    #[test]
    fn all_invert_bounds() {
        assert_eq!(count(&select_all(3, 2), |v| v == 255), 6);
        assert_eq!(invert(None, 3, 2).unwrap(), select_all(3, 2));
        let m = GrayImage::from_raw(3, 1, vec![0, 255, 100]).unwrap();
        assert_eq!(
            invert(Some(&m), 3, 1).unwrap().into_raw(),
            vec![255, 0, 155]
        );
        assert!(invert(Some(&select_all(3, 1)), 3, 1).is_none());
        assert_eq!(
            bounds(&rect_mask(10, 10, (2.0, 3.0), (5.0, 9.0))),
            Some(PixelRect::new(2, 3, 3, 6))
        );
        assert_eq!(bounds(&GrayImage::new(4, 4)), None);
    }

    #[test]
    fn grow_and_shrink_use_a_disk() {
        let mut m = GrayImage::new(21, 21);
        m.put_pixel(10, 10, Luma([255]));
        let g = grow(&m, 3).unwrap();
        assert_eq!(g.get_pixel(13, 10)[0], 255);
        assert_eq!(g.get_pixel(10, 7)[0], 255);
        assert_eq!(g.get_pixel(12, 12)[0], 255); // distance √8 ≤ 3
        assert_eq!(g.get_pixel(13, 12)[0], 0); // distance √13 > 3
        assert_eq!(g.get_pixel(14, 10)[0], 0);
        // A disk of radius 3 has 29 lattice points.
        assert_eq!(count(&g, |v| v == 255), 29);

        let s = shrink(&g, 3).unwrap();
        assert_eq!(selected(&s), vec![(10, 10)]);
        assert!(shrink(&m, 1).is_none());
    }

    #[test]
    fn shrink_does_not_erode_from_canvas_edge() {
        let all = select_all(10, 10);
        assert_eq!(shrink(&all, 3).unwrap(), all);
        let left = rect_mask(10, 10, (0.0, 0.0), (5.0, 10.0));
        let s = shrink(&left, 2).unwrap();
        // Columns 0..=2 stay selected (only the inner edge at x=5 erodes).
        assert_eq!(s.get_pixel(0, 0)[0], 255);
        assert_eq!(s.get_pixel(2, 9)[0], 255);
        assert_eq!(s.get_pixel(3, 5)[0], 0);
    }

    #[test]
    fn grow_preserves_soft_values() {
        let m = GrayImage::from_raw(5, 1, vec![0, 0, 100, 0, 0]).unwrap();
        assert_eq!(grow(&m, 1).unwrap().into_raw(), vec![0, 100, 100, 100, 0]);
    }

    #[test]
    fn feather_produces_a_soft_monotonic_edge() {
        let r = 4;
        let m = rect_mask(60, 10, (0.0, 0.0), (30.0, 10.0));
        let f = feather(&m, r).unwrap();
        let row: Vec<u8> = (0..60).map(|x| f.get_pixel(x, 5)[0]).collect();
        for pair in row.windows(2) {
            assert!(pair[0] >= pair[1], "monotonic: {row:?}");
        }
        assert!(
            row.iter().any(|v| *v > 0 && *v < 255),
            "soft values present"
        );
        assert_eq!(
            row[30 - 2 * r as usize - 1],
            255,
            "255 deeper than 2r inside"
        );
        assert_eq!(row[30 + 2 * r as usize], 0, "0 farther than 2r outside");
        assert!((row[29] as i32 - 128).abs() < 40 && (row[30] as i32 - 128).abs() < 40);
        // Edge clamping: the selection touching the canvas edge stays solid there.
        assert_eq!(f.get_pixel(0, 0)[0], 255);
    }

    #[test]
    fn outline_of_rectangle_and_hole() {
        let m = rect_mask(10, 10, (2.0, 3.0), (5.0, 6.0)); // pixels x 2..=4, y 3..=5
        let mut segs = outline(&m);
        segs.sort();
        assert_eq!(
            segs,
            vec![
                ((2, 3), (2, 6)),
                ((2, 3), (5, 3)),
                ((2, 6), (5, 6)),
                ((5, 3), (5, 6)),
            ]
        );
        // The document edge is a boundary; soft values split at 128.
        assert_eq!(outline(&select_all(2, 2)).len(), 4);
        let soft = GrayImage::from_raw(3, 1, vec![127, 128, 255]).unwrap();
        let segs = outline(&soft);
        assert!(segs.contains(&((1, 0), (1, 1))));
        assert!(!segs.contains(&((0, 0), (0, 1))));
        // A ring has an outer and an inner boundary.
        let ring = combine(
            Some(&select_all(5, 5)),
            &rect_mask(5, 5, (2.0, 2.0), (3.0, 3.0)),
            CombineMode::Subtract,
        )
        .unwrap();
        assert_eq!(outline(&ring).len(), 8);
    }

    #[test]
    fn large_operations_are_bounded_to_the_selection() {
        let m = rect_mask(4000, 4000, (100.0, 100.0), (140.0, 140.0));
        let start = std::time::Instant::now();
        let g = grow(&m, 20).unwrap();
        let f = feather(&g, 20).unwrap();
        let s = shrink(&f, 10).unwrap();
        assert!(bounds(&s).unwrap().right() < 200);
        // Region-limited work stays fast even in debug builds.
        assert!(start.elapsed().as_secs_f32() < 5.0);
    }
}
