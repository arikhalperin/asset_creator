//! Color Balance by tone range.

use super::{luminance, map_pixels, rgb01, to_u8, Category, Filter, ParamKind, ParamSpec, Params};
use image::{Rgba, RgbaImage};

pub struct ColorBalance;

const TONES: &[&str] = &["Shadows", "Midtones", "Highlights"];
const KEYS: [[&str; 3]; 3] = [
    [
        "shadows.cyan_red",
        "shadows.magenta_green",
        "shadows.yellow_blue",
    ],
    [
        "midtones.cyan_red",
        "midtones.magenta_green",
        "midtones.yellow_blue",
    ],
    [
        "highlights.cyan_red",
        "highlights.magenta_green",
        "highlights.yellow_blue",
    ],
];
const LABELS: [&str; 3] = ["Cyan – Red", "Magenta – Green", "Yellow – Blue"];

impl Filter for ColorBalance {
    fn id(&self) -> &'static str {
        "color_balance"
    }
    fn name(&self) -> &'static str {
        "Color Balance"
    }
    fn category(&self) -> Category {
        Category::Adjustments
    }
    fn params(&self) -> Vec<ParamSpec> {
        let mut out = vec![ParamSpec::new(
            "tone",
            "Tone",
            ParamKind::Choice {
                options: TONES,
                default: 1,
            },
        )];
        for (t, keys) in KEYS.iter().enumerate() {
            for (k, key) in keys.iter().enumerate() {
                out.push(
                    ParamSpec::new(
                        key,
                        LABELS[k],
                        ParamKind::Int {
                            min: -100,
                            max: 100,
                            default: 0,
                        },
                    )
                    .in_group("tone", t),
                );
            }
        }
        out.push(ParamSpec::new(
            "preserve_luminosity",
            "Preserve Luminosity",
            ParamKind::Bool { default: true },
        ));
        out
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        let sliders: [[f32; 3]; 3] = KEYS.map(|keys| keys.map(|k| params.int(k) as f32 / 100.0));
        if sliders.iter().flatten().all(|v| *v == 0.0) {
            return source.clone();
        }
        let preserve = params.flag("preserve_luminosity");
        map_pixels(source, |p| {
            let rgb = rgb01(p);
            let l = luminance(rgb);
            let weights = [
                (1.0 - 2.0 * l).max(0.0),
                1.0 - (2.0 * l - 1.0).abs(),
                (2.0 * l - 1.0).max(0.0),
            ];
            let mut out = rgb;
            for c in 0..3 {
                let shift: f32 = (0..3).map(|t| weights[t] * sliders[t][c] * 0.5).sum();
                out[c] = (rgb[c] + shift).clamp(0.0, 1.0);
            }
            if preserve {
                let d = l - luminance(out);
                for v in &mut out {
                    *v = (*v + d).clamp(0.0, 1.0);
                }
            }
            Rgba([to_u8(out[0]), to_u8(out[1]), to_u8(out[2]), p[3]])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::ParamValue;
    use super::*;

    #[test]
    fn tones_and_preserve_luminosity() {
        let gray = RgbaImage::from_pixel(1, 1, Rgba([128, 128, 128, 255]));
        let f = ColorBalance;
        let mut p = Params::defaults(&f.params());
        assert_eq!(f.apply(&gray, &p), gray);
        p.set("midtones.cyan_red", ParamValue::Int(100));
        p.set("preserve_luminosity", ParamValue::Bool(false));
        let out = f.apply(&gray, &p).get_pixel(0, 0).0;
        assert!(out[0] > 128 && out[1] == 128 && out[2] == 128, "{out:?}");
        p.set("preserve_luminosity", ParamValue::Bool(true));
        let kept = f.apply(&gray, &p).get_pixel(0, 0).0;
        let lum = |q: [u8; 4]| 0.299 * q[0] as f32 + 0.587 * q[1] as f32 + 0.114 * q[2] as f32;
        assert!((lum(kept) - 128.0).abs() < 1.5 && kept[0] > kept[1]);
        // Shadow sliders do not touch white.
        let white = RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255]));
        let mut s = Params::defaults(&f.params());
        s.set("shadows.yellow_blue", ParamValue::Int(100));
        assert_eq!(f.apply(&white, &s), white);
    }
}
