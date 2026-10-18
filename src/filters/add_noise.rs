//! Add Noise (seeded, deterministic).

use super::{splitmix64, Category, Filter, ParamKind, ParamSpec, Params};
use image::RgbaImage;

pub struct AddNoise;

/// Uniform value in [0, 1) from a hash.
fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Noise sample in roughly [−0.5, 0.5] for pixel `index` and `channel`.
fn sample(seed: u64, index: u64, channel: u64, gaussian: bool) -> f32 {
    let h = splitmix64(seed ^ splitmix64(index.wrapping_mul(4).wrapping_add(channel)));
    if gaussian {
        // Box–Muller with σ = 0.25.
        let u1 = unit(h).max(1e-7);
        let u2 = unit(splitmix64(h));
        0.25 * (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
    } else {
        unit(h) - 0.5
    }
}

impl Filter for AddNoise {
    fn id(&self) -> &'static str {
        "add_noise"
    }
    fn name(&self) -> &'static str {
        "Add Noise"
    }
    fn category(&self) -> Category {
        Category::Noise
    }
    fn params(&self) -> Vec<ParamSpec> {
        vec![
            ParamSpec::new(
                "amount",
                "Amount (%)",
                ParamKind::Int {
                    min: 0,
                    max: 100,
                    default: 10,
                },
            ),
            ParamSpec::new(
                "distribution",
                "Distribution",
                ParamKind::Choice {
                    options: &["Uniform", "Gaussian"],
                    default: 0,
                },
            ),
            ParamSpec::new(
                "monochromatic",
                "Monochromatic",
                ParamKind::Bool { default: false },
            ),
            ParamSpec::new("seed", "Seed", ParamKind::Seed),
        ]
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        let amount = params.int("amount") as f32 / 100.0 * 255.0;
        let gaussian = params.choice("distribution") == 1;
        let mono = params.flag("monochromatic");
        let seed = params.seed("seed");
        let w = source.width() as usize;
        // Absolute image position, so a pixel gets the same noise whatever area is filtered.
        let (ox, oy) = (
            params.int(super::ORIGIN_X) as u64,
            params.int(super::ORIGIN_Y) as u64,
        );
        let image_w = (params.int(super::IMAGE_WIDTH) as u64).max(w as u64);
        // Offsets are keyed by pixel position, so threading does not change results.
        let mut out = source.clone();
        crate::transform::parallel_rows(&mut out, w * 4, 0..source.height(), |y, row| {
            for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let index = (oy + y as u64) * image_w + ox + x as u64;
                for c in 0..3u64 {
                    let n = sample(seed, index, if mono { 0 } else { c }, gaussian);
                    px[c as usize] = (px[c as usize] as f32 + amount * n)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::super::ParamValue;
    use super::*;
    use image::Rgba;

    #[test]
    fn seeded_and_bounded() {
        let img = RgbaImage::from_pixel(32, 32, Rgba([128, 128, 128, 200]));
        let mut p = Params::defaults(&AddNoise.params());
        p.set("seed", ParamValue::Seed(42));
        let a = AddNoise.apply(&img, &p);
        assert_eq!(a, AddNoise.apply(&img, &p), "same seed, same noise");
        p.set("seed", ParamValue::Seed(43));
        assert_ne!(a, AddNoise.apply(&img, &p), "different seed differs");
        assert!(a.pixels().all(|q| q[3] == 200 && q[0].abs_diff(128) <= 13));
        assert!(a.pixels().any(|q| q[0] != 128));
        p.set("monochromatic", ParamValue::Bool(true));
        let m = AddNoise.apply(&img, &p);
        assert!(m.pixels().all(|q| q[0] == q[1] && q[1] == q[2]));
        p.set("amount", ParamValue::Int(0));
        assert_eq!(AddNoise.apply(&img, &p), img);
    }
}
