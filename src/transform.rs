//! Layer transforms: affine resampling, live transform sessions (Move tool,
//! Free Transform), handle interaction math, and flips. Window-independent.

use image::{GrayImage, Luma, Rgba, RgbaImage};

use crate::document::{blend_over, Document, PixelRect};
use crate::paint::union;
use crate::undo::{Command, CommandError};

/// How transformed pixels are sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Interpolation {
    #[default]
    Bilinear,
    Nearest,
}

impl Interpolation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bilinear => "Bilinear",
            Self::Nearest => "Nearest",
        }
    }
}

/// `p ↦ pivot + R(angle)·S·(p − pivot) + t`, in image pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub pivot: (f32, f32),
    pub translate: (f32, f32),
    pub scale: (f32, f32),
    /// Radians, clockwise on screen (y points down).
    pub angle: f32,
}

impl Affine {
    pub fn identity(pivot: (f32, f32)) -> Self {
        Self {
            pivot,
            translate: (0.0, 0.0),
            scale: (1.0, 1.0),
            angle: 0.0,
        }
    }

    /// cos/sin, exact for multiples of 90°.
    fn cos_sin(&self) -> (f32, f32) {
        let quarter = self.angle / std::f32::consts::FRAC_PI_2;
        if (quarter - quarter.round()).abs() < 1e-6 {
            match (quarter.round() as i64).rem_euclid(4) {
                0 => (1.0, 0.0),
                1 => (0.0, 1.0),
                2 => (-1.0, 0.0),
                _ => (0.0, -1.0),
            }
        } else {
            (self.angle.cos(), self.angle.sin())
        }
    }

    pub fn map(&self, p: (f32, f32)) -> (f32, f32) {
        let (c, s) = self.cos_sin();
        let (x, y) = (
            (p.0 - self.pivot.0) * self.scale.0,
            (p.1 - self.pivot.1) * self.scale.1,
        );
        (
            self.pivot.0 + c * x - s * y + self.translate.0,
            self.pivot.1 + s * x + c * y + self.translate.1,
        )
    }

    pub fn inverse(&self, p: (f32, f32)) -> (f32, f32) {
        let (c, s) = self.cos_sin();
        let (x, y) = (
            p.0 - self.pivot.0 - self.translate.0,
            p.1 - self.pivot.1 - self.translate.1,
        );
        let (rx, ry) = (c * x + s * y, -s * x + c * y);
        (
            self.pivot.0 + rx / self.scale.0,
            self.pivot.1 + ry / self.scale.1,
        )
    }

    pub fn is_identity(&self) -> bool {
        self.translate == (0.0, 0.0) && self.scale == (1.0, 1.0) && self.cos_sin() == (1.0, 0.0)
    }

    /// Corners of `rect` after mapping (top-left, top-right, bottom-right, bottom-left).
    pub fn corners(&self, rect: PixelRect) -> [(f32, f32); 4] {
        let (x0, y0, x1, y1) = (
            rect.x as f32,
            rect.y as f32,
            rect.right() as f32,
            rect.bottom() as f32,
        );
        [
            self.map((x0, y0)),
            self.map((x1, y0)),
            self.map((x1, y1)),
            self.map((x0, y1)),
        ]
    }

    /// Pixels covered by `rect` after mapping, clipped to `width × height`.
    pub fn mapped_bounds(&self, rect: PixelRect, width: u32, height: u32) -> Option<PixelRect> {
        let pts = self.corners(rect);
        let min_x = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min).floor() - 1.0;
        let min_y = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() - 1.0;
        let max_x = pts.iter().map(|p| p.0).fold(f32::MIN, f32::max).ceil() + 1.0;
        let max_y = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() + 1.0;
        let x0 = min_x.clamp(0.0, width as f32) as u32;
        let y0 = min_y.clamp(0.0, height as f32) as u32;
        let x1 = max_x.clamp(0.0, width as f32) as u32;
        let y1 = max_y.clamp(0.0, height as f32) as u32;
        (x1 > x0 && y1 > y0).then(|| PixelRect::new(x0, y0, x1 - x0, y1 - y0))
    }
}

/// The inverse transform as `src = k + mx·(x + ½) + my·(y + ½)`.
#[derive(Debug, Clone, Copy)]
struct LinearMap {
    k: (f32, f32),
    mx: (f32, f32),
    my: (f32, f32),
}

impl LinearMap {
    fn inverse_of(affine: &Affine) -> Self {
        let k = affine.inverse((0.0, 0.0));
        let ex = affine.inverse((1.0, 0.0));
        let ey = affine.inverse((0.0, 1.0));
        Self {
            k,
            mx: (ex.0 - k.0, ex.1 - k.1),
            my: (ey.0 - k.0, ey.1 - k.1),
        }
    }

    /// Source point of the center of output pixel `(x, y)`.
    fn at(&self, x: u32, y: u32) -> (f32, f32) {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        (
            self.k.0 + self.mx.0 * px + self.my.0 * py,
            self.k.1 + self.mx.1 * px + self.my.1 * py,
        )
    }
}

