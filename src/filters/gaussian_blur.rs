//! Gaussian Blur.

use super::{gaussian, gaussian_margin, Category, Filter, ParamKind, ParamSpec, Params, Premul};
use image::RgbaImage;

pub struct GaussianBlur;

impl Filter for GaussianBlur {
    fn id(&self) -> &'static str {
        "gaussian_blur"
    }
    fn name(&self) -> &'static str {
        "Gaussian Blur"
    }
    fn category(&self) -> Category {
        Category::Blur
    }
    fn params(&self) -> Vec<ParamSpec> {
        vec![ParamSpec::new(
            "radius",
            "Radius (px)",
            ParamKind::Float {
                min: 0.1,
                max: 250.0,
                default: 2.0,
                step: 0.1,
            },
        )]
    }
    fn margin(&self, params: &Params) -> u32 {
        gaussian_margin(params.float("radius"))
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        gaussian(&Premul::from_image(source), params.float("radius")).to_image()
    }
}

#[cfg(test)]
mod tests {
    use super::super::ParamValue;
    use super::*;
    use image::Rgba;

    #[test]
    fn blur_spreads_and_conserves() {
        let mut img = RgbaImage::from_pixel(31, 1, Rgba([0, 0, 0, 255]));
        img.put_pixel(15, 0, Rgba([255, 255, 255, 255]));
        for radius in [1.0, 4.0] {
            let mut p = Params::defaults(&GaussianBlur.params());
            p.set("radius", ParamValue::Float(radius));
            let out = GaussianBlur.apply(&img, &p);
            let row: Vec<u8> = (0..31).map(|x| out.get_pixel(x, 0)[0]).collect();
            assert!(row[15] < 255 && row[14] > 0 && row[16] > 0, "{row:?}");
            assert_eq!(row[14], row[16], "symmetric");
            let total: u32 = row.iter().map(|v| *v as u32).sum();
            assert!((total as i32 - 255).abs() < 12, "energy {total}");
        }
        // Transparent neighbours do not darken colour (premultiplied).
        let mut t = RgbaImage::new(9, 1);
        t.put_pixel(4, 0, Rgba([200, 0, 0, 255]));
        let mut p = Params::defaults(&GaussianBlur.params());
        p.set("radius", ParamValue::Float(1.0));
        let out = GaussianBlur.apply(&t, &p);
        assert_eq!(out.get_pixel(3, 0)[0], 200);
        assert!(out.get_pixel(3, 0)[3] < 255);
    }
}
