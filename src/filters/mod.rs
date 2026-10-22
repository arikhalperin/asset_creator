//! The internal filter-plugin API and registry. Each filter lives in its own
//! file below and is listed once in [`registry`]; menus and dialogs are
//! generated from the filters' parameter schemas.

use std::collections::BTreeMap;

use image::{GrayImage, Rgba, RgbaImage};

use crate::document::PixelRect;
use crate::undo::blend_by_mask;

mod add_noise;
mod brightness_contrast;
mod color_balance;
mod curves;
mod desaturate;
mod enhance_lines;
mod gaussian_blur;
mod hue_saturation;
mod invert;
mod levels;
mod make_seamless;
mod median;
mod motion_blur;
mod posterize;
mod sharpen;
mod unsharp_mask;

pub use curves::{curve_lut, IDENTITY_CURVE};

/// Hidden parameters `render_filter` sets: where the filtered area starts in
/// the image, and the image width (not part of any schema).
pub const ORIGIN_X: &str = "_origin_x";
pub const ORIGIN_Y: &str = "_origin_y";
pub const IMAGE_WIDTH: &str = "_image_width";
/// Hidden parameter the editor sets: the document's DPI (filters with
/// millimetre settings use it; 300 when missing).
pub const DPI: &str = "_dpi";

/// Where a filter appears in the Filter menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Adjustments,
    Blur,
    Sharpen,
    Noise,
    Tile,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Self::Adjustments,
        Self::Blur,
        Self::Sharpen,
        Self::Noise,
        Self::Tile,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Adjustments => "Adjustments",
            Self::Blur => "Blur",
            Self::Sharpen => "Sharpen",
            Self::Noise => "Noise",
            Self::Tile => "Tile",
        }
    }
}

/// The type, range, and default of one parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamKind {
    Int {
        min: i64,
        max: i64,
        default: i64,
    },
    Float {
        min: f32,
        max: f32,
        default: f32,
        step: f32,
    },
    Bool {
        default: bool,
    },
    Choice {
        options: &'static [&'static str],
        default: usize,
    },
    /// Integer seed; the default is a fresh random value.
    Seed,
    /// Curve control points (x, y in 0–255); the default is the identity line.
    Curve,
}

/// One entry of a filter's parameter schema.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: ParamKind,
    /// Shown only while the choice parameter `.0` has option index `.1` selected.
    pub group: Option<(&'static str, usize)>,
}

impl ParamSpec {
    pub const fn new(key: &'static str, label: &'static str, kind: ParamKind) -> Self {
        Self {
            key,
            label,
            kind,
            group: None,
        }
    }

    pub const fn in_group(mut self, selector: &'static str, option: usize) -> Self {
        self.group = Some((selector, option));
        self
    }
}

/// A parameter value.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    Int(i64),
    Float(f32),
    Bool(bool),
    Choice(usize),
    Seed(u64),
    Curve(Vec<(f32, f32)>),
}

/// Parameter values of one filter invocation, keyed by parameter key.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Params {
    values: BTreeMap<&'static str, ParamValue>,
}

/// A fresh pseudo-random seed (time-based; reproducibility comes from the seed itself).
pub fn random_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    splitmix64(nanos)
}

impl Params {
    /// Default values for `schema` (seeds get a fresh random value).
    pub fn defaults(schema: &[ParamSpec]) -> Self {
        let mut p = Self::default();
        for spec in schema {
            let value = match &spec.kind {
                ParamKind::Int { default, .. } => ParamValue::Int(*default),
                ParamKind::Float { default, .. } => ParamValue::Float(*default),
                ParamKind::Bool { default } => ParamValue::Bool(*default),
                ParamKind::Choice { default, .. } => ParamValue::Choice(*default),
                ParamKind::Seed => ParamValue::Seed(random_seed()),
                ParamKind::Curve => ParamValue::Curve(IDENTITY_CURVE.to_vec()),
            };
            p.values.insert(spec.key, value);
        }
        p
    }