/// Runs `f(y, row)` for each row in `rows` of an image buffer with `stride`
/// bytes per row, split across threads for large regions.
fn parallel_rows(
    buf: &mut [u8],
    stride: usize,
    rows: std::ops::Range<u32>,
    f: impl Fn(u32, &mut [u8]) + Sync,
) {
    let (start, end) = (rows.start as usize, rows.end as usize);
    if end <= start {
        return;
    }
    let slice = &mut buf[start * stride..end * stride];
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(16);
    let count = end - start;
    if threads == 1 || count < 64 {
        for (i, row) in slice.chunks_mut(stride).enumerate() {
            f((start + i) as u32, row);
        }
        return;
    }
    let per = count.div_ceil(threads);
    std::thread::scope(|scope| {
        for (c, chunk) in slice.chunks_mut(per * stride).enumerate() {
            let f = &f;
            scope.spawn(move || {
                for (i, row) in chunk.chunks_mut(stride).enumerate() {
                    f((start + c * per + i) as u32, row);
                }
            });
        }
    });
}

/// Recombines the staying and moved parts by adding them in premultiplied
/// alpha (alpha capped at 1), so content moved by zero reassembles the
/// original pixel exactly even under a soft selection.
fn combine_parts(stay: Rgba<u8>, moved: Rgba<u8>) -> Rgba<u8> {
    if stay[3] == 0 {
        return moved;
    }
    if moved[3] == 0 {
        return stay;
    }
    if stay.0[..3] == moved.0[..3] {
        return Rgba([
            stay[0],
            stay[1],
            stay[2],
            (stay[3] as u16 + moved[3] as u16).min(255) as u8,
        ]);
    }
    let (sa, ma) = (stay[3] as f32, moved[3] as f32);
    let total = sa + ma;
    if total > 255.0 {
        // Overlap beyond full coverage: the moved content goes on top.
        return blend_over(stay, moved, 1.0);
    }
    let mut out = [0u8; 4];
    for c in 0..3 {
        out[c] = ((stay[c] as f32 * sa + moved[c] as f32 * ma) / total).round() as u8;
    }
    out[3] = total.round() as u8;
    Rgba(out)
}

