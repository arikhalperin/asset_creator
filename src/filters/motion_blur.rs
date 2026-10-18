//! Motion Blur along a line.

use super::{convolve_taps, Category, Filter, ParamKind, ParamSpec, Params, Premul};
use image::RgbaImage;

pub struct MotionBlur;

/// Integer-offset taps of the bilinear line kernel (weights sum to 1).
pub(crate) fn motion_taps(angle_deg: f32, distance: f32) -> Vec<(isize, isize, f32)> {
    let n = distance.round().max(1.0) as usize;
    let (s, c) = angle_deg.to_radians().sin_cos();
    let mut taps: std::collections::BTreeMap<(isize, isize), f32> = Default::default();
    let w = 1.0 / n as f32;
    for i in 0..n {
        let t = i as f32 - (n as f32 - 1.0) / 2.0;
        // Screen y points down: positive angles go counter-clockwise visually.
        let (ox, oy) = (t * c, -t * s);
        let (fx, fy) = (ox.floor(), oy.floor());
        let (ax, ay) = (ox - fx, oy - fy);
        for (dx, dy, wt) in [
            (0, 0, (1.0 - ax) * (1.0 - ay)),
            (1, 0, ax * (1.0 - ay)),
            (0, 1, (1.0 - ax) * ay),
            (1, 1, ax * ay),
        ] {
            if wt > 0.0 {
                *taps
                    .entry((fx as isize + dx, fy as isize + dy))
                    .or_default() += wt * w;
            }
        }
    }
    taps.into_iter().map(|((x, y), wt)| (x, y, wt)).collect()
}

impl Filter for MotionBlur {
    fn id(&self) -> &'static str {
        "motion_blur"
    }
    fn name(&self) -> &'static str {
        "Motion Blur"
    }
    fn category(&self) -> Category {
        Category::Blur
    }
    fn params(&self) -> Vec<ParamSpec> {
        vec![
            ParamSpec::new(
                "angle",
                "Angle (°)",
                ParamKind::Int {
                    min: -180,
                    max: 180,
                    default: 0,
                },
            ),
            ParamSpec::new(
                "distance",
                "Distance (px)",
                ParamKind::Int {
                    min: 1,
                    max: 200,
                    default: 10,
                },
            ),
        ]
    }
    fn margin(&self, params: &Params) -> u32 {
        (params.int("distance") as u32).div_ceil(2) + 2
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        let taps = motion_taps(params.int("angle") as f32, params.int("distance") as f32);
        convolve_taps(&Premul::from_image(source), &taps).to_image()
    }
}

#[cfg(test)]
mod tests {
    use super::super::ParamValue;
    use super::*;
    use image::Rgba;

    #[test]
    fn horizontal_blur_spreads_along_x_only() {
        let taps = motion_taps(0.0, 5.0);
        let total: f32 = taps.iter().map(|t| t.2).sum();
        assert!((total - 1.0).abs() < 1e-5);
        assert!(taps.iter().all(|t| t.1 == 0));
        let mut img = RgbaImage::from_pixel(21, 21, Rgba([0, 0, 0, 255]));
        img.put_pixel(10, 10, Rgba([255, 255, 255, 255]));
        let mut p = Params::defaults(&MotionBlur.params());
        p.set("distance", ParamValue::Int(5));
        let out = MotionBlur.apply(&img, &p);
        assert_eq!(out.get_pixel(12, 10)[0], 51);
        assert_eq!(out.get_pixel(10, 12)[0], 0);
        p.set("angle", ParamValue::Int(90));
        let v = MotionBlur.apply(&img, &p);
        assert_eq!(v.get_pixel(10, 12)[0], 51);
        assert_eq!(v.get_pixel(12, 10)[0], 0);
    }
}