    /// Sets a value (clamped by [`Self::clamp`] before use).
    pub fn set(&mut self, key: &'static str, value: ParamValue) {
        self.values.insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<&ParamValue> {
        self.values.get(key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut ParamValue> {
        self.values.get_mut(key)
    }

    pub fn float(&self, key: &str) -> f32 {
        match self.values.get(key) {
            Some(ParamValue::Float(v)) => *v,
            Some(ParamValue::Int(v)) => *v as f32,
            _ => 0.0,
        }
    }

    pub fn int(&self, key: &str) -> i64 {
        match self.values.get(key) {
            Some(ParamValue::Int(v)) => *v,
            Some(ParamValue::Float(v)) => v.round() as i64,
            _ => 0,
        }
    }

    pub fn flag(&self, key: &str) -> bool {
        matches!(self.values.get(key), Some(ParamValue::Bool(true)))
    }

    pub fn choice(&self, key: &str) -> usize {
        match self.values.get(key) {
            Some(ParamValue::Choice(v)) => *v,
            _ => 0,
        }
    }

    pub fn seed(&self, key: &str) -> u64 {
        match self.values.get(key) {
            Some(ParamValue::Seed(v)) => *v,
            _ => 0,
        }
    }

    pub fn curve(&self, key: &str) -> Vec<(f32, f32)> {
        match self.values.get(key) {
            Some(ParamValue::Curve(points)) => points.clone(),
            _ => IDENTITY_CURVE.to_vec(),
        }
    }

    /// Brings every value into its schema range (missing values get defaults).
    pub fn clamp(&mut self, schema: &[ParamSpec]) {
        for spec in schema {
            let entry = self.values.get(spec.key).cloned();
            let value = match (&spec.kind, entry) {
                (ParamKind::Int { min, max, .. }, Some(ParamValue::Int(v))) => {
                    ParamValue::Int(v.clamp(*min, *max))
                }
                (ParamKind::Float { min, max, .. }, Some(ParamValue::Float(v)))
                    if v.is_finite() =>
                {
                    ParamValue::Float(v.clamp(*min, *max))
                }
                (ParamKind::Bool { .. }, Some(v @ ParamValue::Bool(_))) => v,
                (ParamKind::Choice { options, .. }, Some(ParamValue::Choice(v))) => {
                    ParamValue::Choice(v.min(options.len().saturating_sub(1)))
                }
                (ParamKind::Seed, Some(v @ ParamValue::Seed(_))) => v,
                (ParamKind::Curve, Some(ParamValue::Curve(points))) => {
                    ParamValue::Curve(curves::normalize(points))
                }
                _ => Params::defaults(std::slice::from_ref(spec))
                    .values
                    .remove(spec.key)
                    .expect("default"),
            };
            self.values.insert(spec.key, value);
        }
    }
}

/// A filter plugin.
pub trait Filter: Sync {
    /// Stable identifier (used for golden files).
    fn id(&self) -> &'static str;
    /// Display name, also the undo-step label.
    fn name(&self) -> &'static str;
    fn category(&self) -> Category;
    /// Parameter schema; empty for filters that apply immediately.
    fn params(&self) -> Vec<ParamSpec> {
        Vec::new()
    }
    /// Pixels beyond a region that its result depends on (neighbourhood radius).
    fn margin(&self, _params: &Params) -> u32 {
        0
    }
    /// Filters `source` (pure). Pixels outside `source` read as its edge pixels.
    fn apply(&self, source: &RgbaImage, params: &Params) -> RgbaImage;
}

/// Every filter, in menu order.
pub fn registry() -> &'static [&'static dyn Filter] {
    static FILTERS: [&dyn Filter; 16] = [
        &brightness_contrast::BrightnessContrast,
        &levels::Levels,
        &curves::Curves,
        &hue_saturation::HueSaturation,
        &color_balance::ColorBalance,
        &desaturate::Desaturate,
        &invert::Invert,
        &posterize::Posterize,
        &gaussian_blur::GaussianBlur,
        &motion_blur::MotionBlur,
        &sharpen::Sharpen,
        &unsharp_mask::UnsharpMask,
        &enhance_lines::EnhanceLines,
        &add_noise::AddNoise,
        &median::Median,
        &make_seamless::MakeSeamless,
    ];
    &FILTERS
}

/// Looks a filter up by id.
pub fn find(id: &str) -> Option<&'static dyn Filter> {
    registry().iter().copied().find(|f| f.id() == id)
}

/// Filters `rect` of `snapshot` (a whole layer or mask in RGBA form) and
/// applies the result through `selection`, returning the region's new pixels.
/// Neighbourhood context comes from the snapshot around `rect`.
pub fn render_filter(
    snapshot: &RgbaImage,
    selection: Option<&GrayImage>,
    rect: PixelRect,
    filter: &dyn Filter,
    params: &Params,
) -> RgbaImage {
    let mut params = params.clone();
    params.clamp(&filter.params());
    let margin = filter.margin(&params);
    let (w, h) = snapshot.dimensions();
    let x0 = rect.x.saturating_sub(margin);
    let y0 = rect.y.saturating_sub(margin);
    let x1 = (rect.right() + margin).min(w);
    let y1 = (rect.bottom() + margin).min(h);
    let context = image::imageops::crop_imm(snapshot, x0, y0, x1 - x0, y1 - y0).to_image();
    // Position-dependent filters (Add Noise) key off absolute image coordinates.
    params.set(ORIGIN_X, ParamValue::Int(x0 as i64));
    params.set(ORIGIN_Y, ParamValue::Int(y0 as i64));
    params.set(IMAGE_WIDTH, ParamValue::Int(w as i64));
    let filtered = filter.apply(&context, &params);
    let (ox, oy) = (rect.x - x0, rect.y - y0);
    RgbaImage::from_fn(rect.width, rect.height, |x, y| {
        let old = *snapshot.get_pixel(rect.x + x, rect.y + y);
        let new = *filtered.get_pixel(ox + x, oy + y);
        let coverage = selection.map_or(255, |s| s.get_pixel(rect.x + x, rect.y + y)[0]);
        blend_by_mask(old, new, coverage)
    })
}

// ---------------------------------------------------------------------------
// Shared helpers for filter implementations.

/// SplitMix64: a small, fast, high-quality mixing function.
pub fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Applies `f` to every pixel (in parallel rows).
pub(crate) fn map_pixels(source: &RgbaImage, f: impl Fn(Rgba<u8>) -> Rgba<u8> + Sync) -> RgbaImage {
    let mut out = source.clone();
    let w = source.width() as usize;
    let h = source.height();
    crate::transform::parallel_rows(&mut out, w * 4, 0..h, |_, row| {
        for px in row.as_chunks_mut::<4>().0 {
            *px = f(Rgba(*px)).0;
        }
    });
    out
}

/// Applies a per-channel lookup table to R, G, B (alpha unchanged).
pub(crate) fn apply_luts(source: &RgbaImage, luts: &[[u8; 256]; 3]) -> RgbaImage {
    map_pixels(source, |p| {
        Rgba([
            luts[0][p[0] as usize],
            luts[1][p[1] as usize],
            luts[2][p[2] as usize],
            p[3],
        ])
    })
}

pub(crate) fn to_u8(v: f32) -> u8 {
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Premultiplied RGBA in f32 (0–255 scale), row-major.
pub(crate) struct Premul {
    pub w: usize,
    pub h: usize,
    pub data: Vec<[f32; 4]>,
}

impl Premul {
    pub fn from_image(img: &RgbaImage) -> Self {
        let data = img
            .pixels()
            .map(|p| {
                let a = p[3] as f32 / 255.0;
                [
                    p[0] as f32 * a,
                    p[1] as f32 * a,
                    p[2] as f32 * a,
                    p[3] as f32,
                ]
            })
            .collect();
        Self {
            w: img.width() as usize,
            h: img.height() as usize,
            data,
        }
    }

    pub fn to_image(&self) -> RgbaImage {
        let mut out = RgbaImage::new(self.w as u32, self.h as u32);
        for (px, v) in out.pixels_mut().zip(&self.data) {
            *px = unpremultiply(*v);
        }
        out
    }

    pub fn at(&self, x: isize, y: isize) -> [f32; 4] {
        let x = x.clamp(0, self.w as isize - 1) as usize;
        let y = y.clamp(0, self.h as isize - 1) as usize;
        self.data[y * self.w + x]
    }
}

pub(crate) fn unpremultiply(v: [f32; 4]) -> Rgba<u8> {
    let a = v[3].clamp(0.0, 255.0);
    if a <= 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let k = 255.0 / a;
    let c = |x: f32| (x * k).round().clamp(0.0, 255.0) as u8;
    Rgba([c(v[0]), c(v[1]), c(v[2]), a.round() as u8])
}

/// Runs `f(y, out_row)` over rows in parallel, where `out_row` has `w` pixels.
pub(crate) fn par_rows_f32(
    w: usize,
    h: usize,
    f: impl Fn(usize, &mut [[f32; 4]]) + Sync,
) -> Vec<[f32; 4]> {
    let mut out = vec![[0.0f32; 4]; w * h];
    if w == 0 || h == 0 {
        return out;
    }
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(16);
    let per = h.div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (c, chunk) in out.chunks_mut(per * w).enumerate() {
            let f = &f;
            scope.spawn(move || {
                for (i, row) in chunk.chunks_mut(w).enumerate() {
                    f(c * per + i, row);
                }
            });
        }
    });
    out
}

