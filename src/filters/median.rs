//! Median (denoise), per channel over a square window.

use super::{Category, Filter, ParamKind, ParamSpec, Params};
use image::RgbaImage;

pub struct Median;

/// Median of each RGBA channel over a `(2r+1)²` window (edge clamped),
/// using per-row sliding histograms that track the median incrementally (Huang).
fn median_rgba(src: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    let half = ((2 * r + 1) * (2 * r + 1) / 2) as u32;
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(16);
    let per = h.div_ceil(threads).max(1);
    let clampx = |x: isize| x.clamp(0, w as isize - 1) as usize;
    std::thread::scope(|scope| {
        for (chunk_index, chunk) in out.chunks_mut(per * w * 4).enumerate() {
            scope.spawn(move || {
                for (i, row) in chunk.chunks_mut(w * 4).enumerate() {
                    let y = chunk_index * per + i;
                    // Pixel offsets of the window's rows.
                    let offsets: Vec<usize> = (-(r as isize)..=(r as isize))
                        .map(|dy| (y as isize + dy).clamp(0, h as isize - 1) as usize * w)
                        .collect();
                    let mut hist = [[0u32; 256]; 4];
                    for &off in &offsets {
                        for dx in -(r as isize)..=(r as isize) {
                            let base = (off + clampx(dx)) * 4;
                            for c in 0..4 {
                                hist[c][src[base + c] as usize] += 1;
                            }
                        }
                    }
                    let mut m = [0usize; 4];
                    let mut below = [0u32; 4];
                    for c in 0..4 {
                        while below[c] + hist[c][m[c]] <= half {
                            below[c] += hist[c][m[c]];
                            m[c] += 1;
                        }
                    }
                    for x in 0..w {
                        for c in 0..4 {
                            row[x * 4 + c] = m[c] as u8;
                        }
                        if x + 1 == w {
                            break;
                        }
                        let out_x = clampx(x as isize - r as isize);
                        let in_x = clampx(x as isize + r as isize + 1);
                        if out_x != in_x {
                            for &off in &offsets {
                                let a_base = (off + out_x) * 4;
                                let b_base = (off + in_x) * 4;
                                for c in 0..4 {
                                    let a = src[a_base + c] as usize;
                                    let b = src[b_base + c] as usize;
                                    hist[c][a] -= 1;
                                    hist[c][b] += 1;
                                    below[c] = below[c] - (a < m[c]) as u32 + (b < m[c]) as u32;
                                }
                            }
                        }
                        for c in 0..4 {
                            while below[c] > half {
                                m[c] -= 1;
                                below[c] -= hist[c][m[c]];
                            }
                            while below[c] + hist[c][m[c]] <= half {
                                below[c] += hist[c][m[c]];
                                m[c] += 1;
                            }
                        }
                    }
                }
            });
        }
    });
    out
}

impl Filter for Median {
    fn id(&self) -> &'static str {
        "median"
    }
    fn name(&self) -> &'static str {
        "Median"
    }
    fn category(&self) -> Category {
        Category::Noise
    }
    fn params(&self) -> Vec<ParamSpec> {
        vec![ParamSpec::new(
            "radius",
            "Radius (px)",
            ParamKind::Int {
                min: 1,
                max: 20,
                default: 1,
            },
        )]
    }
    fn margin(&self, params: &Params) -> u32 {
        params.int("radius") as u32
    }
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage {
        let (w, h) = (source.width() as usize, source.height() as usize);
        let r = params.int("radius").clamp(1, 20) as usize;
        let data = median_rgba(source.as_raw(), w, h, r);
        RgbaImage::from_raw(w as u32, h as u32, data).expect("same size")
    }
}

#[cfg(test)]
mod tests {
    use super::super::ParamValue;
    use super::*;
    use image::Rgba;

    fn brute(img: &RgbaImage, r: i64) -> RgbaImage {
        let (w, h) = (img.width() as i64, img.height() as i64);
        RgbaImage::from_fn(img.width(), img.height(), |x, y| {
            let mut px = [0u8; 4];
            for (c, slot) in px.iter_mut().enumerate() {
                let mut v: Vec<u8> = Vec::new();
                for dy in -r..=r {
                    for dx in -r..=r {
                        let sx = (x as i64 + dx).clamp(0, w - 1) as u32;
                        let sy = (y as i64 + dy).clamp(0, h - 1) as u32;
                        v.push(img.get_pixel(sx, sy)[c]);
                    }
                }
                v.sort();
                *slot = v[v.len() / 2];
            }
            Rgba(px)
        })
    }

    #[test]
    fn removes_specks_and_matches_brute_force() {
        let mut img = RgbaImage::from_pixel(9, 9, Rgba([50, 60, 70, 255]));
        img.put_pixel(4, 4, Rgba([255, 255, 255, 255]));
        let p = Params::defaults(&Median.params());
        assert_eq!(Median.apply(&img, &p).get_pixel(4, 4).0, [50, 60, 70, 255]);
        let noisy = RgbaImage::from_fn(23, 17, |x, y| {
            Rgba([
                ((x * 37 + y * 11) % 256) as u8,
                ((x * x + y) % 256) as u8,
                ((y * 53) % 256) as u8,
                ((x * 7) % 256) as u8,
            ])
        });
        for r in [1, 3] {
            let mut q = p.clone();
            q.set("radius", ParamValue::Int(r));
            assert_eq!(Median.apply(&noisy, &q), brute(&noisy, r), "radius {r}");
        }
    }
}
