//! Brightness/Contrast.

use super::{apply_luts, to_u8, Category, Filter, ParamKind, ParamSpec, Params};
use image::RgbaImage;

pub struct BrightnessContrast;

/// The contrast factor `f` for `c` in −100…100.
pub(crate) fn contrast_factor(c: f32) -> f32 {
    if c < 0.0 {
        1.0 + c / 100.0
    } else {
        1.0 / (1.0 - 0.99 * c / 100.0)
    }
}

impl Filter for BrightnessContrast {
    fn id(&self) -> &'static str {
        "brightness_contrast"
    }
    fn name(&self) -> &'static str {
        "Brightness/Contrast"
    }
    fn category(&self) -> Category {
        Category::Adjustments
    }
    fn params(&self) -> Vec<ParamSpec> {
        vec![
            ParamSpec::new(
                "brightness",
                "Brightness",
                ParamKind::Int {
                    min: -100,
                    max: 100,
                    default: 0,
                },
            ),
            ParamSpec::new(
                "contrast",
                "Contrast",
                ParamKind::Int {
                    min: -100,
                    max: 100,
                    default: 0,
                },
            ),
        ]
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        let b = params.int("brightness") as f32;
        let f = contrast_factor(params.int("contrast") as f32);
        let mut lut = [0u8; 256];
        for (i, v) in lut.iter_mut().enumerate() {
            *v = to_u8((i as f32 / 255.0 - 0.5) * f + 0.5 + b / 200.0);
        }
        apply_luts(source, &[lut, lut, lut])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn defaults_change_nothing_and_extremes_behave() {
        let img = RgbaImage::from_fn(16, 1, |x, _| Rgba([(x * 16) as u8, 7, 250, 9]));
        let f = BrightnessContrast;
        assert_eq!(f.apply(&img, &Params::defaults(&f.params())), img);
        let mut p = Params::defaults(&f.params());
        p.set("contrast", super::super::ParamValue::Int(-100));
        assert!(
            f.apply(&img, &p).pixels().all(|q| q[0] == 128 && q[3] == 9),
            "flat gray, alpha kept"
        );
        let mut p = Params::defaults(&f.params());
        p.set("brightness", super::super::ParamValue::Int(100));
        assert_eq!(f.apply(&img, &p).get_pixel(0, 0)[0], 128);
        assert!((contrast_factor(100.0) - 100.0).abs() < 1e-3);
    }
}
