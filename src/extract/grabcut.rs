//! GrabCut segmentation (Rother et al. 2004): RGB Gaussian mixtures for the
//! object and background, contrast-sensitive smoothness, min-cut.

use image::{GrayImage, Luma, RgbaImage};

use super::maxflow::Graph;
use super::segment::Prompt;

const COMPONENTS: usize = 5;
const ITERATIONS: usize = 5;
const GAMMA: f32 = 50.0;
/// Longest side of the image GrabCut works on.
pub const WORK_SIDE: u32 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trimap {
    Bg,
    Fg,
    ProbBg,
    ProbFg,
}

#[derive(Clone, Copy)]
struct Gaussian {
    weight: f32,
    mean: [f32; 3],
    inverse: [[f32; 3]; 3],
    /// ln(weight) − ½ ln det(Σ) (the constant term).
    log_norm: f32,
}

struct Gmm {
    parts: Vec<Gaussian>,
}

impl Gmm {
    /// −ln p(z).
    fn cost(&self, z: [f32; 3]) -> f32 {
        let mut best = f32::NEG_INFINITY;
        let mut terms = [0f32; COMPONENTS];
        for (i, g) in self.parts.iter().enumerate() {
            let d = [z[0] - g.mean[0], z[1] - g.mean[1], z[2] - g.mean[2]];
            let mut m = 0.0;
            for r in 0..3 {
                for c in 0..3 {
                    m += d[r] * g.inverse[r][c] * d[c];
                }
            }
            terms[i] = g.log_norm - 0.5 * m;
            best = best.max(terms[i]);
        }
        let sum: f32 = terms[..self.parts.len()]
            .iter()
            .map(|t| (t - best).exp())
            .sum();
        -(best + sum.ln())
    }

    fn component(&self, z: [f32; 3]) -> usize {
        let mut best = (0, f32::NEG_INFINITY);
        for (i, g) in self.parts.iter().enumerate() {
            let d = [z[0] - g.mean[0], z[1] - g.mean[1], z[2] - g.mean[2]];
            let mut m = 0.0;
            for r in 0..3 {
                for c in 0..3 {
                    m += d[r] * g.inverse[r][c] * d[c];
                }
            }
            let t = g.log_norm - 0.5 * m;
            if t > best.1 {
                best = (i, t);
            }
        }
        best.0
    }

    /// Fits `COMPONENTS` Gaussians to `samples` given an assignment.
    #[allow(clippy::needless_range_loop)] // 3×3 matrix arithmetic
    fn fit(samples: &[[f32; 3]], assign: &[usize]) -> Gmm {
        let mut parts = Vec::new();
        let total = samples.len().max(1) as f32;
        for k in 0..COMPONENTS {
            let members: Vec<&[f32; 3]> = samples
                .iter()
                .zip(assign)
                .filter(|(_, a)| **a == k)
                .map(|(s, _)| s)
                .collect();
            if members.is_empty() {
                continue;
            }
            let n = members.len() as f32;
            let mut mean = [0f32; 3];
            for s in &members {
                for c in 0..3 {
                    mean[c] += s[c] / n;
                }
            }
            let mut cov = [[0f32; 3]; 3];
            for s in &members {
                for r in 0..3 {
                    for c in 0..3 {
                        cov[r][c] += (s[r] - mean[r]) * (s[c] - mean[c]) / n;
                    }
                }
            }
            for (d, row) in cov.iter_mut().enumerate() {
                row[d] += 0.01 * 255.0; // regularize (a few levels of variance)
            }
            let det = cov[0][0] * (cov[1][1] * cov[2][2] - cov[1][2] * cov[2][1])
                - cov[0][1] * (cov[1][0] * cov[2][2] - cov[1][2] * cov[2][0])
                + cov[0][2] * (cov[1][0] * cov[2][1] - cov[1][1] * cov[2][0]);
            let det = det.max(1e-6);
            let mut inverse = [[0f32; 3]; 3];
            for r in 0..3 {
                for c in 0..3 {
                    let (r1, r2) = ((c + 1) % 3, (c + 2) % 3);
                    let (c1, c2) = ((r + 1) % 3, (r + 2) % 3);
                    inverse[r][c] = (cov[r1][c1] * cov[r2][c2] - cov[r1][c2] * cov[r2][c1]) / det;
                }
            }
            let weight = n / total;
            parts.push(Gaussian {
                weight,
                mean,
                inverse,
                log_norm: weight.ln() - 0.5 * det.ln(),
            });
        }
        if parts.is_empty() {
            // No samples: a broad Gaussian.
            let v = 1.0 / (128.0 * 128.0);
            parts.push(Gaussian {
                weight: 1.0,
                mean: [128.0; 3],
                inverse: [[v, 0.0, 0.0], [0.0, v, 0.0], [0.0, 0.0, v]],
                log_norm: -0.5 * (128.0f32 * 128.0).powi(3).ln(),
            });
        }
        let _ = parts.iter().map(|p| p.weight);
        Gmm { parts }
    }

