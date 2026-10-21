//! Finding the printed model parts on a rendered page: ink on the paper
//! colour, grouped so a part's outline, fold lines, glue tabs, and printed
//! number stay one part.

use image::{GrayImage, Luma, Rgba, RgbaImage};

use super::mm_to_px;
use crate::document::PixelRect;

/// Gaps up to this wide (mm) are bridged when grouping ink into parts.
pub const CLOSING_MM: f32 = 1.5;
/// Ink regions smaller than this (mm²) are dropped as specks.
pub const MIN_AREA_MM2: f32 = 4.0;
/// A pixel is ink when a channel differs from the paper colour by more.
const INK_THRESHOLD: i32 = 30;

/// One part: where it is on its page and which pixels belong to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub page: usize,
    /// Bounding box on the page (pixels at `DPI`).
    pub rect: PixelRect,
    /// `rect`-sized: 255 where the part is.
    pub mask: GrayImage,
}

impl Part {
    /// The part's pixels cut from `page` (transparent outside the mask).
    pub fn image(&self, page: &RgbaImage) -> RgbaImage {
        let r = self.rect;
        RgbaImage::from_fn(r.width, r.height, |x, y| {
            if self.mask.get_pixel(x, y)[0] > 0 {
                *page.get_pixel(r.x + x, r.y + y)
            } else {
                Rgba([0, 0, 0, 0])
            }
        })
    }

    /// Union of parts on the same page (Merge).
    pub fn merge(parts: &[&Part]) -> Option<Part> {
        let first = parts.first()?;
        let x0 = parts.iter().map(|p| p.rect.x).min()?;
        let y0 = parts.iter().map(|p| p.rect.y).min()?;
        let x1 = parts.iter().map(|p| p.rect.right()).max()?;
        let y1 = parts.iter().map(|p| p.rect.bottom()).max()?;
        let mut mask = GrayImage::new(x1 - x0, y1 - y0);
        for p in parts {
            for (x, y, m) in p.mask.enumerate_pixels() {
                if m[0] > 0 {
                    mask.put_pixel(p.rect.x - x0 + x, p.rect.y - y0 + y, Luma([255]));
                }
            }
        }
        Some(Part {
            page: first.page,
            rect: PixelRect::new(x0, y0, x1 - x0, y1 - y0),
            mask,
        })
    }

    /// A part from a page-sized mask (e.g. MobileSAM's); `None` if empty.
    pub fn from_page_mask(page: usize, mask: &GrayImage) -> Option<Part> {
        let rect = crate::selection::bounds(mask)?;
        let cut =
            image::imageops::crop_imm(mask, rect.x, rect.y, rect.width, rect.height).to_image();
        Some(Part {
            page,
            rect,
            mask: cut,
        })
    }

    /// Size in millimetres.
    pub fn size_mm(&self) -> (f32, f32) {
        (
            super::px_to_mm(self.rect.width as f32),
            super::px_to_mm(self.rect.height as f32),
        )
    }
}

/// The paper colour: the most common (quantized) colour of a sparse sample.
pub fn paper_color(page: &RgbaImage) -> [u8; 3] {
    let mut counts = std::collections::HashMap::<[u8; 3], u32>::new();
    let (w, h) = page.dimensions();
    for y in (0..h).step_by(4) {
        for x in (0..w).step_by(4) {
            let p = page.get_pixel(x, y);
            *counts
                .entry([p[0] & 0xF8, p[1] & 0xF8, p[2] & 0xF8])
                .or_default() += 1;
        }
    }
    let q = counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(c, _)| c)
        .unwrap_or([248, 248, 248]);
    [q[0] | 4, q[1] | 4, q[2] | 4]
}

/// Square dilation of a binary mask by `r` pixels (separable running counts).
fn dilate(mask: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let mut rows = vec![false; w * h];
    for y in 0..h {
        let row = &mask[y * w..(y + 1) * w];
        let mut count = 0usize;
        // Window [x - r, x + r].
        count += row[..r.min(w)].iter().filter(|b| **b).count();
        for x in 0..w {
            if x + r < w {
                count += row[x + r] as usize;
            }
            if x > r {
                count -= row[x - r - 1] as usize;
            }
            rows[y * w + x] = count > 0;
        }
    }
    let mut out = vec![false; w * h];
    for x in 0..w {
        let mut count = 0usize;
        for y in 0..(r.min(h)) {
            count += rows[y * w + x] as usize;
        }
        for y in 0..h {
            if y + r < h {
                count += rows[(y + r) * w + x] as usize;
            }
            if y > r {
                count -= rows[(y - r - 1) * w + x] as usize;
            }
            out[y * w + x] = count > 0;
        }
    }
    out
}

