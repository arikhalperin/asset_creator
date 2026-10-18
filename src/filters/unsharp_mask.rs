//! Unsharp Mask.

use super::{
    gaussian, gaussian_margin, unpremultiply, Category, Filter, ParamKind, ParamSpec, Params,
    Premul,
};
use image::{Rgba, RgbaImage};

pub struct UnsharpMask;

impl Filter for UnsharpMask {
    fn id(&self) -> &'static str {
        "unsharp_mask"
    }
    fn name(&self) -> &'static str {
        "Unsharp Mask"
    }
    fn category(&self) -> Category {
        Category::Sharpen
    }
    fn params(&self) -> Vec<ParamSpec> {
        vec![
            ParamSpec::new(
                "amount",
                "Amount (%)",
                ParamKind::Int {
                    min: 1,
                    max: 500,
                    default: 100,
                },
            ),
            ParamSpec::new(
                "radius",
                "Radius (px)",
                ParamKind::Float {
                    min: 0.1,
                    max: 250.0,
                    default: 1.0,
                    step: 0.1,
                },
            ),
            ParamSpec::new(
                "threshold",
                "Threshold",
                ParamKind::Int {
                    min: 0,
                    max: 255,
                    default: 0,
                },
            ),
        ]
    }
    fn margin(&self, params: &Params) -> u32 {
        gaussian_margin(params.float("radius"))
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        let amount = params.int("amount") as f32 / 100.0;
        let threshold = params.int("threshold") as f32;
        let blurred = gaussian(&Premul::from_image(source), params.float("radius"));
        RgbaImage::from_fn(source.width(), source.height(), |x, y| {
            let p = *source.get_pixel(x, y);
            let b = unpremultiply(blurred.data[(y * source.width() + x) as usize]);
            let mut out = p;
            for c in 0..3 {
                let d = p[c] as f32 - b[c] as f32;
                if d.abs() >= threshold {
                    out[c] = (p[c] as f32 + amount * d).round().clamp(0.0, 255.0) as u8;
                }
            }
            Rgba([out[0], out[1], out[2], p[3]])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::ParamValue;
    use super::*;

    #[test]
    fn enhances_edges_and_respects_threshold() {
        let flat = RgbaImage::from_pixel(9, 9, Rgba([70, 70, 70, 255]));
        assert_eq!(
            UnsharpMask.apply(&flat, &Params::defaults(&UnsharpMask.params())),
            flat
        );
        let step = RgbaImage::from_fn(12, 1, |x, _| {
            if x < 6 {
                Rgba([100, 100, 100, 255])
            } else {
                Rgba([150, 150, 150, 255])
            }
        });
        let p = Params::defaults(&UnsharpMask.params());
        let out = UnsharpMask.apply(&step, &p);
        assert!(out.get_pixel(5, 0)[0] < 100 && out.get_pixel(6, 0)[0] > 150);
        let mut high = p.clone();
        high.set("threshold", ParamValue::Int(255));
        assert_eq!(UnsharpMask.apply(&step, &high), step);
    }
}
