//! Background keying for sprites on flat or near-flat backgrounds.

use std::collections::{HashMap, VecDeque};

use image::{Rgba, RgbaImage};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyParams {
    pub color: [u8; 3],
    /// 0…1: distance up to which a pixel is background.
    pub tolerance: f32,
    /// 0…1: width of the soft edge above the tolerance.
    pub softness: f32,
    /// Only key background connected to the image border.
    pub contiguous: bool,
}

impl KeyParams {
    pub const DEFAULT_TOLERANCE: f32 = 0.10;
    pub const DEFAULT_SOFTNESS: f32 = 0.10;

    /// Defaults with the image's dominant border color.
    pub fn auto(image: &RgbaImage) -> Self {
        Self {
            color: dominant_border_color(image),
            tolerance: Self::DEFAULT_TOLERANCE,
            softness: Self::DEFAULT_SOFTNESS,
            contiguous: true,
        }
    }
}

/// The most frequent border color (5 bits per channel), refined to the mean
/// of the border pixels in that bin. Transparent pixels are ignored.
pub fn dominant_border_color(image: &RgbaImage) -> [u8; 3] {
    let (w, h) = image.dimensions();
    let mut bins: HashMap<u16, (u32, [u32; 3])> = HashMap::new();
    let mut visit = |x: u32, y: u32| {
        let p = image.get_pixel(x, y);
        if p[3] == 0 {
            return;
        }
        let key = ((p[0] as u16 >> 3) << 10) | ((p[1] as u16 >> 3) << 5) | (p[2] as u16 >> 3);
        let entry = bins.entry(key).or_insert((0, [0; 3]));
        entry.0 += 1;
        for c in 0..3 {
            entry.1[c] += p[c] as u32;
        }
    };
    if w == 0 || h == 0 {
        return [0, 0, 0];
    }
    for x in 0..w {
        visit(x, 0);
        if h > 1 {
            visit(x, h - 1);
        }
    }
    for y in 1..h.saturating_sub(1) {
        visit(0, y);
        if w > 1 {
            visit(w - 1, y);
        }
    }
    bins.into_iter()
        .max_by_key(|(key, (n, _))| (*n, std::cmp::Reverse(*key)))
        .map(|(_, (n, sum))| sum.map(|s| ((s + n / 2) / n) as u8))
        .unwrap_or([0, 0, 0])
}

/// Largest per-channel difference from `bg`, 0…1.
fn distance(p: Rgba<u8>, bg: [u8; 3]) -> f32 {
    (0..3)
        .map(|c| (p[c] as i32 - bg[c] as i32).unsigned_abs())
        .max()
        .unwrap_or(0) as f32
        / 255.0
}

/// Keys the background out of `image`.
pub fn key(image: &RgbaImage, params: &KeyParams) -> RgbaImage {
    let (w, h) = image.dimensions();
    let edge = params.tolerance + params.softness;
    let candidate = |p: Rgba<u8>| {
        p[3] == 0
            || distance(p, params.color) < edge
            || params.softness == 0.0 && distance(p, params.color) <= params.tolerance
    };
    // Pixels allowed to change.
    let region: Vec<bool> = if params.contiguous {
        let mut seen = vec![false; (w * h) as usize];
        let mut queue = VecDeque::new();
        let seed = |x: u32, y: u32, seen: &mut Vec<bool>, queue: &mut VecDeque<(u32, u32)>| {
            let i = (y * w + x) as usize;
            if !seen[i] && candidate(*image.get_pixel(x, y)) {
                seen[i] = true;
                queue.push_back((x, y));
            }
        };
        for x in 0..w {
            seed(x, 0, &mut seen, &mut queue);
            seed(x, h.saturating_sub(1), &mut seen, &mut queue);
        }
        for y in 0..h {
            seed(0, y, &mut seen, &mut queue);
            seed(w.saturating_sub(1), y, &mut seen, &mut queue);
        }
        while let Some((x, y)) = queue.pop_front() {
            if x > 0 {
                seed(x - 1, y, &mut seen, &mut queue);
            }
            if x + 1 < w {
                seed(x + 1, y, &mut seen, &mut queue);
            }
            if y > 0 {
                seed(x, y - 1, &mut seen, &mut queue);
            }
            if y + 1 < h {
                seed(x, y + 1, &mut seen, &mut queue);
            }
        }
        seen
    } else {
        Vec::new()
    };
    let bg = params.color.map(|c| c as f32);
    let mut out = image.clone();
    for (i, p) in out.pixels_mut().enumerate() {
        if params.contiguous && !region[i] {
            continue;
        }
        let d = distance(*p, params.color);
        let alpha = if d <= params.tolerance {
            0.0
        } else if d >= edge || params.softness == 0.0 {
            continue;
        } else {
            (d - params.tolerance) / params.softness
        };
        if alpha <= 0.0 {
            *p = Rgba([0, 0, 0, 0]);
            continue;
        }
        for c in 0..3 {
            let v = (p[c] as f32 - (1.0 - alpha) * bg[c]) / alpha;
            p[c] = v.round().clamp(0.0, 255.0) as u8;
        }
        p[3] = (p[3] as f32 * alpha).round() as u8;
    }
    out
}