    /// Initial model: k-means into `COMPONENTS` clusters.
    #[allow(clippy::needless_range_loop)]
    fn init(samples: &[[f32; 3]]) -> Gmm {
        if samples.is_empty() {
            return Gmm::fit(&[], &[]);
        }
        // Deterministic seeds spread over the samples.
        let mut centers: Vec<[f32; 3]> = (0..COMPONENTS)
            .map(|k| samples[k * samples.len() / COMPONENTS])
            .collect();
        let mut assign = vec![0usize; samples.len()];
        for _ in 0..8 {
            for (s, a) in samples.iter().zip(assign.iter_mut()) {
                *a = (0..COMPONENTS)
                    .min_by(|&i, &j| dist2(s, &centers[i]).total_cmp(&dist2(s, &centers[j])))
                    .unwrap();
            }
            let mut sums = vec![([0f32; 3], 0f32); COMPONENTS];
            for (s, &a) in samples.iter().zip(&assign) {
                for c in 0..3 {
                    sums[a].0[c] += s[c];
                }
                sums[a].1 += 1.0;
            }
            for (center, (sum, n)) in centers.iter_mut().zip(sums) {
                if n > 0.0 {
                    *center = sum.map(|v| v / n);
                }
            }
        }
        Gmm::fit(samples, &assign)
    }

    fn refit(&self, samples: &[[f32; 3]]) -> Gmm {
        let assign: Vec<usize> = samples.iter().map(|s| self.component(*s)).collect();
        Gmm::fit(samples, &assign)
    }
}

fn dist2(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    (0..3).map(|c| (a[c] - b[c]).powi(2)).sum()
}

/// Every `step`-th element (keeps model fitting fast on large regions).
fn subsample(samples: Vec<[f32; 3]>, max: usize) -> Vec<[f32; 3]> {
    if samples.len() <= max {
        return samples;
    }
    let step = samples.len().div_ceil(max);
    samples.into_iter().step_by(step).collect()
}

/// Estimates the object's box from include points: a region grown from them
/// across small neighbour differences (smooth shading, noise) and stopped by
/// edges, padded by 15 % (at least 8 px). `None` without seeds or when the
/// region covers most of the image.
fn click_box(work: &RgbaImage, seeds: &[(f32, f32)]) -> Option<(WorkRect, Vec<bool>)> {
    if seeds.is_empty() {
        return None;
    }
    let smooth = image::imageops::blur(work, 1.0);
    // Tighter steps until the region stays off most of the image.
    for step in [6.0f32, 4.0, 2.5] {
        if let Some(found) = grow(&smooth, seeds, step) {
            return Some(found);
        }
    }
    None
}

