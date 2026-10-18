//! Scene-change detection by color-histogram difference.

use image::RgbaImage;

/// Longest side of the downscaled frame the histogram is built from.
pub const HISTOGRAM_SIDE: u32 = 64;
/// Bins per channel.
pub const BINS: usize = 16;

/// Normalized 16-bin R, G, B histograms (each channel sums to 1).
pub type Histogram = [[f32; BINS]; 3];

/// Box-averages `image` down so its longer side is at most
/// [`HISTOGRAM_SIDE`]; returns RGB triples.
pub fn downscale(image: &RgbaImage) -> Vec<[u8; 3]> {
    let (w, h) = image.dimensions();
    let block = w.max(h).div_ceil(HISTOGRAM_SIDE).max(1);
    let (sw, sh) = (w.div_ceil(block), h.div_ceil(block));
    let mut out = Vec::with_capacity((sw * sh) as usize);
    for by in 0..sh {
        for bx in 0..sw {
            let (x0, y0) = (bx * block, by * block);
            let (x1, y1) = ((x0 + block).min(w), (y0 + block).min(h));
            let mut sum = [0u32; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = image.get_pixel(x, y);
                    for c in 0..3 {
                        sum[c] += p[c] as u32;
                    }
                }
            }
            let n = (x1 - x0) * (y1 - y0);
            out.push(sum.map(|s| ((s + n / 2) / n) as u8));
        }
    }
    out
}

pub fn histogram(image: &RgbaImage) -> Histogram {
    let pixels = downscale(image);
    let mut hist = [[0f32; BINS]; 3];
    for p in &pixels {
        for c in 0..3 {
            hist[c][p[c] as usize * BINS / 256] += 1.0;
        }
    }
    let n = pixels.len().max(1) as f32;
    for channel in &mut hist {
        for bin in channel.iter_mut() {
            *bin /= n;
        }
    }
    hist
}

/// Normalized L1 difference in [0, 1]: `(1/6) Σ |a − b|` over the 48 bins.
pub fn difference(a: &Histogram, b: &Histogram) -> f32 {
    let mut sum = 0.0;
    for c in 0..3 {
        for i in 0..BINS {
            sum += (a[c][i] - b[c][i]).abs();
        }
    }
    sum / 6.0
}

/// Incremental cut detector over frames From…To in order.
#[derive(Debug, Clone)]
pub struct SceneDetector {
    /// Difference (0…1) at or above which a cut is placed.
    pub threshold: f32,
    /// Frames from the previous cut (or From) before another cut is allowed.
    pub min_scene: u64,
    previous: Option<Histogram>,
    last_cut: Option<u64>,
}

impl SceneDetector {
    pub fn new(threshold: f32, min_scene: u64) -> Self {
        Self {
            threshold,
            min_scene: min_scene.max(1),
            previous: None,
            last_cut: None,
        }
    }

    /// Feeds frame `index`; returns whether it starts a new scene. The first
    /// frame fed (From) always does.
    pub fn push(&mut self, index: u64, image: &RgbaImage) -> bool {
        let hist = histogram(image);
        let previous = self.previous.replace(hist);
        let Some(previous) = previous else {
            self.last_cut = Some(index);
            return true;
        };
        let far_enough = self
            .last_cut
            .is_none_or(|cut| index >= cut + self.min_scene);
        if far_enough && difference(&previous, &hist) >= self.threshold {
            self.last_cut = Some(index);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn flat(c: [u8; 3]) -> RgbaImage {
        RgbaImage::from_pixel(100, 50, Rgba([c[0], c[1], c[2], 255]))
    }

    #[test]
    fn identical_frames_differ_by_zero_and_disjoint_by_one() {
        let a = histogram(&flat([0, 0, 0]));
        let b = histogram(&flat([255, 255, 255]));
        assert_eq!(difference(&a, &a), 0.0);
        assert!((difference(&a, &b) - 1.0).abs() < 1e-6);
        // Only red moves: one channel of three.
        let c = histogram(&flat([255, 0, 0]));
        assert!((difference(&a, &c) - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn downscale_keeps_longer_side_at_most_64() {
        let img = RgbaImage::new(1920, 1080);
        let small = downscale(&img);
        // block = 30 → 64 × 36.
        assert_eq!(small.len(), 64 * 36);
        assert_eq!(downscale(&RgbaImage::new(10, 10)).len(), 100);
    }

    #[test]
    fn detector_respects_threshold_and_minimum_length() {
        let frames: Vec<RgbaImage> = (0..30)
            .map(|i| match i {
                0..10 => flat([0, 0, 0]),
                10..12 => flat([255, 255, 255]),
                12..20 => flat([0, 0, 0]),
                _ => flat([255, 255, 255]),
            })
            .collect();
        let mut d = SceneDetector::new(0.3, 5);
        let cuts: Vec<u64> = (0..30u64)
            .filter(|&i| d.push(i, &frames[i as usize]))
            .collect();
        // 12 is too close to the cut at 10.
        assert_eq!(cuts, vec![0, 10, 20]);
        let mut d = SceneDetector::new(0.3, 1);
        let cuts: Vec<u64> = (0..30u64)
            .filter(|&i| d.push(i, &frames[i as usize]))
            .collect();
        assert_eq!(cuts, vec![0, 10, 12, 20]);
    }
}