/// Remove Background as a (hidden) filter, so it shares the filter preview,
/// selection blending, and undo machinery.
pub struct RemoveBackground;

/// Parameter keys of [`RemoveBackground`].
pub mod param {
    pub const RED: &str = "red";
    pub const GREEN: &str = "green";
    pub const BLUE: &str = "blue";
    pub const TOLERANCE: &str = "tolerance";
    pub const SOFTNESS: &str = "softness";
    pub const CONTIGUOUS: &str = "contiguous";
}

impl RemoveBackground {
    pub fn params_for(key: &KeyParams) -> crate::filters::Params {
        use crate::filters::{ParamValue, Params};
        let mut p = Params::defaults(&crate::filters::Filter::params(&RemoveBackground));
        p.set(param::RED, ParamValue::Int(key.color[0] as i64));
        p.set(param::GREEN, ParamValue::Int(key.color[1] as i64));
        p.set(param::BLUE, ParamValue::Int(key.color[2] as i64));
        p.set(
            param::TOLERANCE,
            ParamValue::Int((key.tolerance * 100.0).round() as i64),
        );
        p.set(
            param::SOFTNESS,
            ParamValue::Int((key.softness * 100.0).round() as i64),
        );
        p.set(param::CONTIGUOUS, ParamValue::Bool(key.contiguous));
        p
    }

    pub fn key_params(params: &crate::filters::Params) -> KeyParams {
        KeyParams {
            color: [
                params.int(param::RED) as u8,
                params.int(param::GREEN) as u8,
                params.int(param::BLUE) as u8,
            ],
            tolerance: params.int(param::TOLERANCE) as f32 / 100.0,
            softness: params.int(param::SOFTNESS) as f32 / 100.0,
            contiguous: params.flag(param::CONTIGUOUS),
        }
    }
}

impl crate::filters::Filter for RemoveBackground {
    fn id(&self) -> &'static str {
        "remove_background"
    }
    fn name(&self) -> &'static str {
        "Remove Background"
    }
    fn category(&self) -> crate::filters::Category {
        crate::filters::Category::Adjustments
    }
    fn params(&self) -> Vec<crate::filters::ParamSpec> {
        use crate::filters::{ParamKind, ParamSpec};
        let int = |key, label, max, default| {
            ParamSpec::new(
                key,
                label,
                ParamKind::Int {
                    min: 0,
                    max,
                    default,
                },
            )
        };
        vec![
            int(param::RED, "Red", 255, 0),
            int(param::GREEN, "Green", 255, 0),
            int(param::BLUE, "Blue", 255, 0),
            int(param::TOLERANCE, "Tolerance", 100, 10),
            int(param::SOFTNESS, "Edge Softness", 100, 10),
            ParamSpec::new(
                param::CONTIGUOUS,
                "Contiguous",
                ParamKind::Bool { default: true },
            ),
        ]
    }
    /// The whole layer is context: the border color and contiguity are
    /// judged from the image border, whatever is selected.
    fn margin(&self, _params: &crate::filters::Params) -> u32 {
        1 << 20
    }
    fn apply(&self, source: &RgbaImage, params: &crate::filters::Params) -> RgbaImage {
        key(source, &Self::key_params(params))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite_on(bg: [u8; 3]) -> RgbaImage {
        RgbaImage::from_fn(20, 20, |x, y| {
            if (5..15).contains(&x) && (5..15).contains(&y) {
                if (8..12).contains(&x) && (8..12).contains(&y) {
                    Rgba([bg[0], bg[1], bg[2], 255]) // enclosed bg-colored hole
                } else {
                    Rgba([250, 200, 10, 255])
                }
            } else if x == 4 && (5..15).contains(&y) {
                // anti-aliased edge: 15 % sprite over bg
                let mix = |s: u32, b: u8| ((s * 15 + b as u32 * 85) / 100) as u8;
                Rgba([mix(250, bg[0]), mix(200, bg[1]), mix(10, bg[2]), 255])
            } else {
                Rgba([bg[0], bg[1], bg[2], 255])
            }
        })
    }

    #[test]
    fn dominant_border_color_is_the_background() {
        assert_eq!(dominant_border_color(&sprite_on([0, 0, 255])), [0, 0, 255]);
    }

    #[test]
    fn keying_clears_background_keeps_interior_and_softens_edges() {
        let img = sprite_on([0, 0, 255]);
        let keyed = key(&img, &KeyParams::auto(&img));
        assert_eq!(keyed.get_pixel(0, 0)[3], 0);
        assert_eq!(
            keyed.get_pixel(6, 6).0,
            [250, 200, 10, 255],
            "interior unchanged"
        );
        assert_eq!(
            keyed.get_pixel(9, 9)[3],
            255,
            "contiguous keeps enclosed holes"
        );
        let edge = keyed.get_pixel(4, 8);
        assert!(edge[3] > 0 && edge[3] < 255, "{edge:?}");
        assert!(edge[2] < img.get_pixel(4, 8)[2], "blue removed: {edge:?}");
        let mut all = KeyParams::auto(&img);
        all.contiguous = false;
        assert_eq!(
            key(&img, &all).get_pixel(9, 9)[3],
            0,
            "non-contiguous keys holes too"
        );
    }
}