/// One horizontal box blur pass of half-width `r` (edge clamped).
fn box_pass(src: &Premul, r: usize) -> Premul {
    let (w, h) = (src.w, src.h);
    let norm = 1.0 / (2 * r + 1) as f32;
    let data = par_rows_f32(w, h, |y, out| {
        let row = &src.data[y * w..(y + 1) * w];
        let get = |x: isize| row[x.clamp(0, w as isize - 1) as usize];
        let mut acc = [0.0f32; 4];
        for k in -(r as isize)..=(r as isize) {
            let p = get(k);
            for c in 0..4 {
                acc[c] += p[c];
            }
        }
        for (x, o) in out.iter_mut().enumerate() {
            for c in 0..4 {
                o[c] = acc[c] * norm;
            }
            let add = get(x as isize + r as isize + 1);
            let sub = get(x as isize - r as isize);
            for c in 0..4 {
                acc[c] += add[c] - sub[c];
            }
        }
    });
    Premul { w, h, data }
}

/// Box half-widths of three passes approximating a Gaussian of `sigma`.
fn three_box_radii(sigma: f32) -> [usize; 3] {
    let n = 3.0;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n)
        / (-4.0 * wl as f32 - 4.0))
        .round() as i64;
    let mut out = [0usize; 3];
    for (i, r) in out.iter_mut().enumerate() {
        let width = if (i as i64) < m { wl } else { wu };
        *r = ((width.max(1) - 1) / 2) as usize;
    }
    out
}

