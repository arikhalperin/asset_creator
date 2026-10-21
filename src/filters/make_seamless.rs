//! Make Seamless: the region becomes a tile that repeats without a seam.

use super::{Category, Filter, ParamKind, ParamSpec, Params};
use image::RgbaImage;

pub struct MakeSeamless;

impl Filter for MakeSeamless {
    fn id(&self) -> &'static str {
        "make_seamless"
    }
    fn name(&self) -> &'static str {
        "Make Seamless"
    }
    fn category(&self) -> Category {
        Category::Tile
    }
    fn params(&self) -> Vec<ParamSpec> {
        vec![ParamSpec::new(
            "blend",
            "Blend width (%)",
            ParamKind::Int {
                min: 5,
                max: 50,
                default: 25,
            },
        )]
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        crate::pattern::make_seamless(source, params.int("blend") as f32 / 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn result_is_seamless_and_the_same_size() {
        let img = RgbaImage::from_fn(50, 30, |x, y| Rgba([(x * 5) as u8, (y * 8) as u8, 7, 255]));
        let out = MakeSeamless.apply(&img, &Params::defaults(&MakeSeamless.params()));
        assert_eq!(out.dimensions(), (50, 30));
        assert!(crate::pattern::seam_ratio(&out) <= crate::pattern::SEAMLESS_RATIO);
    }
}
