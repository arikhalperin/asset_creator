//! Posterize.

use super::{apply_luts, Category, Filter, ParamKind, ParamSpec, Params};
use image::RgbaImage;

pub struct Posterize;

impl Filter for Posterize {
    fn id(&self) -> &'static str {
        "posterize"
    }
    fn name(&self) -> &'static str {
        "Posterize"
    }
    fn category(&self) -> Category {
        Category::Adjustments
    }
    fn params(&self) -> Vec<ParamSpec> {
        vec![ParamSpec::new(
            "levels",
            "Levels",
            ParamKind::Int {
                min: 2,
                max: 255,
                default: 4,
            },
        )]
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        let n = params.int("levels").clamp(2, 255) as f32;
        let mut lut = [0u8; 256];
        for (i, v) in lut.iter_mut().enumerate() {
            let step = (i as f32 / 255.0 * (n - 1.0)).round();
            *v = (step / (n - 1.0) * 255.0).round() as u8;
        }
        apply_luts(source, &[lut, lut, lut])
    }
}

#[cfg(test)]
mod tests {
    use super::super::ParamValue;
    use super::*;
    use image::Rgba;

    #[test]
    fn two_levels_are_black_and_white() {
        let img = RgbaImage::from_fn(256, 1, |x, _| Rgba([x as u8, x as u8, x as u8, 255]));
        let mut p = Params::defaults(&Posterize.params());
        p.set("levels", ParamValue::Int(2));
        let out = Posterize.apply(&img, &p);
        assert!(out.pixels().all(|q| q[0] == 0 || q[0] == 255));
        for n in [3u8, 4, 17] {
            p.set("levels", ParamValue::Int(n as i64));
            let mut values: Vec<u8> = Posterize.apply(&img, &p).pixels().map(|q| q[0]).collect();
            values.dedup();
            assert_eq!(values.len(), n as usize, "{n} levels");
            assert_eq!((values[0], *values.last().unwrap()), (0, 255));
        }
    }
}