/// Samples `img` at image point `p` (transparent outside).
pub fn sample(img: &RgbaImage, p: (f32, f32), interp: Interpolation) -> Rgba<u8> {
    let (w, h) = (img.width() as i64, img.height() as i64);
    let get = |x: i64, y: i64| {
        if x < 0 || y < 0 || x >= w || y >= h {
            Rgba([0, 0, 0, 0])
        } else {
            *img.get_pixel(x as u32, y as u32)
        }
    };
    match interp {
        Interpolation::Nearest => get(p.0.floor() as i64, p.1.floor() as i64),
        Interpolation::Bilinear => {
            let (u, v) = (p.0 - 0.5, p.1 - 0.5);
            let (x0, y0) = (u.floor(), v.floor());
            let (fx, fy) = (u - x0, v - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            if fx == 0.0 && fy == 0.0 {
                return get(x0, y0);
            }
            let taps = [
                (get(x0, y0), (1.0 - fx) * (1.0 - fy)),
                (get(x0 + 1, y0), fx * (1.0 - fy)),
                (get(x0, y0 + 1), (1.0 - fx) * fy),
                (get(x0 + 1, y0 + 1), fx * fy),
            ];
            let mut acc = [0.0f32; 4];
            for (px, wgt) in taps {
                let a = px[3] as f32 / 255.0 * wgt;
                for c in 0..3 {
                    acc[c] += px[c] as f32 * a;
                }
                acc[3] += a;
            }
            if acc[3] <= 0.0 {
                return Rgba([0, 0, 0, 0]);
            }
            let mut out = [0u8; 4];
            for c in 0..3 {
                out[c] = (acc[c] / acc[3]).round().clamp(0.0, 255.0) as u8;
            }
            out[3] = (acc[3] * 255.0).round().clamp(0.0, 255.0) as u8;
            Rgba(out)
        }
    }
}

/// Samples a gray image at `p` (`outside` beyond its edges).
pub fn sample_gray(img: &GrayImage, p: (f32, f32), interp: Interpolation, outside: u8) -> u8 {
    let (w, h) = (img.width() as i64, img.height() as i64);
    let get = |x: i64, y: i64| {
        if x < 0 || y < 0 || x >= w || y >= h {
            outside as f32
        } else {
            img.get_pixel(x as u32, y as u32)[0] as f32
        }
    };
    match interp {
        Interpolation::Nearest => get(p.0.floor() as i64, p.1.floor() as i64) as u8,
        Interpolation::Bilinear => {
            let (u, v) = (p.0 - 0.5, p.1 - 0.5);
            let (x0, y0) = (u.floor(), v.floor());
            let (fx, fy) = (u - x0, v - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let value = get(x0, y0) * (1.0 - fx) * (1.0 - fy)
                + get(x0 + 1, y0) * fx * (1.0 - fy)
                + get(x0, y0 + 1) * (1.0 - fx) * fy
                + get(x0 + 1, y0 + 1) * fx * fy;
            value.round().clamp(0.0, 255.0) as u8
        }
    }
}

/// Non-transparent bounds of a layer's pixels.
pub fn opaque_bounds(pixels: &RgbaImage) -> Option<PixelRect> {
    let alpha = GrayImage::from_fn(pixels.width(), pixels.height(), |x, y| {
        Luma([pixels.get_pixel(x, y)[3]])
    });
    crate::selection::bounds(&alpha)
}

/// A move/transform in progress on one layer: the document shows it live;
/// [`Self::commit`] produces the undo step and [`Self::cancel`] restores.
#[derive(Debug, Clone)]
pub struct TransformSession {
    layer: usize,
    base: RgbaImage,
    base_mask: Option<GrayImage>,
    base_selection: Option<GrayImage>,
    /// The part that moves (base × selection).
    content: RgbaImage,
    /// The part that stays (base × (1 − selection)).
    remainder: RgbaImage,
    content_bounds: PixelRect,
    affine: Affine,
    pub interpolation: Interpolation,
    last_bounds: Option<PixelRect>,
    touched: Option<PixelRect>,
}

impl TransformSession {
    /// Starts on the active layer; `bounds` overrides the content bounds
    /// (used by Flip, which mirrors about the canvas without a selection).
    pub fn begin(
        doc: &Document,
        interpolation: Interpolation,
        bounds: Option<PixelRect>,
    ) -> Result<Self, CommandError> {
        let layer_index = doc.active_layer();
        let layer = doc.layer(layer_index).ok_or(CommandError::NoSuchLayer {
            index: layer_index,
            len: doc.layer_count(),
        })?;
        let base = layer.pixels.clone();
        let selection = doc.selection().cloned();
        let mut content = base.clone();
        let mut remainder = RgbaImage::new(base.width(), base.height());
        if let Some(sel) = &selection {
            for (x, y, c) in content.enumerate_pixels_mut() {
                let s = sel.get_pixel(x, y)[0] as u32;
                let p = *base.get_pixel(x, y);
                c[3] = (p[3] as u32 * s / 255) as u8;
                // The two parts add back up to the original alpha exactly.
                let keep = p[3] - c[3];
                remainder.put_pixel(x, y, Rgba([p[0], p[1], p[2], keep]));
            }
        }
        let content_bounds = bounds
            .or_else(|| selection.as_ref().and_then(crate::selection::bounds))
            .or_else(|| opaque_bounds(&base))
            .unwrap_or(doc.bounds());
        let pivot = (
            content_bounds.x as f32 + content_bounds.width as f32 / 2.0,
            content_bounds.y as f32 + content_bounds.height as f32 / 2.0,
        );
        Ok(Self {
            layer: layer_index,
            base,
            base_mask: layer.mask().cloned(),
            base_selection: selection,
            content,
            remainder,
            content_bounds,
            affine: Affine::identity(pivot),
            interpolation,
            last_bounds: Some(content_bounds),
            touched: None,
        })
    }

    pub fn affine(&self) -> Affine {
        self.affine
    }

    /// The untransformed content bounds (the transform box before mapping).
    pub fn content_bounds(&self) -> PixelRect {
        self.content_bounds
    }

    pub fn layer(&self) -> usize {
        self.layer
    }

    /// Sets the transform and re-renders the old and new transformed bounds.
    /// Returns the region repainted.
    pub fn set_affine(&mut self, doc: &mut Document, affine: Affine) -> Option<PixelRect> {
        self.affine = affine;
        self.rerender(doc)
    }

    pub fn set_interpolation(
        &mut self,
        doc: &mut Document,
        interpolation: Interpolation,
    ) -> Option<PixelRect> {
        self.interpolation = interpolation;
        self.rerender(doc)
    }

    fn rerender(&mut self, doc: &mut Document) -> Option<PixelRect> {
        let new_bounds = self
            .affine
            .mapped_bounds(self.content_bounds, doc.width(), doc.height());
        let region = match (self.last_bounds, new_bounds) {
            (Some(a), Some(b)) => union(Some(a), b),
            (a, b) => a.or(b)?,
        };
        self.render(doc, region);
        self.last_bounds = new_bounds;
        self.touched = Some(union(self.touched, region));
        Some(region)
    }

    fn render(&self, doc: &mut Document, region: PixelRect) {
        let interp = self.interpolation;
        let map = LinearMap::inverse_of(&self.affine);
        let width = doc.width() as usize;
        let rows = region.y..region.bottom();
        let (x0, x1) = (region.x, region.right());
        // Source points outside these bounds sample nothing.
        let cb = self.content_bounds;
        let (lo_x, lo_y) = (cb.x as f32 - 1.0, cb.y as f32 - 1.0);
        let (hi_x, hi_y) = (cb.right() as f32 + 1.0, cb.bottom() as f32 + 1.0);
        let has_selection = self.base_selection.is_some();
        if let Some(layer) = doc.layer_mut(self.layer) {
            parallel_rows(&mut layer.pixels, width * 4, rows.clone(), |y, row| {
                for x in x0..x1 {
                    let src = map.at(x, y);
                    let moved = if src.0 < lo_x || src.1 < lo_y || src.0 > hi_x || src.1 > hi_y {
                        Rgba([0, 0, 0, 0])
                    } else {
                        sample(&self.content, src, interp)
                    };
                    let stay = if has_selection {
                        *self.remainder.get_pixel(x, y)
                    } else {
                        Rgba([0, 0, 0, 0])
                    };
                    let i = x as usize * 4;
                    row[i..i + 4].copy_from_slice(&combine_parts(stay, moved).0);
                }
            });
            if let Some(base_mask) = &self.base_mask {
                if let Some(mask) = layer.mask_mut() {
                    parallel_rows(mask, width, rows.clone(), |y, row| {
                        for x in x0..x1 {
                            let src = map.at(x, y);
                            // Without a selection the whole mask moves; with
                            // one, only where the transformed selection lands.
                            let weight = match &self.base_selection {
                                Some(sel) => sample_gray(sel, src, interp, 0),
                                None => 255,
                            } as f32
                                / 255.0;
                            let m0 = base_mask.get_pixel(x, y)[0] as f32;
                            let mt = sample_gray(base_mask, src, interp, 255) as f32;
                            row[x as usize] = (m0 + (mt - m0) * weight).round() as u8;
                        }
                    });
                }
            }
        }
        if let Some(base_selection) = &self.base_selection {
            // Outside the region the selection keeps its current values.
            let mut sel = doc
                .selection()
                .cloned()
                .unwrap_or_else(|| base_selection.clone());
            parallel_rows(&mut sel, width, rows, |y, row| {
                for x in x0..x1 {
                    row[x as usize] = sample_gray(base_selection, map.at(x, y), interp, 0);
                }
            });
            // The transformed selection may be empty (moved off-canvas).
            let _ = doc.set_selection(Some(sel));
        }
    }

    /// Ends the session, returning the undo step (or `None` if nothing changed)
    /// and the region repainted.
    pub fn commit(self, doc: &Document, label: &'static str) -> Option<TransformPatch> {
        let rect = self.touched?;
        let layer = doc.layer(self.layer)?;
        let crop = |img: &RgbaImage| {
            image::imageops::crop_imm(img, rect.x, rect.y, rect.width, rect.height).to_image()
        };
        let crop_gray = |img: &GrayImage| {
            image::imageops::crop_imm(img, rect.x, rect.y, rect.width, rect.height).to_image()
        };
        let patch = TransformPatch {
            label,
            layer: self.layer,
            rect,
            before: crop(&self.base),
            after: crop(&layer.pixels),
            mask_before: self.base_mask.as_ref().map(crop_gray),
            mask_after: layer.mask().map(crop_gray),
            selection_before: self.base_selection.clone(),
            selection_after: doc.selection().cloned(),
        };
        let changed = patch.before != patch.after
            || patch.mask_before != patch.mask_after
            || patch.selection_before != patch.selection_after;
        changed.then_some(patch)
    }

    /// Restores everything the session changed; returns the region repainted.
    pub fn cancel(self, doc: &mut Document) -> Option<PixelRect> {
        let rect = self.touched?;
        if let Some(layer) = doc.layer_mut(self.layer) {
            let before =
                image::imageops::crop_imm(&self.base, rect.x, rect.y, rect.width, rect.height)
                    .to_image();
            image::imageops::replace(&mut layer.pixels, &before, rect.x as i64, rect.y as i64);
            if let Some(base_mask) = &self.base_mask {
                let _ = layer.set_mask(Some(base_mask.clone()));
            }
        }
        if self.base_selection.is_some() {
            let _ = doc.set_selection(self.base_selection.clone());
        }
        Some(rect)
    }
}

/// The undo step of a committed move, transform, or flip.
#[derive(Debug, Clone)]
pub struct TransformPatch {
    label: &'static str,
    layer: usize,
    rect: PixelRect,
    before: RgbaImage,
    after: RgbaImage,
    mask_before: Option<GrayImage>,
    mask_after: Option<GrayImage>,
    selection_before: Option<GrayImage>,
    selection_after: Option<GrayImage>,
}

impl TransformPatch {
    fn write(&self, doc: &mut Document, after: bool) -> Result<(), CommandError> {
        let (pixels, mask, selection) = if after {
            (&self.after, &self.mask_after, &self.selection_after)
        } else {
            (&self.before, &self.mask_before, &self.selection_before)
        };
        let len = doc.layer_count();
        let layer = doc.layer_mut(self.layer).ok_or(CommandError::NoSuchLayer {
            index: self.layer,
            len,
        })?;
        image::imageops::replace(
            &mut layer.pixels,
            pixels,
            self.rect.x as i64,
            self.rect.y as i64,
        );
        if let (Some(region), Some(layer_mask)) = (mask, layer.mask_mut()) {
            image::imageops::replace(layer_mask, region, self.rect.x as i64, self.rect.y as i64);
        }
        if self.selection_before.is_some() || self.selection_after.is_some() {
            doc.set_selection(selection.clone())
                .map_err(|e| CommandError::Invalid(e.to_string()))?;
        }
        Ok(())
    }
}

impl Command for TransformPatch {
    fn label(&self) -> &str {
        self.label
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        self.write(doc, true)
    }

    fn revert(&mut self, doc: &mut Document) {
        let _ = self.write(doc, false);
    }

    fn dirty_rect(&self, _doc: &Document) -> PixelRect {
        self.rect
    }

    fn memory_cost(&self) -> usize {
        let gray = |m: &Option<GrayImage>| m.as_ref().map_or(0, |m| m.as_raw().len());
        self.before.as_raw().len()
            + self.after.as_raw().len()
            + gray(&self.mask_before)
            + gray(&self.mask_after)
            + gray(&self.selection_before)
            + gray(&self.selection_after)
    }

    fn changes_selection(&self) -> bool {
        self.selection_before != self.selection_after
    }
}

/// Edit → Flip Horizontal / Vertical as one step (exact, nearest sampling).
pub fn flip(doc: &mut Document, horizontal: bool) -> Result<Option<TransformPatch>, CommandError> {
    let bounds = doc
        .selection()
        .and_then(crate::selection::bounds)
        .unwrap_or(doc.bounds());
    let mut session = TransformSession::begin(doc, Interpolation::Nearest, Some(bounds))?;
    let mut affine = session.affine();
    affine.scale = if horizontal { (-1.0, 1.0) } else { (1.0, -1.0) };
    session.set_affine(doc, affine);
    let label = if horizontal {
        "Flip Horizontal"
    } else {
        "Flip Vertical"
    };
    let patch = session.clone().commit(doc, label);
    // The caller records the step through the undo stack, which re-applies it.
    session.cancel(doc);
    Ok(patch)
}

/// What a pointer press on the Free Transform box grabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    Move,
    /// Scale handle at box position (ax, ay) ∈ {−1, 0, 1}² (not both 0).
    Scale(i8, i8),
    Rotate,
}

/// Handle positions in image coordinates: (handle, point).
pub fn handle_points(affine: &Affine, bounds: PixelRect) -> Vec<(Handle, (f32, f32))> {
    let (cx, cy) = (
        bounds.x as f32 + bounds.width as f32 / 2.0,
        bounds.y as f32 + bounds.height as f32 / 2.0,
    );
    let (hx, hy) = (bounds.width as f32 / 2.0, bounds.height as f32 / 2.0);
    let mut out = Vec::new();
    for ay in -1i8..=1 {
        for ax in -1i8..=1 {
            if ax == 0 && ay == 0 {
                continue;
            }
            let p = (cx + ax as f32 * hx, cy + ay as f32 * hy);
            out.push((Handle::Scale(ax, ay), affine.map(p)));
        }
    }
    out
}

/// What is under `pointer` (image coordinates); `tolerance` is the grab
/// radius in image pixels.
pub fn hit_test(
    affine: &Affine,
    bounds: PixelRect,
    pointer: (f32, f32),
    tolerance: f32,
) -> Option<Handle> {
    let near =
        |p: (f32, f32)| ((p.0 - pointer.0).powi(2) + (p.1 - pointer.1).powi(2)).sqrt() <= tolerance;
    if let Some((handle, _)) = handle_points(affine, bounds)
        .into_iter()
        .find(|(_, p)| near(*p))
    {
        return Some(handle);
    }
    let local = affine.inverse(pointer);
    let inside = local.0 >= bounds.x as f32
        && local.1 >= bounds.y as f32
        && local.0 <= bounds.right() as f32
        && local.1 <= bounds.bottom() as f32;
    if inside {
        return Some(Handle::Move);
    }
    // Just outside a corner rotates.
    let corners = affine.corners(bounds);
    corners
        .iter()
        .any(|c| ((c.0 - pointer.0).powi(2) + (c.1 - pointer.1).powi(2)).sqrt() <= tolerance * 4.0)
        .then_some(Handle::Rotate)
}

/// The transform after dragging `handle` from `start` to `now` (image
/// coordinates), starting from `initial`. `shift` keeps proportions when
/// scaling from a corner and snaps rotation to 15°.
pub fn drag(
    initial: &Affine,
    bounds: PixelRect,
    handle: Handle,
    start: (f32, f32),
    now: (f32, f32),
    shift: bool,
) -> Affine {
    let mut a = *initial;
    match handle {
        Handle::Move => {
            a.translate = (
                initial.translate.0 + now.0 - start.0,
                initial.translate.1 + now.1 - start.1,
            );
        }
        Handle::Rotate => {
            let center = initial.map(initial.pivot);
            let angle0 = (start.1 - center.1).atan2(start.0 - center.0);
            let angle1 = (now.1 - center.1).atan2(now.0 - center.0);
            let mut angle = initial.angle + (angle1 - angle0);
            if shift {
                let step = 15f32.to_radians();
                angle = (angle / step).round() * step;
            }
            a.angle = angle;
        }
        Handle::Scale(ax, ay) => {
            let (cx, cy) = initial.pivot;
            let (hx, hy) = (bounds.width as f32 / 2.0, bounds.height as f32 / 2.0);
            let anchor = (cx - ax as f32 * hx, cy - ay as f32 * hy);
            let anchor_world = initial.map(anchor);
            // Pointer relative to the fixed anchor, in the box's unrotated frame.
            let (c, s) = (initial.angle.cos(), initial.angle.sin());
            let (dx, dy) = (now.0 - anchor_world.0, now.1 - anchor_world.1);
            let (lx, ly) = (c * dx + s * dy, -s * dx + c * dy);
            let min_size = 1.0;
            let mut sx = initial.scale.0;
            let mut sy = initial.scale.1;
            if ax != 0 && hx > 0.0 {
                sx = lx / (2.0 * hx * ax as f32);
                if (sx * 2.0 * hx).abs() < min_size {
                    sx = min_size / (2.0 * hx)
                        * sx.signum().max(0.0).max(1.0)
                        * initial.scale.0.signum();
                }
            }
            if ay != 0 && hy > 0.0 {
                sy = ly / (2.0 * hy * ay as f32);
                if (sy * 2.0 * hy).abs() < min_size {
                    sy = min_size / (2.0 * hy) * initial.scale.1.signum();
                }
            }
            if shift && ax != 0 && ay != 0 {
                let f = (sx / initial.scale.0 + sy / initial.scale.1) / 2.0;
                sx = initial.scale.0 * f;
                sy = initial.scale.1 * f;
            }
            a.scale = (sx, sy);
            // Keep the anchor where it was.
            let moved = a.map(anchor);
            a.translate = (
                a.translate.0 + anchor_world.0 - moved.0,
                a.translate.1 + anchor_world.1 - moved.1,
            );
        }
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Layer;
    use crate::undo::UndoStack;

    fn pattern(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            Rgba([
                (x * 37 % 256) as u8,
                (y * 53 % 256) as u8,
                ((x + y) * 11 % 256) as u8,
                255,
            ])
        })
    }

    fn doc_with(pixels: RgbaImage) -> Document {
        let mut doc = Document::new("d", pixels.width(), pixels.height(), 72.0).unwrap();
        doc.add_layer(Layer::new("l", pixels)).unwrap();
        doc
    }

    fn transformed(
        doc: &Document,
        interp: Interpolation,
        edit: impl FnOnce(&mut Affine),
    ) -> Document {
        let mut doc = doc.clone();
        let mut s = TransformSession::begin(&doc, interp, Some(doc.bounds())).unwrap();
        let mut a = s.affine();
        edit(&mut a);
        s.set_affine(&mut doc, a);
        doc
    }

    #[test]
    fn affine_map_and_inverse_round_trip() {
        let a = Affine {
            pivot: (10.0, 5.0),
            translate: (3.0, -2.0),
            scale: (2.0, 0.5),
            angle: 0.7,
        };
        let p = (13.25, 8.5);
        let back = a.inverse(a.map(p));
        assert!((back.0 - p.0).abs() < 1e-4 && (back.1 - p.1).abs() < 1e-4);
        assert!(Affine::identity((0.0, 0.0)).is_identity());
    }

    #[test]
    fn identity_and_whole_pixel_translation_are_exact() {
        let doc = doc_with(pattern(16, 12));
        for interp in [Interpolation::Bilinear, Interpolation::Nearest] {
            assert_eq!(transformed(&doc, interp, |_| {}), doc);
            let moved = transformed(&doc, interp, |a| a.translate = (3.0, 2.0));
            let px = &moved.layer(0).unwrap().pixels;
            assert_eq!(
                px.get_pixel(5, 4),
                doc.layer(0).unwrap().pixels.get_pixel(2, 2)
            );
            assert_eq!(px.get_pixel(0, 0)[3], 0, "vacated area is transparent");
        }
    }

    #[test]
    fn nearest_rotation_by_90_and_scale_2_are_exact() {
        let doc = doc_with(pattern(8, 8));
        let src = &doc.layer(0).unwrap().pixels;
        let rot = transformed(&doc, Interpolation::Nearest, |a| {
            a.angle = std::f32::consts::FRAC_PI_2
        });
        let r = &rot.layer(0).unwrap().pixels;
        // 90° clockwise about the center: (x, y) ← (y, 7 − x).
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(r.get_pixel(x, y), src.get_pixel(y, 7 - x));
            }
        }
        let half = doc_with(pattern(4, 4));
        let mut big = Document::new("b", 8, 8, 72.0).unwrap();
        let mut layer = RgbaImage::new(8, 8);
        image::imageops::replace(&mut layer, &half.layer(0).unwrap().pixels, 2, 2);
        big.add_layer(Layer::new("l", layer)).unwrap();
        let scaled = transformed(&big, Interpolation::Nearest, |a| {
            a.pivot = (4.0, 4.0);
            a.scale = (2.0, 2.0);
        });
        let s = &scaled.layer(0).unwrap().pixels;
        let original = &half.layer(0).unwrap().pixels;
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(s.get_pixel(x, y), original.get_pixel(x / 2, y / 2));
            }
        }
    }

    #[test]
    fn bilinear_produces_intermediate_values() {
        let mut img = RgbaImage::from_pixel(4, 1, Rgba([0, 0, 0, 255]));
        img.put_pixel(2, 0, Rgba([255, 255, 255, 255]));
        img.put_pixel(3, 0, Rgba([255, 255, 255, 255]));
        let v = sample(&img, (2.0, 0.5), Interpolation::Bilinear);
        assert_eq!(v[0], 128);
        assert_eq!(sample(&img, (2.0, 0.5), Interpolation::Nearest)[0], 255);
        // Transparent neighbours don't darken (premultiplied).
        let mut t = RgbaImage::new(2, 1);
        t.put_pixel(0, 0, Rgba([200, 0, 0, 255]));
        let edge = sample(&t, (1.0, 0.5), Interpolation::Bilinear);
        assert_eq!(edge[0], 200);
        assert_eq!(edge[3], 128);
    }

    #[test]
    fn session_moves_selected_content_and_selection() {
        let mut doc = doc_with(RgbaImage::from_pixel(20, 10, Rgba([255, 255, 255, 255])));
        // A red square inside the selection at (2..6, 2..6).
        for y in 2..6 {
            for x in 2..6 {
                doc.layer_mut(0)
                    .unwrap()
                    .pixels
                    .put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        let sel = crate::selection::rect_mask(20, 10, (2.0, 2.0), (6.0, 6.0));
        doc.set_selection(Some(sel)).unwrap();
        let before = doc.clone();
        let mut s = TransformSession::begin(&doc, Interpolation::Bilinear, None).unwrap();
        assert_eq!(s.content_bounds(), PixelRect::new(2, 2, 4, 4));
        let mut a = s.affine();
        a.translate = (10.0, 1.0);
        let region = s.set_affine(&mut doc, a).unwrap();
        assert!(region.width <= 20);
        let px = |d: &Document, x, y| d.layer(0).unwrap().pixels.get_pixel(x, y).0;
        assert_eq!(px(&doc, 13, 4), [255, 0, 0, 255], "moved content");
        assert_eq!(
            px(&doc, 3, 3)[3],
            0,
            "vacated: transparent (selected part lifted)"
        );
        assert_eq!(px(&doc, 0, 0), [255, 255, 255, 255], "unselected stays");
        let moved_sel = crate::selection::bounds(doc.selection().unwrap()).unwrap();
        assert_eq!(moved_sel, PixelRect::new(12, 3, 4, 4));

        let patch = s.clone().commit(&doc, "Move").unwrap();
        let after = doc.clone();
        let mut stack = UndoStack::new();
        stack.execute(&mut doc, Box::new(patch)).unwrap();
        assert_eq!(doc, after);
        stack.undo(&mut doc);
        assert_eq!(doc, before);
        stack.redo(&mut doc);
        assert_eq!(doc, after);

        // Cancel restores.
        let mut d2 = before.clone();
        let mut s2 = TransformSession::begin(&d2, Interpolation::Bilinear, None).unwrap();
        let mut a2 = s2.affine();
        a2.angle = 0.5;
        s2.set_affine(&mut d2, a2);
        s2.cancel(&mut d2);
        assert_eq!(d2, before);
    }

    #[test]
    fn content_moved_off_canvas_and_back_within_a_session_is_kept() {
        let doc = doc_with(pattern(10, 10));
        let mut d = doc.clone();
        let mut s = TransformSession::begin(&d, Interpolation::Nearest, None).unwrap();
        let mut a = s.affine();
        a.translate = (8.0, 0.0);
        s.set_affine(&mut d, a);
        a.translate = (0.0, 0.0);
        s.set_affine(&mut d, a);
        assert_eq!(d, doc);
        // Committed off-canvas pixels are clipped.
        a.translate = (8.0, 0.0);
        s.set_affine(&mut d, a);
        let patch = s.commit(&d, "Move").unwrap();
        let mut stack = UndoStack::new();
        stack.execute(&mut d, Box::new(patch)).unwrap();
        let mut back =
            TransformSession::begin(&d, Interpolation::Nearest, Some(d.bounds())).unwrap();
        let mut b = back.affine();
        b.translate = (-8.0, 0.0);
        back.set_affine(&mut d, b);
        assert_eq!(
            d.layer(0).unwrap().pixels.get_pixel(5, 5)[3],
            0,
            "lost after commit"
        );
    }

    #[test]
    fn layer_mask_moves_with_content() {
        let mut doc = doc_with(pattern(10, 4));
        let mut mask = GrayImage::from_pixel(10, 4, Luma([255]));
        mask.put_pixel(1, 1, Luma([0]));
        doc.layer_mut(0).unwrap().set_mask(Some(mask)).unwrap();
        let moved = transformed(&doc, Interpolation::Nearest, |a| a.translate = (5.0, 0.0));
        let m = moved.layer(0).unwrap().mask().unwrap();
        assert_eq!(m.get_pixel(6, 1)[0], 0);
        assert_eq!(m.get_pixel(1, 1)[0], 255);
    }

    #[test]
    fn flips_are_exact_and_involutive() {
        let mut doc = doc_with(pattern(9, 7));
        let original = doc.clone();
        let mut stack = UndoStack::new();
        let patch = flip(&mut doc, true).unwrap().unwrap();
        assert_eq!(doc, original, "flip() leaves recording to the caller");
        stack.execute(&mut doc, Box::new(patch)).unwrap();
        let px = &doc.layer(0).unwrap().pixels;
        assert_eq!(
            px.get_pixel(0, 3),
            original.layer(0).unwrap().pixels.get_pixel(8, 3)
        );
        assert_eq!(stack.undo_label(), Some("Flip Horizontal"));
        let again = flip(&mut doc, true).unwrap().unwrap();
        stack.execute(&mut doc, Box::new(again)).unwrap();
        assert_eq!(doc, original);
        let v = flip(&mut doc, false).unwrap().unwrap();
        stack.execute(&mut doc, Box::new(v)).unwrap();
        assert_eq!(
            doc.layer(0).unwrap().pixels.get_pixel(2, 0),
            original.layer(0).unwrap().pixels.get_pixel(2, 6)
        );
    }

    #[test]
    fn handles_hit_test_and_drag() {
        let bounds = PixelRect::new(10, 10, 20, 10);
        let a = Affine::identity((20.0, 15.0));
        assert_eq!(
            hit_test(&a, bounds, (30.0, 20.0), 2.0),
            Some(Handle::Scale(1, 1))
        );
        assert_eq!(
            hit_test(&a, bounds, (20.0, 10.5), 2.0),
            Some(Handle::Scale(0, -1))
        );
        assert_eq!(hit_test(&a, bounds, (18.0, 15.0), 2.0), Some(Handle::Move));
        assert_eq!(
            hit_test(&a, bounds, (33.0, 23.0), 2.0),
            Some(Handle::Rotate)
        );
        assert_eq!(hit_test(&a, bounds, (60.0, 60.0), 2.0), None);

        // Dragging the bottom-right corner to (50, 30) doubles the size, top-left fixed.
        let s = drag(
            &a,
            bounds,
            Handle::Scale(1, 1),
            (30.0, 20.0),
            (50.0, 30.0),
            false,
        );
        let tl = s.map((10.0, 10.0));
        let br = s.map((30.0, 20.0));
        assert!((tl.0 - 10.0).abs() < 1e-3 && (tl.1 - 10.0).abs() < 1e-3);
        assert!((br.0 - 50.0).abs() < 1e-3 && (br.1 - 30.0).abs() < 1e-3);

        // Shift keeps proportions.
        let p = drag(
            &a,
            bounds,
            Handle::Scale(1, 1),
            (30.0, 20.0),
            (50.0, 20.0),
            true,
        );
        assert!((p.scale.0 - p.scale.1).abs() < 1e-4);

        // Edge handle changes one axis only.
        let e = drag(
            &a,
            bounds,
            Handle::Scale(1, 0),
            (30.0, 15.0),
            (40.0, 99.0),
            false,
        );
        assert!((e.scale.0 - 1.5).abs() < 1e-4 && e.scale.1 == 1.0);

        // Rotation by a quarter turn around the center; Shift snaps.
        let r = drag(
            &a,
            bounds,
            Handle::Rotate,
            (40.0, 15.0),
            (20.0, 35.0),
            false,
        );
        assert!((r.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        let snapped = drag(&a, bounds, Handle::Rotate, (40.0, 15.0), (40.0, 17.0), true);
        assert_eq!(snapped.angle, 0.0);

        let m = drag(&a, bounds, Handle::Move, (15.0, 15.0), (18.0, 11.0), false);
        assert_eq!(m.translate, (3.0, -4.0));
    }

    /// Release-mode benchmark: `cargo test --release --lib bench_transform -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn bench_transform_updates_1024() {
        let mut doc = doc_with(pattern(1024, 1024));
        let mut s = TransformSession::begin(&doc, Interpolation::Bilinear, None).unwrap();
        let start = std::time::Instant::now();
        let n = 30;
        for i in 0..n {
            let mut a = s.affine();
            a.angle = i as f32 * 0.01;
            a.scale = (1.0 - i as f32 * 0.005, 1.0 - i as f32 * 0.005);
            a.translate = (i as f32 * 2.0, i as f32);
            s.set_affine(&mut doc, a);
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("transform update on 1024x1024: {ms:.2} ms");
        if !cfg!(debug_assertions) {
            assert!(ms < 16.0, "{ms:.2} ms per update");
        }
    }

    #[test]
    fn zero_move_under_soft_selection_is_exact() {
        let mut doc = doc_with(pattern(12, 8));
        let mut sel = GrayImage::new(12, 8);
        for (x, _, v) in sel.enumerate_pixels_mut() {
            *v = Luma([(x * 23) as u8]);
        }
        doc.set_selection(Some(sel)).unwrap();
        let before = doc.clone();
        let mut s = TransformSession::begin(&doc, Interpolation::Bilinear, None).unwrap();
        let a = s.affine();
        s.set_affine(&mut doc, a);
        assert_eq!(doc, before);
        assert!(s.commit(&doc, "Move").is_none(), "no change, no step");
    }
}