/// The padded box of the region grown from `seeds` with neighbour steps of at
/// most `step`, and the region (`None` if it leaked: more than half the image
/// or touching its border).
fn grow(smooth: &RgbaImage, seeds: &[(f32, f32)], step: f32) -> Option<(WorkRect, Vec<bool>)> {
    let (w, h) = smooth.dimensions();
    let px = |x: u32, y: u32| {
        let p = smooth.get_pixel(x, y);
        [p[0] as f32, p[1] as f32, p[2] as f32]
    };
    let mut seen = vec![false; (w * h) as usize];
    let mut stack = Vec::new();
    for &(sx, sy) in seeds {
        let (x, y) = (
            (sx.max(0.0) as u32).min(w - 1),
            (sy.max(0.0) as u32).min(h - 1),
        );
        if !seen[(y * w + x) as usize] {
            seen[(y * w + x) as usize] = true;
            stack.push((x, y));
        }
    }
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    let mut count = 0u32;
    while let Some((x, y)) = stack.pop() {
        count += 1;
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + 1);
        y1 = y1.max(y + 1);
        let here = px(x, y);
        for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                continue;
            }
            let (nx, ny) = (nx as u32, ny as u32);
            let i = (ny * w + nx) as usize;
            if !seen[i] && dist2(&here, &px(nx, ny)).sqrt() <= step {
                seen[i] = true;
                stack.push((nx, ny));
            }
        }
    }
    // A region over half the image, or reaching its border, has leaked past
    // the object's edge.
    if count as f32 > 0.5 * (w * h) as f32 || x0 == 0 || y0 == 0 || x1 == w || y1 == h {
        return None;
    }
    let pad_x = ((x1 - x0) as f32 * 0.25).max(8.0);
    let pad_y = ((y1 - y0) as f32 * 0.25).max(8.0);
    Some((
        (
            ((x0 as f32 - pad_x).max(0.0), (y0 as f32 - pad_y).max(0.0)),
            (
                (x1 as f32 + pad_x).min(w as f32),
                (y1 as f32 + pad_y).min(h as f32),
            ),
        ),
        seen,
    ))
}

/// The image at GrabCut's working resolution and its smoothness edges.
struct Work {
    z: Vec<[f32; 3]>,
    w: u32,
    h: u32,
    /// Full-resolution pixels per work pixel (≥ 1).
    scale: f32,
    image: RgbaImage,
    /// (a, b, capacity) for the 8-neighbour edges.
    smooth: Vec<(usize, usize, f32)>,
}

fn prepare(image: &RgbaImage) -> Work {
    let (w, h) = image.dimensions();
    let scale = (w.max(h) as f32 / WORK_SIDE as f32).max(1.0);
    let (ww, wh) = (
        ((w as f32 / scale).round() as u32).max(1),
        ((h as f32 / scale).round() as u32).max(1),
    );
    let work = if scale > 1.0 {
        image::imageops::resize(image, ww, wh, image::imageops::FilterType::Triangle)
    } else {
        image.clone()
    };
    let z: Vec<[f32; 3]> = work
        .pixels()
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
        .collect();
    // β from the mean squared neighbour difference.
    let diagonal = 1.0 / 2f32.sqrt();
    let neighbours: [(i64, i64, f32); 4] = [
        (1, 0, 1.0),
        (0, 1, 1.0),
        (1, 1, diagonal),
        (-1, 1, diagonal),
    ];
    let index = |x: i64, y: i64| (y * ww as i64 + x) as usize;
    let mut pairs = Vec::with_capacity(z.len() * 4);
    let mut sum = 0.0f64;
    for y in 0..wh as i64 {
        for x in 0..ww as i64 {
            for &(dx, dy, weight) in &neighbours {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && nx < ww as i64 && ny < wh as i64 {
                    let (a, b) = (index(x, y), index(nx, ny));
                    let d = dist2(&z[a], &z[b]);
                    sum += d as f64;
                    pairs.push((a, b, weight, d));
                }
            }
        }
    }
    let beta = if sum > 0.0 {
        (pairs.len() as f64 / (2.0 * sum)) as f32
    } else {
        0.0
    };
    let smooth = pairs
        .into_iter()
        .map(|(a, b, weight, d)| (a, b, GAMMA * weight * (-beta * d).exp()))
        .collect();
    Work {
        z,
        w: ww,
        h: wh,
        scale,
        image: work,
        smooth,
    }
}

