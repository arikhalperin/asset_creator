//! Sharpen (fixed 3×3 kernel).

use super::{convolve_taps, Category, Filter, Params, Premul};
use image::RgbaImage;

pub struct Sharpen;

impl Filter for Sharpen {
    fn id(&self) -> &'static str {
        "sharpen"
    }
    fn name(&self) -> &'static str {
        "Sharpen"
    }
    fn category(&self) -> Category {
        Category::Sharpen
    }
    fn margin(&self, _params: &Params) -> u32 {
        1
    }
    fn apply(&self, source: &RgbaImage, _params: &Params) -> RgbaImage {
        let taps = [
            (0, -1, -1.0),
            (-1, 0, -1.0),
            (0, 0, 5.0),
            (1, 0, -1.0),
            (0, 1, -1.0),
        ];
        let mut out = convolve_taps(&Premul::from_image(source), &taps);
        // Alpha is not sharpened: restore it (keeping colour per unit alpha).
        for (o, p) in out.data.iter_mut().zip(source.pixels()) {
            let a_new = o[3].clamp(1e-6, f32::MAX);
            let a = p[3] as f32;
            for v in &mut o[..3] {
                *v = (*v / a_new * 255.0).clamp(0.0, 255.0) * a / 255.0;
            }
            o[3] = a;
        }
        out.to_image()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn flat_unchanged_edges_enhanced() {
        let flat = RgbaImage::from_pixel(5, 5, Rgba([90, 90, 90, 255]));
        assert_eq!(Sharpen.apply(&flat, &Params::default()), flat);
        let step = RgbaImage::from_fn(6, 1, |x, _| {
            if x < 3 {
                Rgba([100, 100, 100, 255])
            } else {
                Rgba([150, 150, 150, 255])
            }
        });
        let out = Sharpen.apply(&step, &Params::default());
        assert!(out.get_pixel(2, 0)[0] < 100 && out.get_pixel(3, 0)[0] > 150);
    }
}