/// Exact separable Gaussian, kernel truncated at 3σ.
fn exact_gaussian(src: &Premul, sigma: f32) -> Premul {
    let k = (3.0 * sigma).ceil().max(1.0) as isize;
    let weights: Vec<f32> = (-k..=k)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = weights.iter().sum();
    let weights: Vec<f32> = weights.iter().map(|w| w / total).collect();
    let (w, h) = (src.w, src.h);
    let horiz = Premul {
        w,
        h,
        data: par_rows_f32(w, h, |y, out| {
            for (x, o) in out.iter_mut().enumerate() {
                let mut acc = [0.0f32; 4];
                for (i, wt) in weights.iter().enumerate() {
                    let p = src.at(x as isize + i as isize - k, y as isize);
                    for c in 0..4 {
                        acc[c] += p[c] * wt;
                    }
                }
                *o = acc;
            }
        }),
    };
    Premul {
        w,
        h,
        data: par_rows_f32(w, h, |y, out| {
            for (x, o) in out.iter_mut().enumerate() {
                let mut acc = [0.0f32; 4];
                for (i, wt) in weights.iter().enumerate() {
                    let p = horiz.at(x as isize, y as isize + i as isize - k);
                    for c in 0..4 {
                        acc[c] += p[c] * wt;
                    }
                }
                *o = acc;
            }
        }),
    }
}