/// A finished cut at work resolution.
struct Cut {
    fg: Vec<bool>,
    fg_model: Gmm,
    bg_model: Gmm,
}

type WorkRect = ((f32, f32), (f32, f32));

/// GrabCut with `rect` (outside = background) or, without one, the image
/// border as background; `points` (work coordinates) pin disks. With
/// `region`, only those pixels start as object inside the box (so nearby
/// shadows do not enter the object's colour model).
fn run_cut(
    work: &Work,
    rect: Option<WorkRect>,
    points: &[(f32, f32, bool)],
    region: Option<&[bool]>,
) -> Cut {
    let (ww, wh) = (work.w, work.h);
    let n = (ww * wh) as usize;
    let z = &work.z;
    let mut trimap = vec![Trimap::ProbBg; n];
    match rect {
        Some((a, b)) => {
            let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
            let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
            for y in 0..wh {
                for x in 0..ww {
                    let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                    let inside = cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1;
                    let i = (y * ww + x) as usize;
                    trimap[i] = match (inside, region) {
                        (false, _) => Trimap::Bg,
                        (true, Some(r)) if !r[i] => Trimap::ProbBg,
                        (true, _) => Trimap::ProbFg,
                    };
                }
            }
        }
        None => {
            let band = ((ww.min(wh) as f32 * 0.02).ceil() as u32).max(1);
            for y in 0..wh {
                for x in 0..ww {
                    if x < band || y < band || x >= ww - band || y >= wh - band {
                        trimap[(y * ww + x) as usize] = Trimap::Bg;
                    }
                }
            }
        }
    }
    let radius = ((ww as f32).hypot(wh as f32) * 0.01).max(1.5);
    for &(cx, cy, include) in points {
        let r = radius.ceil() as i64;
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx.floor() as i64 + dx, cy.floor() as i64 + dy);
                if x < 0 || y < 0 || x >= ww as i64 || y >= wh as i64 {
                    continue;
                }
                if ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt() <= radius
                {
                    trimap[(y as u32 * ww + x as u32) as usize] =
                        if include { Trimap::Fg } else { Trimap::Bg };
                }
            }
        }
    }
    let hard = 1.0 + 8.0 * GAMMA;
    let is_fg = |t: Trimap| matches!(t, Trimap::Fg | Trimap::ProbFg);
    let samples = |trimap: &[Trimap], fg: bool| -> Vec<[f32; 3]> {
        subsample(
            trimap
                .iter()
                .zip(z)
                .filter(|(t, _)| is_fg(**t) == fg)
                .map(|(_, v)| *v)
                .collect(),
            20_000,
        )
    };
    let mut fg_model = Gmm::init(&samples(&trimap, true));
    // With a region start, the background model begins from definite
    // background only: in-box pixels outside the region are undecided, not
    // evidence for the background.
    let mut bg_model = if region.is_some() {
        Gmm::init(&subsample(
            trimap
                .iter()
                .zip(z)
                .filter(|(t, _)| **t == Trimap::Bg)
                .map(|(_, v)| *v)
                .collect(),
            20_000,
        ))
    } else {
        Gmm::init(&samples(&trimap, false))
    };
    for iteration in 0..ITERATIONS {
        if iteration > 0 {
            fg_model = fg_model.refit(&samples(&trimap, true));
            bg_model = bg_model.refit(&samples(&trimap, false));
        }
        let mut graph = Graph::new(n, n * 12);
        for (i, t) in trimap.iter().enumerate() {
            let (to_source, to_sink) = match t {
                Trimap::Fg => (hard, 0.0),
                Trimap::Bg => (0.0, hard),
                _ => (bg_model.cost(z[i]), fg_model.cost(z[i])),
            };
            graph.terminals(i, to_source, to_sink);
        }
        for &(a, b, cap) in &work.smooth {
            graph.edge(a, b, cap);
        }
        let side = graph.min_cut();
        let mut changed = 0;
        for (i, t) in trimap.iter_mut().enumerate() {
            if matches!(t, Trimap::ProbBg | Trimap::ProbFg) {
                let new = if side[i] {
                    Trimap::ProbFg
                } else {
                    Trimap::ProbBg
                };
                if new != *t {
                    changed += 1;
                }
                *t = new;
            }
        }
        if iteration > 0 && changed == 0 {
            break;
        }
    }
    Cut {
        fg: trimap.iter().map(|t| is_fg(*t)).collect(),
        fg_model,
        bg_model,
    }
}