/// Finds the parts on a rendered page (`index` is the page number).
pub fn detect(page: &RgbaImage, index: usize) -> Vec<Part> {
    let (w, h) = (page.width() as usize, page.height() as usize);
    let paper = paper_color(page);
    let ink: Vec<bool> = page
        .pixels()
        .map(|p| (0..3).any(|c| (p[c] as i32 - paper[c] as i32).abs() > INK_THRESHOLD) && p[3] > 0)
        .collect();
    let r = (mm_to_px(CLOSING_MM) / 2.0).ceil() as usize;
    let grown = dilate(&ink, w, h, r);
    // Closing: grow, then shrink back (bridges gaps up to CLOSING_MM).
    let outside: Vec<bool> = grown.iter().map(|g| !g).collect();
    let closed: Vec<bool> = dilate(&outside, w, h, r).iter().map(|o| !o).collect();
    // Group by the grown mask; each part keeps its closed (or ink) pixels.
    let mut label = vec![u32::MAX; w * h];
    let mut stack = Vec::new();
    let min_area = (MIN_AREA_MM2 * mm_to_px(1.0) * mm_to_px(1.0)) as usize;
    let mut parts = Vec::new();
    let mut next = 0u32;
    for start in 0..w * h {
        if !grown[start] || label[start] != u32::MAX {
            continue;
        }
        let id = next;
        next += 1;
        label[start] = id;
        stack.push(start);
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        let mut area = 0usize;
        let mut members = Vec::new();
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            if closed[i] || ink[i] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
                members.push(i);
                area += ink[i] as usize;
            }
            if x > 0 && grown[i - 1] && label[i - 1] == u32::MAX {
                label[i - 1] = id;
                stack.push(i - 1);
            }
            if x + 1 < w && grown[i + 1] && label[i + 1] == u32::MAX {
                label[i + 1] = id;
                stack.push(i + 1);
            }
            if y > 0 && grown[i - w] && label[i - w] == u32::MAX {
                label[i - w] = id;
                stack.push(i - w);
            }
            if y + 1 < h && grown[i + w] && label[i + w] == u32::MAX {
                label[i + w] = id;
                stack.push(i + w);
            }
        }
        if area < min_area || members.is_empty() {
            continue;
        }
        let rect = PixelRect::new(
            x0 as u32,
            y0 as u32,
            (x1 - x0 + 1) as u32,
            (y1 - y0 + 1) as u32,
        );
        let mut mask = GrayImage::new(rect.width, rect.height);
        for i in members {
            mask.put_pixel((i % w - x0) as u32, (i / w - y0) as u32, Luma([255]));
        }
        fill_holes(&mut mask);
        parts.push(Part {
            page: index,
            rect,
            mask,
        });
    }
    let mut parts = absorb_enclosed(parts);
    parts.sort_by_key(|p| (p.rect.y / 20, p.rect.x));
    parts
}

/// Sets every pixel enclosed by the mask (not reachable from outside its
/// bounding box through unset pixels): an unfilled part owns its inside.
pub fn fill_holes(mask: &mut GrayImage) {
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    let mut outside = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();
    let open = |m: &GrayImage, i: usize| m.get_pixel((i % w) as u32, (i / w) as u32)[0] == 0;
    for x in 0..w {
        stack.push(x);
        stack.push((h - 1) * w + x);
    }
    for y in 0..h {
        stack.push(y * w);
        stack.push(y * w + w - 1);
    }
    while let Some(i) = stack.pop() {
        if outside[i] || !open(mask, i) {
            continue;
        }
        outside[i] = true;
        let (x, y) = (i % w, i / w);
        if x > 0 {
            stack.push(i - 1);
        }
        if x + 1 < w {
            stack.push(i + 1);
        }
        if y > 0 {
            stack.push(i - w);
        }
        if y + 1 < h {
            stack.push(i + w);
        }
    }
    for (i, out) in outside.iter().enumerate() {
        if !out {
            mask.put_pixel((i % w) as u32, (i / w) as u32, Luma([255]));
        }
    }
}

