//! The asset-extraction engine: background keying, object detection,
//! segmentation (MobileSAM or GrabCut), and tight transparent crops.

pub mod detect;
pub mod grabcut;
pub mod keying;
mod maxflow;
pub mod models;
pub mod sam;
pub mod segment;

use image::{GrayImage, RgbaImage};

use crate::document::PixelRect;

/// Multiplies alpha by `mask` coverage (both image-sized) and crops to the
/// non-transparent pixels. `None` when nothing remains.
pub fn tight_crop(pixels: &RgbaImage, mask: &GrayImage) -> Option<(RgbaImage, PixelRect)> {
    tight_crop_region(
        pixels,
        mask,
        PixelRect::new(0, 0, mask.width(), mask.height()),
    )
}

/// Like [`tight_crop`] with `mask` covering only `region` of the image
/// (coverage outside it is 0).
pub fn tight_crop_region(
    pixels: &RgbaImage,
    mask: &GrayImage,
    region: PixelRect,
) -> Option<(RgbaImage, PixelRect)> {
    let mut masked = RgbaImage::new(region.width, region.height);
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, out) in masked.enumerate_pixels_mut() {
        let (ix, iy) = (region.x + x, region.y + y);
        if ix >= pixels.width() || iy >= pixels.height() {
            continue;
        }
        let mut p = *pixels.get_pixel(ix, iy);
        let coverage = mask.get_pixel(x, y)[0] as u32;
        p[3] = ((p[3] as u32 * coverage + 127) / 255) as u8;
        if p[3] == 0 {
            continue;
        }
        *out = p;
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + 1);
        y1 = y1.max(y + 1);
    }
    if x0 == u32::MAX {
        return None;
    }
    let image = image::imageops::crop_imm(&masked, x0, y0, x1 - x0, y1 - y0).to_image();
    Some((
        image,
        PixelRect::new(region.x + x0, region.y + y0, x1 - x0, y1 - y0),
    ))
}

/// Keeps only the 8-connected region of `mask` (coverage ≥ 128) containing
/// one of `seeds`, and fills enclosed holes smaller than `max_hole` pixels.
/// Without seeds inside the mask, keeps the largest region.
pub fn clean_mask(mask: &GrayImage, seeds: &[(u32, u32)], hole_fraction: f32) -> GrayImage {
    let (w, h) = mask.dimensions();
    let on = |x: u32, y: u32| mask.get_pixel(x, y)[0] >= 128;
    let labels = detect::label(w, h, on, true);
    let mut keep: Vec<u32> = seeds
        .iter()
        .filter(|&&(x, y)| x < w && y < h && on(x, y))
        .map(|&(x, y)| labels.label[(y * w + x) as usize])
        .collect();
    if keep.is_empty() {
        if let Some((best, _)) = labels
            .sizes
            .iter()
            .enumerate()
            .skip(1)
            .max_by_key(|(_, s)| **s)
        {
            keep.push(best as u32);
        }
    }
    let mut out = GrayImage::new(w, h);
    let mut area = 0u32;
    for (i, &l) in labels.label.iter().enumerate() {
        if l != 0 && keep.contains(&l) {
            out.as_mut()[i] = 255;
            area += 1;
        }
    }
    // Fill background regions not connected to the border (holes).
    let max_hole = (area as f32 * hole_fraction) as u32;
    let holes = detect::label(w, h, |x, y| out.get_pixel(x, y)[0] == 0, false);
    let mut touches_border = vec![false; holes.sizes.len()];
    for x in 0..w {
        touches_border[holes.label[x as usize] as usize] = true;
        touches_border[holes.label[((h - 1) * w + x) as usize] as usize] = true;
    }
    for y in 0..h {
        touches_border[holes.label[(y * w) as usize] as usize] = true;
        touches_border[holes.label[(y * w + w - 1) as usize] as usize] = true;
    }
    for (i, &l) in holes.label.iter().enumerate() {
        if l != 0 && !touches_border[l as usize] && holes.sizes[l as usize] <= max_hole {
            out.as_mut()[i] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Luma, Rgba};

    #[test]
    fn tight_crop_multiplies_alpha_and_crops() {
        let pixels = RgbaImage::from_pixel(10, 8, Rgba([10, 20, 30, 200]));
        let mut mask = GrayImage::new(10, 8);
        mask.put_pixel(3, 2, Luma([255]));
        mask.put_pixel(6, 4, Luma([128]));
        let (img, rect) = tight_crop(&pixels, &mask).unwrap();
        assert_eq!(rect, PixelRect::new(3, 2, 4, 3));
        assert_eq!(img.get_pixel(0, 0).0, [10, 20, 30, 200]);
        assert_eq!(img.get_pixel(3, 2)[3], 100);
        assert_eq!(img.get_pixel(1, 1)[3], 0, "outside the mask is transparent");
        assert!(tight_crop(&pixels, &GrayImage::new(10, 8)).is_none());
    }

    #[test]
    fn clean_mask_keeps_seeded_region_and_fills_small_holes() {
        let mut mask = GrayImage::new(40, 40);
        for y in 5..25 {
            for x in 5..25 {
                mask.put_pixel(x, y, Luma([255]));
            }
        }
        mask.put_pixel(15, 15, Luma([0])); // pinhole
        for y in 30..35 {
            for x in 30..35 {
                mask.put_pixel(x, y, Luma([255])); // island
            }
        }
        let clean = clean_mask(&mask, &[(10, 10)], 0.005);
        assert_eq!(clean.get_pixel(15, 15)[0], 255, "hole filled");
        assert_eq!(clean.get_pixel(32, 32)[0], 0, "island dropped");
        let island = clean_mask(&mask, &[(32, 32)], 0.005);
        assert_eq!(island.get_pixel(10, 10)[0], 0);
        assert_eq!(island.get_pixel(32, 32)[0], 255);
    }
}
