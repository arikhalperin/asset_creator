//! Clean Up: stray alpha removal (despeckle) and cropping to the content.

use image::RgbaImage;

use crate::document::{Document, PixelRect};

/// Alpha below this is treated as stray and cleared.
pub const MIN_ALPHA: u8 = 8;
/// Specks smaller than this many pixels (or 0.1 % of the content, whichever
/// is larger) are removed.
pub const MIN_SPECK: usize = 4;

/// Clears faint alpha and removes small 8-connected specks of
/// non-transparent pixels. Cleared pixels become fully transparent black.
pub fn despeckle(img: &RgbaImage) -> RgbaImage {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut out = img.clone();
    for p in out.pixels_mut() {
        if p[3] < MIN_ALPHA {
            p.0 = [0, 0, 0, 0];
        }
    }
    let solid: Vec<bool> = out.pixels().map(|p| p[3] > 0).collect();
    let total = solid.iter().filter(|s| **s).count();
    let min = MIN_SPECK.max((total as f64 * 0.001).ceil() as usize);
    let mut label = vec![usize::MAX; w * h];
    let mut stack = Vec::new();
    let mut members = Vec::new();
    for start in 0..w * h {
        if !solid[start] || label[start] != usize::MAX {
            continue;
        }
        members.clear();
        label[start] = start;
        stack.push(start);
        while let Some(i) = stack.pop() {
            members.push(i);
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if solid[j] && label[j] == usize::MAX {
                        label[j] = start;
                        stack.push(j);
                    }
                }
            }
        }
        if members.len() < min {
            for &i in &members {
                out.get_pixel_mut((i % w) as u32, (i / w) as u32).0 = [0, 0, 0, 0];
            }
        }
    }
    out
}

/// Bounding box of the non-transparent pixels of all `frames` (same size).
pub fn content_bounds(frames: &[&RgbaImage]) -> Option<PixelRect> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for img in frames {
        for (x, y, p) in img.enumerate_pixels() {
            if p[3] > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    (x1 > x0).then(|| PixelRect::new(x0, y0, x1 - x0, y1 - y0))
}

/// Despeckles every frame and crops all of them to their common content.
/// `None` when nothing would remain.
pub fn clean_frames(frames: &[RgbaImage]) -> Option<Vec<RgbaImage>> {
    let clean: Vec<RgbaImage> = frames.iter().map(despeckle).collect();
    let refs: Vec<&RgbaImage> = clean.iter().collect();
    let r = content_bounds(&refs)?;
    Some(
        clean
            .iter()
            .map(|f| image::imageops::crop_imm(f, r.x, r.y, r.width, r.height).to_image())
            .collect(),
    )
}

/// Image ▸ Clean Up: the active layer despeckled, then the canvas cropped to
/// the visible content. `None` when the document would not change or nothing
/// visible would remain.
pub fn clean_document(doc: &Document) -> Option<Document> {
    let mut out = doc.clone();
    let active = out.active_layer();
    if let Some(layer) = out.layer_mut(active) {
        layer.pixels = despeckle(&layer.pixels);
    }
    let composite = out.composite();
    let r = content_bounds(&[&composite])?;
    let out = out.cropped(r)?;
    let same = out.width() == doc.width()
        && out.height() == doc.height()
        && out
            .layers()
            .iter()
            .zip(doc.layers())
            .all(|(a, b)| a.pixels == b.pixels);
    (!same).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn sprite() -> RgbaImage {
        let mut img = RgbaImage::new(40, 30);
        for y in 8..20 {
            for x in 10..25 {
                img.put_pixel(x, y, Rgba([200, 50, 50, 255]));
            }
        }
        img
    }

    #[test]
    fn specks_and_faint_alpha_go_and_the_crop_fits_the_content() {
        let mut img = sprite();
        img.put_pixel(2, 2, Rgba([9, 9, 9, 255])); // lone speck
        img.put_pixel(35, 25, Rgba([9, 9, 9, 255]));
        img.put_pixel(36, 25, Rgba([9, 9, 9, 255])); // 2-px speck
        img.put_pixel(0, 29, Rgba([1, 2, 3, 5])); // faint
        let clean = clean_frames(&[img]).unwrap();
        assert_eq!(clean[0].dimensions(), (15, 12));
        assert!(clean[0].pixels().all(|p| p[3] == 255));
        // Already clean: unchanged.
        assert_eq!(clean_frames(&clean).unwrap(), clean);
    }

    #[test]
    fn diagonal_neighbours_count_as_connected_and_big_parts_stay() {
        let mut img = RgbaImage::new(20, 20);
        for i in 0..6 {
            img.put_pixel(2 + i, 2 + i, Rgba([255, 255, 255, 255])); // diagonal line
        }
        img.put_pixel(15, 15, Rgba([255, 255, 255, 255]));
        let d = despeckle(&img);
        assert_eq!(d.get_pixel(4, 4)[3], 255, "6-px diagonal line kept");
        assert_eq!(d.get_pixel(15, 15)[3], 0, "lone pixel removed");
    }

    #[test]
    fn animation_frames_share_one_crop() {
        let a = sprite();
        let mut b = RgbaImage::new(40, 30);
        for y in 5..10 {
            for x in 20..30 {
                b.put_pixel(x, y, Rgba([0, 0, 255, 255]));
            }
        }
        let out = clean_frames(&[a, b]).unwrap();
        assert_eq!(out[0].dimensions(), (20, 15));
        assert_eq!(out[1].dimensions(), (20, 15));
    }

    #[test]
    fn empty_images_have_nothing_to_keep() {
        assert!(clean_frames(&[RgbaImage::new(5, 5)]).is_none());
    }
}