/// Merges proposals that lie (90 % or more of their pixels) inside a larger
/// part's mask — e.g. a number printed inside an unfilled part.
fn absorb_enclosed(mut parts: Vec<Part>) -> Vec<Part> {
    parts.sort_by_key(|p| std::cmp::Reverse(p.rect.width as u64 * p.rect.height as u64));
    let mut out: Vec<Part> = Vec::new();
    'next: for part in parts {
        for host in out.iter_mut() {
            let (hr, pr) = (host.rect, part.rect);
            if pr.x < hr.x || pr.y < hr.y || pr.right() > hr.right() || pr.bottom() > hr.bottom() {
                continue;
            }
            let (mut total, mut inside) = (0usize, 0usize);
            for (x, y, m) in part.mask.enumerate_pixels() {
                if m[0] > 0 {
                    total += 1;
                    let hx = pr.x + x - hr.x;
                    let hy = pr.y + y - hr.y;
                    inside += (host.mask.get_pixel(hx, hy)[0] > 0) as usize;
                }
            }
            if total > 0 && inside * 10 >= total * 9 {
                for (x, y, m) in part.mask.enumerate_pixels() {
                    if m[0] > 0 {
                        host.mask
                            .put_pixel(pr.x + x - hr.x, pr.y + y - hr.y, Luma([255]));
                    }
                }
                continue 'next;
            }
        }
        out.push(part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet() -> RgbaImage {
        let mut img = RgbaImage::from_pixel(600, 400, Rgba([255, 255, 255, 255]));
        // A filled panel with a white tab joined by its outline.
        for y in 50..150 {
            for x in 50..250 {
                let edge = y == 50 || y == 149 || x == 50 || x == 249;
                img.put_pixel(
                    x,
                    y,
                    if edge {
                        Rgba([0, 0, 0, 255])
                    } else {
                        Rgba([150, 160, 170, 255])
                    },
                );
            }
        }
        for x in 80..120 {
            img.put_pixel(x, 40, Rgba([0, 0, 0, 255]));
        }
        for y in 40..50 {
            img.put_pixel(80, y, Rgba([0, 0, 0, 255]));
            img.put_pixel(119, y, Rgba([0, 0, 0, 255]));
        }
        // A second part far away, and a speck.
        for y in 250..300 {
            for x in 400..500 {
                img.put_pixel(x, y, Rgba([90, 120, 60, 255]));
            }
        }
        img.put_pixel(550, 50, Rgba([0, 0, 0, 255]));
        img
    }

    #[test]
    fn parts_keep_their_tabs_and_specks_are_dropped() {
        let parts = detect(&sheet(), 0);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].rect, PixelRect::new(50, 40, 200, 110));
        assert_eq!(parts[1].rect, PixelRect::new(400, 250, 100, 50));
        let img = parts[0].image(&sheet());
        assert_eq!(img.get_pixel(0, 10)[3], 255);
        assert_eq!(img.get_pixel(5, 2)[3], 0, "outside the tab is transparent");
    }

    #[test]
    fn an_unfilled_part_owns_its_inside_and_the_label_in_it() {
        let mut img = RgbaImage::from_pixel(700, 500, Rgba([255, 255, 255, 255]));
        // A white part: only a 2-px outline, 400 × 300.
        for y in 50..350 {
            for x in 50..450 {
                let edge = !(52..348).contains(&y) || !(52..448).contains(&x);
                if edge {
                    img.put_pixel(x, y, Rgba([0, 0, 0, 255]));
                }
            }
        }
        // A label in the middle, far (> 1.5 mm) from the outline.
        for y in 190..215 {
            for x in 230..290 {
                if (x + y) % 3 == 0 {
                    img.put_pixel(x, y, Rgba([0, 0, 0, 255]));
                }
            }
        }
        let parts = detect(&img, 0);
        assert_eq!(parts.len(), 1, "the label belongs to the part");
        let p = &parts[0];
        assert_eq!(p.rect, PixelRect::new(50, 50, 400, 300));
        assert_eq!(p.mask.get_pixel(200, 100)[0], 255, "inside is part of it");
        let cut = p.image(&img);
        assert_eq!(
            cut.get_pixel(100, 100).0,
            [255, 255, 255, 255],
            "paper inside kept"
        );
        // The label (page 250, 200) is in the cut-out.
        assert_eq!(cut.get_pixel(200, 150).0, [0, 0, 0, 255]);
    }

    #[test]
    fn merge_unions_masks() {
        let parts = detect(&sheet(), 0);
        let m = Part::merge(&[&parts[0], &parts[1]]).unwrap();
        assert_eq!(m.rect, PixelRect::new(50, 40, 450, 260));
        assert_eq!(m.mask.get_pixel(400 - 50, 250 - 40)[0], 255);
    }
}