/// Gaussian blur of σ in premultiplied space (exact below σ = 2, three box blurs above).
pub(crate) fn gaussian(src: &Premul, sigma: f32) -> Premul {
    if sigma <= 0.0 || src.w == 0 || src.h == 0 {
        return Premul {
            w: src.w,
            h: src.h,
            data: src.data.clone(),
        };
    }
    if sigma < 2.0 {
        return exact_gaussian(src, sigma);
    }
    // Box passes along one axis commute with those along the other, so run
    // all horizontal passes, transpose, run them again, and transpose back:
    // every pass is then row-parallel.
    let radii = three_box_radii(sigma);
    let mut out = Premul {
        w: src.w,
        h: src.h,
        data: src.data.clone(),
    };
    for r in radii {
        out = box_pass(&out, r);
    }
    out = transpose(&out);
    for r in radii {
        out = box_pass(&out, r);
    }
    transpose(&out)
}

/// Swaps rows and columns (in parallel).
fn transpose(src: &Premul) -> Premul {
    let (w, h) = (src.w, src.h);
    Premul {
        w: h,
        h: w,
        data: par_rows_f32(h, w, |x, out| {
            for (y, o) in out.iter_mut().enumerate() {
                *o = src.data[y * w + x];
            }
        }),
    }
}

/// The margin a Gaussian of `sigma` needs.
pub(crate) fn gaussian_margin(sigma: f32) -> u32 {
    if sigma < 2.0 {
        (3.0 * sigma).ceil() as u32 + 1
    } else {
        three_box_radii(sigma).iter().sum::<usize>() as u32 + 1
    }
}

/// Convolves with sparse integer-offset taps `(dx, dy, weight)` (edge clamped).
pub(crate) fn convolve_taps(src: &Premul, taps: &[(isize, isize, f32)]) -> Premul {
    let (w, h) = (src.w, src.h);
    Premul {
        w,
        h,
        data: par_rows_f32(w, h, |y, out| {
            for &(dx, dy, wt) in taps {
                let sy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
                let row = &src.data[sy * w..(sy + 1) * w];
                // Interior: contiguous, vectorizable.
                let lo = (-dx).clamp(0, w as isize) as usize;
                let hi = (w as isize - dx).clamp(0, w as isize) as usize;
                for x in 0..lo.min(w) {
                    let p = row[(x as isize + dx).clamp(0, w as isize - 1) as usize];
                    for c in 0..4 {
                        out[x][c] += p[c] * wt;
                    }
                }
                if hi > lo {
                    let src_slice = &row[(lo as isize + dx) as usize..(hi as isize + dx) as usize];
                    for (o, p) in out[lo..hi].iter_mut().zip(src_slice) {
                        for c in 0..4 {
                            o[c] += p[c] * wt;
                        }
                    }
                }
                for x in hi.max(lo)..w {
                    let p = row[(x as isize + dx).clamp(0, w as isize - 1) as usize];
                    for c in 0..4 {
                        out[x][c] += p[c] * wt;
                    }
                }
            }
        }),
    }
}

/// Unpremultiplied RGB in [0, 1].
pub(crate) fn rgb01(p: Rgba<u8>) -> [f32; 3] {
    [
        p[0] as f32 / 255.0,
        p[1] as f32 / 255.0,
        p[2] as f32 / 255.0,
    ]
}

