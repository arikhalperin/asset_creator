//! Hue/Saturation/Lightness in HSL.

use super::{map_pixels, rgb01, to_u8, Category, Filter, ParamKind, ParamSpec, Params};
use image::{Rgba, RgbaImage};

pub struct HueSaturation;

pub(crate) fn rgb_to_hsl([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if max == min {
        return [0.0, 0.0, l];
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [h * 60.0, s, l]
}

pub(crate) fn hsl_to_rgb([h, s, l]: [f32; 3]) -> [f32; 3] {
    if s == 0.0 {
        return [l, l, l];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let h = h.rem_euclid(360.0) / 360.0;
    let channel = |mut t: f32| {
        t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
}

impl Filter for HueSaturation {
    fn id(&self) -> &'static str {
        "hue_saturation"
    }
    fn name(&self) -> &'static str {
        "Hue/Saturation/Lightness"
    }
    fn category(&self) -> Category {
        Category::Adjustments
    }
    fn params(&self) -> Vec<ParamSpec> {
        vec![
            ParamSpec::new(
                "hue",
                "Hue",
                ParamKind::Int {
                    min: -180,
                    max: 180,
                    default: 0,
                },
            ),
            ParamSpec::new(
                "saturation",
                "Saturation",
                ParamKind::Int {
                    min: -100,
                    max: 100,
                    default: 0,
                },
            ),
            ParamSpec::new(
                "lightness",
                "Lightness",
                ParamKind::Int {
                    min: -100,
                    max: 100,
                    default: 0,
                },
            ),
        ]
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        let (dh, ds, dl) = (
            params.int("hue") as f32,
            params.int("saturation") as f32 / 100.0,
            params.int("lightness") as f32 / 100.0,
        );
        if dh == 0.0 && ds == 0.0 && dl == 0.0 {
            return source.clone();
        }
        map_pixels(source, |p| {
            let [h, s, l] = rgb_to_hsl(rgb01(p));
            let s = (s * (1.0 + ds)).clamp(0.0, 1.0);
            let l = l + dl * if dl > 0.0 { 1.0 - l } else { l };
            let [r, g, b] = hsl_to_rgb([h + dh, s, l]);
            Rgba([to_u8(r), to_u8(g), to_u8(b), p[3]])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::ParamValue;
    use super::*;

    #[test]
    fn hsl_round_trip_and_shifts() {
        for rgb in [
            [1.0, 0.0, 0.0],
            [0.2, 0.6, 0.4],
            [0.5, 0.5, 0.5],
            [0.9, 0.1, 0.7],
        ] {
            let back = hsl_to_rgb(rgb_to_hsl(rgb));
            for c in 0..3 {
                assert!((back[c] - rgb[c]).abs() < 1e-4, "{rgb:?} {back:?}");
            }
        }
        let red = RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 200]));
        let f = HueSaturation;
        let mut p = Params::defaults(&f.params());
        assert_eq!(f.apply(&red, &p), red);
        p.set("hue", ParamValue::Int(120));
        assert_eq!(f.apply(&red, &p).get_pixel(0, 0).0, [0, 255, 0, 200]);
        let mut p = Params::defaults(&f.params());
        p.set("saturation", ParamValue::Int(-100));
        assert_eq!(f.apply(&red, &p).get_pixel(0, 0).0, [128, 128, 128, 200]);
        let mut p = Params::defaults(&f.params());
        p.set("lightness", ParamValue::Int(100));
        assert_eq!(f.apply(&red, &p).get_pixel(0, 0).0, [255, 255, 255, 200]);
    }
}
