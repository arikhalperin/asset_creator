//! Invert colors.

use super::{map_pixels, Category, Filter, Params};
use image::{Rgba, RgbaImage};

pub struct Invert;

impl Filter for Invert {
    fn id(&self) -> &'static str {
        "invert"
    }
    fn name(&self) -> &'static str {
        "Invert"
    }
    fn category(&self) -> Category {
        Category::Adjustments
    }
    fn apply(&self, source: &RgbaImage, _params: &Params) -> RgbaImage {
        map_pixels(source, |p| Rgba([255 - p[0], 255 - p[1], 255 - p[2], p[3]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_twice_is_identity() {
        let img = RgbaImage::from_fn(8, 8, |x, y| {
            Rgba([(x * 30) as u8, (y * 30) as u8, 9, (x * y) as u8])
        });
        let once = Invert.apply(&img, &Params::default());
        assert_eq!(once.get_pixel(1, 0).0, [225, 255, 246, 0]);
        assert_eq!(Invert.apply(&once, &Params::default()), img);
    }
}