/// Fraction of the box's edge pixels (inside the image) labelled object.
fn edge_fraction(work: &Work, cut: &Cut, rect: WorkRect) -> f32 {
    let (ww, wh) = (work.w as i64, work.h as i64);
    let ((ax, ay), (bx, by)) = rect;
    let x0 = (ax.min(bx).ceil() as i64).clamp(0, ww - 1);
    let x1 = (ax.max(bx).floor() as i64 - 1).clamp(0, ww - 1);
    let y0 = (ay.min(by).ceil() as i64).clamp(0, wh - 1);
    let y1 = (ay.max(by).floor() as i64 - 1).clamp(0, wh - 1);
    let (mut on, mut total) = (0u32, 0u32);
    let mut count = |x: i64, y: i64| {
        total += 1;
        on += cut.fg[(y * ww + x) as usize] as u32;
    };
    for x in x0..=x1 {
        count(x, y0);
        count(x, y1);
    }
    for y in y0..=y1 {
        count(x0, y);
        count(x1, y);
    }
    on as f32 / total.max(1) as f32
}

/// Click-only prompts: candidate boxes around the include points (the
/// region-grown box, then squares of 30 %, 50 %, 75 %, and 100 % of the short
/// side), smallest first; the first whose object does not touch its edges wins.
fn cut_from_points(work: &Work, points: &[(f32, f32, bool)]) -> Cut {
    let includes: Vec<(f32, f32)> = points.iter().filter(|p| p.2).map(|p| (p.0, p.1)).collect();
    if includes.is_empty() {
        return run_cut(work, None, points, None);
    }
    let (ww, wh) = (work.w as f32, work.h as f32);
    let cx = includes.iter().map(|p| p.0).sum::<f32>() / includes.len() as f32;
    let cy = includes.iter().map(|p| p.1).sum::<f32>() / includes.len() as f32;
    // The grown region (when it stays compact) is the most specific start.
    if let Some((rect, region)) = click_box(&work.image, &includes) {
        let cut = run_cut(work, Some(rect), points, Some(&region));
        if edge_fraction(work, &cut, rect) <= 0.02 {
            return cut;
        }
    }
    let mut candidates: Vec<WorkRect> = Vec::new();
    for fraction in [0.3f32, 0.5, 0.75, 1.0] {
        let side = ww.min(wh) * fraction;
        let x0 = (cx - side / 2.0).clamp(0.0, (ww - side).max(0.0));
        let y0 = (cy - side / 2.0).clamp(0.0, (wh - side).max(0.0));
        candidates.push(((x0, y0), ((x0 + side).min(ww), (y0 + side).min(wh))));
    }
    let area = |r: &WorkRect| ((r.1 .0 - r.0 .0) * (r.1 .1 - r.0 .1)).abs();
    candidates.sort_by(|a, b| area(a).total_cmp(&area(b)));
    let mut last = None;
    for rect in candidates {
        let cut = run_cut(work, Some(rect), points, None);
        if edge_fraction(work, &cut, rect) <= 0.02 {
            return cut;
        }
        last = Some(cut);
    }
    last.expect("at least one candidate")
}

