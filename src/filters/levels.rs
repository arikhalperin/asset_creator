//! Levels with RGB and per-channel settings.

use super::{apply_luts, to_u8, Category, Filter, ParamKind, ParamSpec, Params};
use image::RgbaImage;

pub struct Levels;

pub(crate) const CHANNELS: &[&str] = &["RGB", "Red", "Green", "Blue"];
const PREFIX: [&str; 4] = ["rgb", "red", "green", "blue"];

fn key(channel: usize, field: &str) -> &'static str {
    // Keys are static strings so they can live in the schema.
    const KEYS: [[&str; 5]; 4] = [
        [
            "rgb.in_black",
            "rgb.in_white",
            "rgb.gamma",
            "rgb.out_black",
            "rgb.out_white",
        ],
        [
            "red.in_black",
            "red.in_white",
            "red.gamma",
            "red.out_black",
            "red.out_white",
        ],
        [
            "green.in_black",
            "green.in_white",
            "green.gamma",
            "green.out_black",
            "green.out_white",
        ],
        [
            "blue.in_black",
            "blue.in_white",
            "blue.gamma",
            "blue.out_black",
            "blue.out_white",
        ],
    ];
    let i = ["in_black", "in_white", "gamma", "out_black", "out_white"]
        .iter()
        .position(|f| *f == field)
        .expect("field");
    let _ = PREFIX;
    KEYS[channel][i]
}

/// Lookup table for one set of levels.
pub(crate) fn levels_lut(ib: f32, iw: f32, gamma: f32, ob: f32, ow: f32) -> [u8; 256] {
    let iw = iw.max(ib + 2.0);
    let mut lut = [0u8; 256];
    for (i, v) in lut.iter_mut().enumerate() {
        let t = ((i as f32 - ib) / (iw - ib))
            .clamp(0.0, 1.0)
            .powf(1.0 / gamma.max(0.01));
        *v = to_u8((ob + (ow - ob) * t) / 255.0);
    }
    lut
}

impl Filter for Levels {
    fn id(&self) -> &'static str {
        "levels"
    }
    fn name(&self) -> &'static str {
        "Levels"
    }
    fn category(&self) -> Category {
        Category::Adjustments
    }
    fn params(&self) -> Vec<ParamSpec> {
        let mut out = vec![ParamSpec::new(
            "channel",
            "Channel",
            ParamKind::Choice {
                options: CHANNELS,
                default: 0,
            },
        )];
        for c in 0..4 {
            out.extend(
                [
                    ParamSpec::new(
                        key(c, "in_black"),
                        "Input Black",
                        ParamKind::Int {
                            min: 0,
                            max: 253,
                            default: 0,
                        },
                    ),
                    ParamSpec::new(
                        key(c, "in_white"),
                        "Input White",
                        ParamKind::Int {
                            min: 2,
                            max: 255,
                            default: 255,
                        },
                    ),
                    ParamSpec::new(
                        key(c, "gamma"),
                        "Gamma",
                        ParamKind::Float {
                            min: 0.1,
                            max: 9.99,
                            default: 1.0,
                            step: 0.01,
                        },
                    ),
                    ParamSpec::new(
                        key(c, "out_black"),
                        "Output Black",
                        ParamKind::Int {
                            min: 0,
                            max: 255,
                            default: 0,
                        },
                    ),
                    ParamSpec::new(
                        key(c, "out_white"),
                        "Output White",
                        ParamKind::Int {
                            min: 0,
                            max: 255,
                            default: 255,
                        },
                    ),
                ]
                .map(|s| s.in_group("channel", c)),
            );
        }
        out
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        let lut_for = |c: usize| {
            levels_lut(
                params.int(key(c, "in_black")) as f32,
                params.int(key(c, "in_white")) as f32,
                params.float(key(c, "gamma")),
                params.int(key(c, "out_black")) as f32,
                params.int(key(c, "out_white")) as f32,
            )
        };
        let master = lut_for(0);
        let luts = [1, 2, 3].map(|c| {
            let own = lut_for(c);
            let mut lut = [0u8; 256];
            for (i, v) in lut.iter_mut().enumerate() {
                *v = master[own[i] as usize];
            }
            lut
        });
        apply_luts(source, &luts)
    }
}

#[cfg(test)]
mod tests {
    use super::super::ParamValue;
    use super::*;
    use image::Rgba;

    #[test]
    fn identity_and_known_values() {
        let img = RgbaImage::from_fn(256, 1, |x, _| Rgba([x as u8, 255 - x as u8, 128, 77]));
        let f = Levels;
        let mut p = Params::defaults(&f.params());
        assert_eq!(f.apply(&img, &p), img);
        p.set("rgb.in_black", ParamValue::Int(64));
        p.set("rgb.in_white", ParamValue::Int(192));
        let out = f.apply(&img, &p);
        assert_eq!(out.get_pixel(64, 0)[0], 0);
        assert_eq!(out.get_pixel(192, 0)[0], 255);
        assert_eq!(out.get_pixel(128, 0)[0], 128);
        // Per-channel: only red is inverted.
        let mut q = Params::defaults(&f.params());
        q.set("red.out_black", ParamValue::Int(255));
        q.set("red.out_white", ParamValue::Int(0));
        let out = f.apply(&img, &q);
        assert_eq!(out.get_pixel(10, 0).0, [245, 245, 128, 77]);
        // Gamma 2 brightens midtones.
        assert!(levels_lut(0.0, 255.0, 2.0, 0.0, 255.0)[64] > 64);
    }
}