pub(crate) fn luminance(rgb: [f32; 3]) -> f32 {
    0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_are_unique_and_schemas_valid() {
        let mut ids: Vec<&str> = registry().iter().map(|f| f.id()).collect();
        assert_eq!(ids.len(), 16);
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 16, "unique ids");
        for f in registry() {
            let schema = f.params();
            let mut keys: Vec<&str> = schema.iter().map(|s| s.key).collect();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), schema.len(), "{}: unique keys", f.id());
            let defaults = Params::defaults(&schema);
            let mut clamped = defaults.clone();
            clamped.clamp(&schema);
            for spec in &schema {
                if spec.kind != ParamKind::Seed {
                    assert_eq!(
                        clamped.get(spec.key),
                        defaults.get(spec.key),
                        "{} {}",
                        f.id(),
                        spec.key
                    );
                }
                if let Some((selector, option)) = spec.group {
                    let sel = schema
                        .iter()
                        .find(|s| s.key == selector)
                        .expect("selector exists");
                    match &sel.kind {
                        ParamKind::Choice { options, .. } => assert!(option < options.len()),
                        _ => panic!("group selector must be a choice"),
                    }
                }
            }
            assert!(find(f.id()).is_some());
        }
        let menu: Vec<Category> = registry().iter().map(|f| f.category()).collect();
        assert!(menu
            .windows(2)
            .all(|w| Category::ALL.iter().position(|c| *c == w[0])
                <= Category::ALL.iter().position(|c| *c == w[1])));
    }

    #[test]
    fn params_clamp_to_schema() {
        let schema = vec![
            ParamSpec::new(
                "a",
                "A",
                ParamKind::Int {
                    min: 1,
                    max: 5,
                    default: 2,
                },
            ),
            ParamSpec::new(
                "b",
                "B",
                ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    step: 0.1,
                },
            ),
            ParamSpec::new(
                "c",
                "C",
                ParamKind::Choice {
                    options: &["x", "y"],
                    default: 0,
                },
            ),
        ];
        let mut p = Params::defaults(&schema);
        p.set("a", ParamValue::Int(99));
        p.set("b", ParamValue::Float(f32::NAN));
        p.set("c", ParamValue::Choice(7));
        p.clamp(&schema);
        assert_eq!(p.int("a"), 5);
        assert_eq!(p.float("b"), 0.5);
        assert_eq!(p.choice("c"), 1);
    }

    #[test]
    fn gaussian_keeps_flat_images_and_three_box_radii_grow() {
        let img = RgbaImage::from_pixel(20, 10, Rgba([40, 80, 120, 200]));
        for sigma in [0.5, 1.5, 3.0, 12.0] {
            let out = gaussian(&Premul::from_image(&img), sigma).to_image();
            assert!(
                out.pixels().all(|p| p.0 == [40, 80, 120, 200]),
                "sigma {sigma}"
            );
        }
        assert!(
            three_box_radii(10.0).iter().sum::<usize>()
                > three_box_radii(3.0).iter().sum::<usize>()
        );
    }

    #[test]
    fn render_filter_uses_context_and_selection() {
        // A blur of a stripe near the region edge must see pixels outside the region.
        let mut img = RgbaImage::from_pixel(40, 10, Rgba([0, 0, 0, 255]));
        for y in 0..10 {
            img.put_pixel(21, y, Rgba([255, 255, 255, 255]));
        }
        let blur = find("gaussian_blur").unwrap();
        let mut params = Params::defaults(&blur.params());
        params.set("radius", ParamValue::Float(1.5));
        let full = render_filter(&img, None, PixelRect::new(0, 0, 40, 10), blur, &params);
        let part = render_filter(&img, None, PixelRect::new(10, 0, 10, 10), blur, &params);
        for x in 0..10 {
            assert_eq!(part.get_pixel(x, 5), full.get_pixel(10 + x, 5));
        }
        assert!(
            part.get_pixel(9, 5)[0] > 0,
            "stripe outside the region bleeds in"
        );
        // Selection: unselected pixels keep their values.
        let sel = GrayImage::new(40, 10);
        let none = render_filter(
            &img,
            Some(&sel),
            PixelRect::new(0, 0, 40, 10),
            blur,
            &params,
        );
        assert_eq!(none, img);
    }
}
