//! Desaturate (luminance gray).

use super::{luminance, map_pixels, rgb01, to_u8, Category, Filter, Params};
use image::{Rgba, RgbaImage};

pub struct Desaturate;

impl Filter for Desaturate {
    fn id(&self) -> &'static str {
        "desaturate"
    }
    fn name(&self) -> &'static str {
        "Desaturate"
    }
    fn category(&self) -> Category {
        Category::Adjustments
    }
    fn apply(&self, source: &RgbaImage, _params: &Params) -> RgbaImage {
        map_pixels(source, |p| {
            let l = to_u8(luminance(rgb01(p)));
            Rgba([l, l, l, p[3]])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luminance_gray() {
        let img = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 10]));
        assert_eq!(
            Desaturate.apply(&img, &Params::default()).get_pixel(0, 0).0,
            [76, 76, 76, 10]
        );
    }
}
