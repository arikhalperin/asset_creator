//! Curves with RGB and per-channel curves (monotone cubic interpolation).

use super::{apply_luts, Category, Filter, ParamKind, ParamSpec, Params};
use image::RgbaImage;

pub struct Curves;

pub const IDENTITY_CURVE: [(f32, f32); 2] = [(0.0, 0.0), (255.0, 255.0)];
const KEYS: [&str; 4] = ["rgb.curve", "red.curve", "green.curve", "blue.curve"];

/// Sorts points by x, clamps to 0–255, merges equal x, and keeps endpoints at x = 0 and 255.
pub(crate) fn normalize(mut points: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    for p in &mut points {
        p.0 = p.0.clamp(0.0, 255.0);
        p.1 = p.1.clamp(0.0, 255.0);
    }
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    points.dedup_by(|a, b| (a.0 - b.0).abs() < 1.0);
    if points.first().is_none_or(|p| p.0 > 0.0) {
        points.insert(0, (0.0, 0.0));
    }
    if points.last().is_none_or(|p| p.0 < 255.0) {
        points.push((255.0, 255.0));
    }
    points
}

/// 256-entry lookup table of a curve (Fritsch–Carlson monotone cubic).
pub fn curve_lut(points: &[(f32, f32)]) -> [u8; 256] {
    let pts = normalize(points.to_vec());
    let n = pts.len();
    let mut lut = [0u8; 256];
    if n < 2 {
        for (i, v) in lut.iter_mut().enumerate() {
            *v = i as u8;
        }
        return lut;
    }
    let slopes: Vec<f32> = pts
        .windows(2)
        .map(|w| (w[1].1 - w[0].1) / (w[1].0 - w[0].0))
        .collect();
    let mut tangents = vec![0.0f32; n];
    tangents[0] = slopes[0];
    tangents[n - 1] = slopes[n - 2];
    for i in 1..n - 1 {
        tangents[i] = if slopes[i - 1] * slopes[i] <= 0.0 {
            0.0
        } else {
            (slopes[i - 1] + slopes[i]) / 2.0
        };
    }
    for i in 0..n - 1 {
        if slopes[i] == 0.0 {
            tangents[i] = 0.0;
            tangents[i + 1] = 0.0;
        } else {
            let a = tangents[i] / slopes[i];
            let b = tangents[i + 1] / slopes[i];
            let s = a * a + b * b;
            if s > 9.0 {
                let t = 3.0 / s.sqrt();
                tangents[i] = t * a * slopes[i];
                tangents[i + 1] = t * b * slopes[i];
            }
        }
    }
    for (x, v) in lut.iter_mut().enumerate() {
        let xf = x as f32;
        let i = pts.windows(2).position(|w| xf <= w[1].0).unwrap_or(n - 2);
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[i + 1];
        let h = x1 - x0;
        let t = ((xf - x0) / h).clamp(0.0, 1.0);
        let (t2, t3) = (t * t, t * t * t);
        let y = (2.0 * t3 - 3.0 * t2 + 1.0) * y0
            + (t3 - 2.0 * t2 + t) * h * tangents[i]
            + (-2.0 * t3 + 3.0 * t2) * y1
            + (t3 - t2) * h * tangents[i + 1];
        *v = y.round().clamp(0.0, 255.0) as u8;
    }
    lut
}

impl Filter for Curves {
    fn id(&self) -> &'static str {
        "curves"
    }
    fn name(&self) -> &'static str {
        "Curves"
    }
    fn category(&self) -> Category {
        Category::Adjustments
    }
    fn params(&self) -> Vec<ParamSpec> {
        let mut out = vec![ParamSpec::new(
            "channel",
            "Channel",
            ParamKind::Choice {
                options: super::levels::CHANNELS,
                default: 0,
            },
        )];
        for (c, key) in KEYS.iter().enumerate() {
            out.push(ParamSpec::new(key, "Curve", ParamKind::Curve).in_group("channel", c));
        }
        out
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        let master = curve_lut(&params.curve(KEYS[0]));
        let luts = [1, 2, 3].map(|c| {
            let own = curve_lut(&params.curve(KEYS[c]));
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
    fn identity_monotone_and_through_points() {
        let id = curve_lut(&IDENTITY_CURVE);
        assert!(id.iter().enumerate().all(|(i, v)| *v as usize == i));
        let lut = curve_lut(&[(0.0, 0.0), (64.0, 128.0), (255.0, 255.0)]);
        assert_eq!(lut[64], 128);
        assert!(lut.windows(2).all(|w| w[0] <= w[1]), "monotone");
        let img = RgbaImage::from_fn(256, 1, |x, _| Rgba([x as u8, x as u8, x as u8, 255]));
        let f = Curves;
        let mut p = Params::defaults(&f.params());
        assert_eq!(f.apply(&img, &p), img);
        p.set(
            "blue.curve",
            ParamValue::Curve(vec![(0.0, 255.0), (255.0, 0.0)]),
        );
        assert_eq!(f.apply(&img, &p).get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(
            normalize(vec![(300.0, 10.0), (50.0, 50.0)]),
            vec![(0.0, 0.0), (50.0, 50.0), (255.0, 10.0)]
        );
    }
}