/// Segments the object described by `prompt` (in image coordinates);
/// returns a full-size mask (255 = object).
pub fn segment(image: &RgbaImage, prompt: &Prompt) -> GrayImage {
    let (w, h) = image.dimensions();
    let work = prepare(image);
    let scale = work.scale;
    let to_work = |p: (f32, f32)| (p.0 / scale, p.1 / scale);
    let points: Vec<(f32, f32, bool)> = prompt
        .points
        .iter()
        .map(|&(x, y, include)| {
            let (x, y) = to_work((x, y));
            (x, y, include)
        })
        .collect();
    let cut = match prompt.rect {
        Some((a, b)) => run_cut(&work, Some((to_work(a), to_work(b))), &points, None),
        None => cut_from_points(&work, &points),
    };

    // Full resolution: upsample, then reclassify a band along the border with
    // the colour models.
    let (ww, wh) = (work.w, work.h);
    let label_at = |x: u32, y: u32| -> bool {
        let wx = ((x as f32 + 0.5) / scale).floor().min(ww as f32 - 1.0) as u32;
        let wy = ((y as f32 + 0.5) / scale).floor().min(wh as f32 - 1.0) as u32;
        cut.fg[(wy * ww + wx) as usize]
    };
    let mut mask = GrayImage::from_fn(w, h, |x, y| Luma([if label_at(x, y) { 255 } else { 0 }]));
    if scale > 1.0 {
        let band = scale.ceil() as i64 + 1;
        let coarse = mask.clone();
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let here = coarse.get_pixel(x as u32, y as u32)[0];
                let mut border = false;
                'scan: for dy in [-band, 0, band] {
                    for dx in [-band, 0, band] {
                        let (nx, ny) = (
                            (x + dx).clamp(0, w as i64 - 1),
                            (y + dy).clamp(0, h as i64 - 1),
                        );
                        if coarse.get_pixel(nx as u32, ny as u32)[0] != here {
                            border = true;
                            break 'scan;
                        }
                    }
                }
                if border {
                    let p = image.get_pixel(x as u32, y as u32);
                    let zp = [p[0] as f32, p[1] as f32, p[2] as f32];
                    let fg = cut.fg_model.cost(zp) < cut.bg_model.cost(zp);
                    mask.put_pixel(x as u32, y as u32, Luma([if fg { 255 } else { 0 }]));
                }
            }
        }
    }
    let seeds = prompt.seeds(w, h);
    super::clean_mask(&mask, &seeds, 0.005)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn disc_image() -> RgbaImage {
        RgbaImage::from_fn(120, 90, |x, y| {
            let d = ((x as f32 - 60.0).powi(2) + (y as f32 - 45.0).powi(2)).sqrt();
            let n = ((x * 31 + y * 17) % 13) as u8;
            if d < 25.0 {
                Rgba([200 + n, 60 + n, 30, 255])
            } else {
                Rgba([40 + n, 90 + n, 140 + n, 255])
            }
        })
    }

    fn iou(mask: &GrayImage, truth: impl Fn(u32, u32) -> bool) -> f32 {
        let (mut i, mut u) = (0, 0);
        for (x, y, p) in mask.enumerate_pixels() {
            let (a, b) = (p[0] > 0, truth(x, y));
            i += (a && b) as u32;
            u += (a || b) as u32;
        }
        i as f32 / u as f32
    }

    #[test]
    fn click_and_box_segment_a_disc() {
        let img = disc_image();
        let truth =
            |x: u32, y: u32| ((x as f32 - 60.0).powi(2) + (y as f32 - 45.0).powi(2)).sqrt() < 25.0;
        let click = segment(&img, &Prompt::click((60.0, 45.0)));
        assert!(
            iou(&click, truth) > 0.95,
            "click IoU {}",
            iou(&click, truth)
        );
        let boxed = segment(&img, &Prompt::boxed((30.0, 15.0), (92.0, 76.0)));
        assert!(iou(&boxed, truth) > 0.95, "box IoU {}", iou(&boxed, truth));
    }
}
