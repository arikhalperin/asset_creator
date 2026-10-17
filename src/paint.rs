//! Painting: brush dabs, stroke interpolation, the live stroke engine, and
//! the bucket and gradient fills. Window-independent.

use image::{GrayImage, Luma, Rgba, RgbaImage};

use crate::document::{blend_over, Document, PixelRect};
use crate::selection;
use crate::undo::{apply_masked, blend_by_mask, Command, CommandError, SavedRegion};

pub const MIN_BRUSH_SIZE: f32 = 1.0;
pub const MAX_BRUSH_SIZE: f32 = 500.0;
/// Dab spacing as a fraction of the brush size.
pub const SPACING: f32 = 0.15;
/// Smallest dab spacing in pixels.
pub const MIN_SPACING: f32 = 0.5;

/// Size, hardness, opacity, and Pixel mode of a round brush.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrushSettings {
    /// Diameter in image pixels (1–500).
    pub size: f32,
    /// 0.0–1.0.
    pub hardness: f32,
    /// 0.01–1.0.
    pub opacity: f32,
    /// Hard whole-pixel dabs.
    pub pixel: bool,
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self {
            size: 20.0,
            hardness: 1.0,
            opacity: 1.0,
            pixel: false,
        }
    }
}

impl BrushSettings {
    /// Distance between dabs along a stroke.
    pub fn spacing(&self) -> f32 {
        if self.pixel {
            MIN_SPACING
        } else {
            (SPACING * self.size).max(MIN_SPACING)
        }
    }

    /// The size after pressing `]` (`grow`) or `[`.
    pub fn stepped_size(&self, grow: bool) -> f32 {
        let step = (self.size * 0.2).round().max(1.0);
        let size = if grow {
            self.size + step
        } else {
            self.size - step
        };
        size.clamp(MIN_BRUSH_SIZE, MAX_BRUSH_SIZE)
    }
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Anti-aliased coverage (0–1) of a pixel whose center is `d` pixels from the
/// center of a dab of radius `r` and hardness `h`.
pub fn dab_coverage(d: f32, r: f32, hardness: f32) -> f32 {
    let w = (r * (1.0 - hardness)).max(1.0);
    let c = ((r - d) / w).clamp(0.0, 1.0);
    if hardness < 1.0 {
        smoothstep(c)
    } else {
        c
    }
}

/// Coverage accumulated by one stroke (maximum over its dabs).
#[derive(Debug, Clone)]
pub struct StrokeMask {
    coverage: GrayImage,
    bounds: Option<PixelRect>,
}

impl StrokeMask {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            coverage: GrayImage::new(width, height),
            bounds: None,
        }
    }

    pub fn coverage(&self) -> &GrayImage {
        &self.coverage
    }

    /// Union of everything stamped so far.
    pub fn bounds(&self) -> Option<PixelRect> {
        self.bounds
    }

    /// Erases everything stamped; returns the region that was covered.
    pub fn clear(&mut self) -> Option<PixelRect> {
        let rect = self.bounds.take()?;
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                self.coverage.put_pixel(x, y, Luma([0]));
            }
        }
        Some(rect)
    }

    /// Stamps a dab at `center` (image coordinates); returns the pixels it touched.
    pub fn stamp(&mut self, center: (f32, f32), brush: &BrushSettings) -> Option<PixelRect> {
        let (w, h) = self.coverage.dimensions();
        let r = brush.size / 2.0;
        let (cx, cy) = if brush.pixel {
            (center.0.floor() + 0.5, center.1.floor() + 0.5)
        } else {
            center
        };
        let x0 = (cx - r - 1.0).floor().max(0.0) as u32;
        let y0 = (cy - r - 1.0).floor().max(0.0) as u32;
        let x1 = ((cx + r + 1.0).ceil().max(0.0) as u32).min(w);
        let y1 = ((cy + r + 1.0).ceil().max(0.0) as u32).min(h);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let mut touched: Option<PixelRect> = None;
        for y in y0..y1 {
            for x in x0..x1 {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let c = if brush.pixel {
                    if d <= r.max(0.5) {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    dab_coverage(d, r, brush.hardness)
                };
                let v = (c * 255.0).round() as u8;
                if v == 0 {
                    continue;
                }
                let px = self.coverage.get_pixel_mut(x, y);
                if v > px[0] {
                    px[0] = v;
                }
                touched = Some(union(touched, PixelRect::new(x, y, 1, 1)));
            }
        }
        if let Some(t) = touched {
            self.bounds = Some(union(self.bounds, t));
        }
        touched
    }
}

/// Smallest rectangle containing both.
pub fn union(a: Option<PixelRect>, b: PixelRect) -> PixelRect {
    match a {
        None => b,
        Some(a) => {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            PixelRect::new(
                x,
                y,
                a.right().max(b.right()) - x,
                a.bottom().max(b.bottom()) - y,
            )
        }
    }
}

type Point = (f32, f32);

fn lerp(a: Point, b: Point, t: f32) -> Point {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

fn dist(a: Point, b: Point) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Uniform Catmull-Rom point between `p1` and `p2` at `t`.
pub fn catmull_rom(p0: Point, p1: Point, p2: Point, p3: Point, t: f32) -> Point {
    let t2 = t * t;
    let t3 = t2 * t;
    let f = |a: f32, b: f32, c: f32, d: f32| {
        0.5 * ((2.0 * b)
            + (-a + c) * t
            + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
            + (-a + 3.0 * b - 3.0 * c + d) * t3)
    };
    (f(p0.0, p1.0, p2.0, p3.0), f(p0.1, p1.1, p2.1, p3.1))
}

/// Turns pointer samples into evenly spaced dab centers along a Catmull-Rom
/// curve. A segment is emitted once the sample after it is known (for the
/// tangent); `finish` emits the last one.
#[derive(Debug, Clone)]
pub struct StrokeSampler {
    spacing: f32,
    points: Vec<Point>,
    emitted_segments: usize,
    /// Distance travelled since the last dab.
    carry: f32,
    last: Option<Point>,
}

impl StrokeSampler {
    pub fn new(spacing: f32) -> Self {
        Self {
            spacing: spacing.max(0.05),
            points: Vec::new(),
            emitted_segments: 0,
            carry: 0.0,
            last: None,
        }
    }

    /// Adds a pointer sample; returns dab centers that are now final.
    pub fn add_point(&mut self, p: Point) -> Vec<Point> {
        if self.points.last() == Some(&p) {
            return Vec::new();
        }
        self.points.push(p);
        let mut dabs = Vec::new();
        if self.points.len() == 1 {
            self.last = Some(p);
            dabs.push(p);
            return dabs;
        }
        // Segment i (points[i] → points[i+1]) is final once points[i+2] exists.
        while self.emitted_segments + 2 < self.points.len() {
            self.emit_segment(self.emitted_segments, &mut dabs);
            self.emitted_segments += 1;
        }
        dabs
    }

    /// Emits the remaining segments; returns their dab centers.
    pub fn finish(&mut self) -> Vec<Point> {
        let mut dabs = Vec::new();
        while self.emitted_segments + 1 < self.points.len() {
            self.emit_segment(self.emitted_segments, &mut dabs);
            self.emitted_segments += 1;
        }
        dabs
    }

    /// Dab centers for the not-yet-final last segment, as it would be drawn
    /// if the stroke ended now. Does not change the sampler.
    pub fn preview_tail(&self) -> Vec<Point> {
        if self.emitted_segments + 1 >= self.points.len() {
            return Vec::new();
        }
        // Only the points the remaining segments depend on.
        let start = self.emitted_segments.saturating_sub(1);
        let mut tail = StrokeSampler {
            spacing: self.spacing,
            points: self.points[start..].to_vec(),
            emitted_segments: self.emitted_segments - start,
            carry: self.carry,
            last: self.last,
        };
        tail.finish()
    }

    /// The last pointer sample.
    pub fn end_point(&self) -> Option<Point> {
        self.points.last().copied()
    }

    fn emit_segment(&mut self, i: usize, dabs: &mut Vec<Point>) {
        let pts = &self.points;
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p0 = if i == 0 { p1 } else { pts[i - 1] };
        let p3 = pts.get(i + 2).copied().unwrap_or(p2);
        // Flatten the curve finely, then walk it at the dab spacing.
        let steps =
            ((dist(p1, p2) / (self.spacing * 0.5).max(0.25)).ceil() as usize).clamp(1, 10_000);
        let mut prev = self.last.unwrap_or(p1);
        for s in 1..=steps {
            let q = catmull_rom(p0, p1, p2, p3, s as f32 / steps as f32);
            let mut seg = dist(prev, q);
            let mut from = prev;
            while self.carry + seg >= self.spacing {
                let need = self.spacing - self.carry;
                let t = need / seg;
                let dab = lerp(from, q, t);
                dabs.push(dab);
                from = dab;
                seg -= need;
                self.carry = 0.0;
            }
            self.carry += seg;
            prev = q;
        }
        self.last = Some(prev);
    }
}

/// What a stroke does to the pixels it covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrokeKind {
    Brush {
        color: [u8; 4],
    },
    Eraser,
    /// Copies from `source = pixel + offset` in the stroke's starting snapshot.
    Clone {
        offset: (f32, f32),
    },
}

impl StrokeKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Brush { .. } => "Brush Stroke",
            Self::Eraser => "Eraser Stroke",
            Self::Clone { .. } => "Clone Stamp",
        }
    }
}

/// Brush rule: move toward `color` by `amount`/255.
pub fn brush_pixel(old: Rgba<u8>, color: Rgba<u8>, amount: u8) -> Rgba<u8> {
    blend_by_mask(old, color, amount)
}

/// Eraser rule: scale alpha by `1 − amount/255`.
pub fn eraser_pixel(old: Rgba<u8>, amount: u8) -> Rgba<u8> {
    let alpha = old[3] as f32 * (1.0 - amount as f32 / 255.0);
    Rgba([old[0], old[1], old[2], alpha.round() as u8])
}

/// A stroke in progress: paints live into the document from a snapshot of
/// the layer taken when it began.
#[derive(Debug, Clone)]
pub struct ActiveStroke {
    kind: StrokeKind,
    layer: usize,
    brush: BrushSettings,
    snapshot: RgbaImage,
    mask: StrokeMask,
    /// Provisional dabs of the last, not-yet-final segment (redrawn each update).
    tail: StrokeMask,
    sampler: StrokeSampler,
}

impl ActiveStroke {
    /// Starts a stroke at `start`, painting its first dab. Returns the stroke
    /// and the region it changed.
    pub fn begin(
        doc: &mut Document,
        layer: usize,
        kind: StrokeKind,
        brush: BrushSettings,
        start: Point,
    ) -> Result<(Self, Option<PixelRect>), CommandError> {
        let len = doc.layer_count();
        let snapshot = doc
            .layer(layer)
            .ok_or(CommandError::NoSuchLayer { index: layer, len })?
            .pixels
            .clone();
        let mut stroke = Self {
            kind,
            layer,
            brush,
            mask: StrokeMask::new(doc.width(), doc.height()),
            tail: StrokeMask::new(doc.width(), doc.height()),
            sampler: StrokeSampler::new(brush.spacing()),
            snapshot,
        };
        let dabs = stroke.sampler.add_point(start);
        let dirty = stroke.paint(doc, &dabs, true);
        Ok((stroke, dirty))
    }

    /// Adds a pointer sample; returns the region repainted.
    pub fn add_point(&mut self, doc: &mut Document, p: Point) -> Option<PixelRect> {
        let dabs = self.sampler.add_point(p);
        self.paint(doc, &dabs, true)
    }

    /// Paints the final segment and returns the undo command (or `None` if
    /// nothing changed), plus the region repainted and the stroke's end point.
    pub fn finish(
        mut self,
        doc: &mut Document,
    ) -> (Option<PixelPatch>, Option<PixelRect>, Option<Point>) {
        let dabs = self.sampler.finish();
        let dirty = self.paint(doc, &dabs, false);
        let end = self.sampler.end_point();
        let patch = self.mask.bounds().and_then(|rect| {
            let rect = rect.intersect(doc.editable_bounds())?;
            let layer = doc.layer(self.layer)?;
            let before =
                image::imageops::crop_imm(&self.snapshot, rect.x, rect.y, rect.width, rect.height)
                    .to_image();
            let after =
                image::imageops::crop_imm(&layer.pixels, rect.x, rect.y, rect.width, rect.height)
                    .to_image();
            (before != after)
                .then(|| PixelPatch::new(self.kind.label(), self.layer, rect, before, after))
        });
        (patch, dirty, end)
    }

    /// Restores the pixels the stroke changed; returns that region.
    pub fn cancel(self, doc: &mut Document) -> Option<PixelRect> {
        let rect = match (self.mask.bounds(), self.tail.bounds()) {
            (Some(a), Some(b)) => union(Some(a), b),
            (a, b) => a.or(b)?,
        };
        let before =
            image::imageops::crop_imm(&self.snapshot, rect.x, rect.y, rect.width, rect.height)
                .to_image();
        if let Some(layer) = doc.layer_mut(self.layer) {
            image::imageops::replace(&mut layer.pixels, &before, rect.x as i64, rect.y as i64);
        }
        Some(rect)
    }

    pub fn kind(&self) -> StrokeKind {
        self.kind
    }

    /// Stamps final `dabs`, redraws the provisional tail (when `with_tail`),
    /// and recomputes the pixels whose coverage changed.
    fn paint(&mut self, doc: &mut Document, dabs: &[Point], with_tail: bool) -> Option<PixelRect> {
        let mut region: Option<PixelRect> = self.tail.clear();
        for dab in dabs {
            if let Some(r) = self.mask.stamp(*dab, &self.brush) {
                region = Some(union(region, r));
            }
        }
        if with_tail {
            for dab in self.sampler.preview_tail() {
                if let Some(r) = self.tail.stamp(dab, &self.brush) {
                    region = Some(union(region, r));
                }
            }
        }
        let rect = region?;
        let opacity = self.brush.opacity.clamp(0.0, 1.0);
        let (w, h) = (doc.width() as i64, doc.height() as i64);
        let (selection, layer) = doc.selection_and_layer_mut(self.layer);
        let layer = layer?;
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                let cov = self.mask.coverage().get_pixel(x, y)[0]
                    .max(self.tail.coverage().get_pixel(x, y)[0]);
                let amount = (cov as f32 * opacity).round() as u8;
                let old = *self.snapshot.get_pixel(x, y);
                let painted = match self.kind {
                    StrokeKind::Brush { color } => brush_pixel(old, Rgba(color), amount),
                    StrokeKind::Eraser => eraser_pixel(old, amount),
                    StrokeKind::Clone { offset } => {
                        let sx = (x as f32 + offset.0).round() as i64;
                        let sy = (y as f32 + offset.1).round() as i64;
                        if sx < 0 || sy < 0 || sx >= w || sy >= h {
                            old
                        } else {
                            blend_by_mask(
                                old,
                                *self.snapshot.get_pixel(sx as u32, sy as u32),
                                amount,
                            )
                        }
                    }
                };
                let sel = selection.map_or(255, |m| m.get_pixel(x, y)[0]);
                layer
                    .pixels
                    .put_pixel(x, y, blend_by_mask(old, painted, sel));
            }
        }
        Some(rect)
    }
}

/// An undo step that swaps a region of a layer between two recorded states.
#[derive(Debug, Clone)]
pub struct PixelPatch {
    label: &'static str,
    layer: usize,
    rect: PixelRect,
    before: RgbaImage,
    after: RgbaImage,
}

impl PixelPatch {
    pub fn new(
        label: &'static str,
        layer: usize,
        rect: PixelRect,
        before: RgbaImage,
        after: RgbaImage,
    ) -> Self {
        Self {
            label,
            layer,
            rect,
            before,
            after,
        }
    }

    fn write(&self, doc: &mut Document, pixels: &RgbaImage) -> Result<(), CommandError> {
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
        Ok(())
    }
}

impl Command for PixelPatch {
    fn label(&self) -> &str {
        self.label
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        let after = self.after.clone();
        self.write(doc, &after)
    }

    fn revert(&mut self, doc: &mut Document) {
        let before = self.before.clone();
        let _ = self.write(doc, &before);
    }

    fn dirty_rect(&self, _doc: &Document) -> PixelRect {
        self.rect
    }

    fn memory_cost(&self) -> usize {
        self.before.as_raw().len() + self.after.as_raw().len()
    }
}

/// Paint Bucket: fills the magic-wand region at `seed` with `color` at `opacity`.
pub struct BucketFill {
    layer: usize,
    seed: (u32, u32),
    color: Rgba<u8>,
    tolerance: u8,
    contiguous: bool,
    opacity: f32,
    saved: Option<SavedRegion>,
}

impl BucketFill {
    pub fn new(
        layer: usize,
        seed: (u32, u32),
        color: [u8; 4],
        tolerance: u8,
        contiguous: bool,
        opacity: f32,
    ) -> Self {
        Self {
            layer,
            seed,
            color: Rgba(color),
            tolerance,
            contiguous,
            opacity,
            saved: None,
        }
    }
}

impl Command for BucketFill {
    fn label(&self) -> &str {
        "Paint Bucket"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        let len = doc.layer_count();
        let layer = doc.layer(self.layer).ok_or(CommandError::NoSuchLayer {
            index: self.layer,
            len,
        })?;
        if self.seed.0 >= doc.width() || self.seed.1 >= doc.height() {
            return Err(CommandError::Invalid("click outside the image".to_owned()));
        }
        let region =
            selection::magic_wand(&layer.pixels, self.seed, self.tolerance, self.contiguous);
        let amount = (self.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let color = self.color;
        self.saved = Some(apply_masked(doc, self.layer, |x, y, old| {
            if region.get_pixel(x, y)[0] == 0 {
                old
            } else {
                blend_by_mask(old, color, amount)
            }
        })?);
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) {
        if let Some(saved) = self.saved.take() {
            saved.restore(doc);
        }
    }

    fn dirty_rect(&self, doc: &Document) -> PixelRect {
        self.saved
            .as_ref()
            .map_or(doc.editable_bounds(), |s| s.rect)
    }

    fn memory_cost(&self) -> usize {
        self.saved.as_ref().map_or(0, SavedRegion::bytes)
    }
}

/// Gradient shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradientKind {
    #[default]
    Linear,
    Radial,
}

/// Gradient end colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GradientColors {
    #[default]
    ForegroundToBackground,
    ForegroundToTransparent,
}

/// Position (0–1) of the pixel center `p` along the gradient.
pub fn gradient_position(kind: GradientKind, start: Point, end: Point, p: Point) -> f32 {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let len2 = dx * dx + dy * dy;
    if len2 <= 0.0 {
        return 0.0;
    }
    let t = match kind {
        GradientKind::Linear => ((p.0 - start.0) * dx + (p.1 - start.1) * dy) / len2,
        GradientKind::Radial => dist(start, p) / len2.sqrt(),
    };
    t.clamp(0.0, 1.0)
}

/// Interpolates two colors in premultiplied RGBA.
pub fn mix_premultiplied(a: Rgba<u8>, b: Rgba<u8>, t: f32) -> Rgba<u8> {
    let aa = a[3] as f32 / 255.0;
    let ba = b[3] as f32 / 255.0;
    let alpha = aa + (ba - aa) * t;
    if alpha <= 0.0 {
        return Rgba([0, 0, 0, 0]);
    }
    let mut out = [0u8; 4];
    for c in 0..3 {
        let v = (a[c] as f32 * aa + (b[c] as f32 * ba - a[c] as f32 * aa) * t) / alpha;
        out[c] = v.round().clamp(0.0, 255.0) as u8;
    }
    out[3] = (alpha * 255.0).round() as u8;
    Rgba(out)
}

/// Gradient tool: paints a linear or radial gradient over the layer.
pub struct GradientFill {
    layer: usize,
    start: Point,
    end: Point,
    kind: GradientKind,
    from: Rgba<u8>,
    to: Rgba<u8>,
    opacity: f32,
    saved: Option<SavedRegion>,
}

impl GradientFill {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        layer: usize,
        start: Point,
        end: Point,
        kind: GradientKind,
        colors: GradientColors,
        foreground: [u8; 4],
        background: [u8; 4],
        opacity: f32,
    ) -> Self {
        let from = Rgba(foreground);
        let to = match colors {
            GradientColors::ForegroundToBackground => Rgba(background),
            GradientColors::ForegroundToTransparent => {
                Rgba([foreground[0], foreground[1], foreground[2], 0])
            }
        };
        Self {
            layer,
            start,
            end,
            kind,
            from,
            to,
            opacity,
            saved: None,
        }
    }

    /// The gradient color at pixel `(x, y)` before opacity.
    pub fn color_at(&self, x: u32, y: u32) -> Rgba<u8> {
        let t = gradient_position(
            self.kind,
            self.start,
            self.end,
            (x as f32 + 0.5, y as f32 + 0.5),
        );
        mix_premultiplied(self.from, self.to, t)
    }
}

impl Command for GradientFill {
    fn label(&self) -> &str {
        "Gradient"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        if dist(self.start, self.end) < 1.0 {
            return Err(CommandError::Invalid("gradient drag too short".to_owned()));
        }
        let opacity = self.opacity.clamp(0.0, 1.0);
        let gradient = GradientFill {
            saved: None,
            ..*self
        };
        self.saved = Some(apply_masked(doc, self.layer, |x, y, old| {
            blend_over(old, gradient.color_at(x, y), opacity)
        })?);
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) {
        if let Some(saved) = self.saved.take() {
            saved.restore(doc);
        }
    }

    fn dirty_rect(&self, doc: &Document) -> PixelRect {
        self.saved
            .as_ref()
            .map_or(doc.editable_bounds(), |s| s.rect)
    }

    fn memory_cost(&self) -> usize {
        self.saved.as_ref().map_or(0, SavedRegion::bytes)
    }
}

/// The pixel the eyedropper reads at `(x, y)`: from the visible composite or the active layer.
pub fn sample_color(
    doc: &Document,
    layer: usize,
    x: u32,
    y: u32,
    all_layers: bool,
) -> Option<[u8; 4]> {
    if x >= doc.width() || y >= doc.height() {
        return None;
    }
    if all_layers {
        Some(
            doc.composite_region(PixelRect::new(x, y, 1, 1))
                .get_pixel(0, 0)
                .0,
        )
    } else {
        doc.layer(layer).map(|l| l.pixels.get_pixel(x, y).0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Layer;
    use crate::undo::UndoStack;

    fn white_doc(w: u32, h: u32) -> Document {
        let mut doc = Document::new("d", w, h, 72.0).unwrap();
        doc.add_layer(Layer::new(
            "bg",
            RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255])),
        ))
        .unwrap();
        doc
    }

    fn brush(size: f32) -> BrushSettings {
        BrushSettings {
            size,
            ..Default::default()
        }
    }

    fn stroke(
        doc: &mut Document,
        kind: StrokeKind,
        b: BrushSettings,
        points: &[Point],
    ) -> Option<PixelPatch> {
        let (mut s, _) = ActiveStroke::begin(doc, 0, kind, b, points[0]).unwrap();
        for p in &points[1..] {
            s.add_point(doc, *p);
        }
        s.finish(doc).0
    }

    const BLACK: StrokeKind = StrokeKind::Brush {
        color: [0, 0, 0, 255],
    };

    #[test]
    fn coverage_hard_edge_is_antialiased() {
        // 100% hardness: solid inside, 1-px ramp at the edge.
        assert_eq!(dab_coverage(0.0, 10.0, 1.0), 1.0);
        assert_eq!(dab_coverage(9.0, 10.0, 1.0), 1.0);
        assert!((dab_coverage(9.5, 10.0, 1.0) - 0.5).abs() < 1e-6);
        assert_eq!(dab_coverage(10.0, 10.0, 1.0), 0.0);
        // Soft brush: wide smooth falloff.
        let soft: Vec<f32> = (0..=10)
            .map(|d| dab_coverage(d as f32, 10.0, 0.0))
            .collect();
        assert!(soft.windows(2).all(|w| w[0] >= w[1]));
        assert!(soft[5] > 0.3 && soft[5] < 0.7, "{soft:?}");
        assert_eq!(soft[10], 0.0);
        let half = dab_coverage(7.0, 10.0, 0.5);
        assert!(half > 0.0 && half < 1.0);
    }

    #[test]
    fn stamp_has_partial_edges_unless_pixel_mode() {
        let mut m = StrokeMask::new(30, 30);
        m.stamp((15.0, 15.0), &brush(10.0));
        let partial = m.coverage().iter().filter(|v| **v > 0 && **v < 255).count();
        assert!(partial > 8, "anti-aliased edge pixels: {partial}");
        assert_eq!(m.coverage().get_pixel(14, 14)[0], 255);

        let mut p = StrokeMask::new(30, 30);
        p.stamp(
            (15.3, 15.7),
            &BrushSettings {
                pixel: true,
                ..brush(10.0)
            },
        );
        assert!(p.coverage().iter().all(|v| *v == 0 || *v == 255));
        let mut one = StrokeMask::new(5, 5);
        one.stamp(
            (2.9, 1.1),
            &BrushSettings {
                pixel: true,
                ..brush(1.0)
            },
        );
        assert_eq!(one.coverage().iter().filter(|v| **v == 255).count(), 1);
        assert_eq!(one.coverage().get_pixel(2, 1)[0], 255);
    }

    #[test]
    fn sampler_spaces_dabs_evenly_and_finishes() {
        let mut s = StrokeSampler::new(2.0);
        let mut dabs = s.add_point((0.0, 0.0));
        dabs.extend(s.add_point((10.0, 0.0)));
        assert_eq!(dabs.len(), 1, "segment waits for the next sample");
        dabs.extend(s.add_point((20.0, 0.0)));
        dabs.extend(s.finish());
        // 0, 2, 4, ..., 20
        assert_eq!(dabs.len(), 11, "{dabs:?}");
        for w in dabs.windows(2) {
            assert!((dist(w[0], w[1]) - 2.0).abs() < 0.05, "{w:?}");
        }
        assert_eq!(s.end_point(), Some((20.0, 0.0)));
    }

    #[test]
    fn catmull_rom_passes_through_samples_and_curves() {
        let (p0, p1, p2, p3) = ((0.0, 0.0), (10.0, 0.0), (20.0, 10.0), (20.0, 20.0));
        assert_eq!(catmull_rom(p0, p1, p2, p3, 0.0), p1);
        let end = catmull_rom(p0, p1, p2, p3, 1.0);
        assert!(dist(end, p2) < 1e-4);
        // Mid-point bulges away from the straight chord.
        let mid = catmull_rom(p0, p1, p2, p3, 0.5);
        let chord_mid = lerp(p1, p2, 0.5);
        assert!(dist(mid, chord_mid) > 0.5);
    }

    #[test]
    fn sparse_fast_stroke_has_no_gaps() {
        let mut doc = white_doc(600, 120);
        // Samples 200 px apart, as from a very fast mouse.
        let pts = [(20.0, 60.0), (220.0, 40.0), (420.0, 80.0), (580.0, 60.0)];
        stroke(&mut doc, BLACK, brush(6.0), &pts);
        let px = &doc.layer(0).unwrap().pixels;
        // Every column between the ends has painted pixels.
        for x in 20..580 {
            let painted = (0..120).any(|y| px.get_pixel(x, y)[0] < 128);
            assert!(painted, "gap at column {x}");
        }
    }

    #[test]
    fn opacity_is_capped_within_a_stroke() {
        let mut doc = white_doc(100, 40);
        let half = BrushSettings {
            opacity: 0.5,
            ..brush(10.0)
        };
        // Back-and-forth over the same pixels.
        stroke(
            &mut doc,
            BLACK,
            half,
            &[(10.0, 20.0), (90.0, 20.0), (10.0, 20.0), (90.0, 20.0)],
        );
        let v = doc.layer(0).unwrap().pixels.get_pixel(50, 20)[0];
        assert!((v as i32 - 128).abs() <= 1, "{v}");
        // A second stroke builds up.
        stroke(&mut doc, BLACK, half, &[(10.0, 20.0), (90.0, 20.0)]);
        let v2 = doc.layer(0).unwrap().pixels.get_pixel(50, 20)[0];
        assert!((v2 as i32 - 64).abs() <= 2, "{v2}");
    }

    #[test]
    fn pixel_mode_one_pixel_line_is_connected_and_hard() {
        let mut doc = white_doc(40, 40);
        let b = BrushSettings {
            pixel: true,
            ..brush(1.0)
        };
        stroke(&mut doc, BLACK, b, &[(2.5, 2.5), (30.5, 20.5)]);
        let px = &doc.layer(0).unwrap().pixels;
        assert!(px.pixels().all(|p| p[0] == 0 || p[0] == 255));
        // 8-connected: every painted column has a painted neighbour column.
        let rows: Vec<Vec<u32>> = (2..=30)
            .map(|x| (0..40).filter(|y| px.get_pixel(x, *y)[0] == 0).collect())
            .collect();
        assert!(rows.iter().all(|r| !r.is_empty()));
        for w in rows.windows(2) {
            let near = w[0]
                .iter()
                .any(|a| w[1].iter().any(|b| a.abs_diff(*b) <= 1));
            assert!(near, "{w:?}");
        }
        // Thin: at most 2 pixels per column.
        assert!(rows.iter().all(|r| r.len() <= 2));
    }

    #[test]
    fn eraser_reduces_alpha() {
        let mut doc = white_doc(40, 40);
        stroke(&mut doc, StrokeKind::Eraser, brush(10.0), &[(20.0, 20.0)]);
        let px = &doc.layer(0).unwrap().pixels;
        assert_eq!(px.get_pixel(20, 20).0, [255, 255, 255, 0]);
        assert_eq!(px.get_pixel(0, 0)[3], 255);
        assert_eq!(
            eraser_pixel(Rgba([9, 8, 7, 200]), 128),
            Rgba([9, 8, 7, 100])
        );
    }

    #[test]
    fn strokes_respect_selection_and_undo_exactly() {
        let mut doc = white_doc(60, 20);
        let mut mask = GrayImage::new(60, 20);
        for y in 0..20 {
            for x in 0..30 {
                mask.put_pixel(x, y, Luma([255]));
            }
            mask.put_pixel(30, y, Luma([128]));
        }
        doc.set_selection(Some(mask)).unwrap();
        let original = doc.clone();
        let patch = stroke(&mut doc, BLACK, brush(30.0), &[(5.0, 10.0), (55.0, 10.0)]).unwrap();
        {
            let px = &doc.layer(0).unwrap().pixels;
            assert_eq!(px.get_pixel(15, 10).0, [0, 0, 0, 255]);
            assert_eq!(
                px.get_pixel(45, 10).0,
                [255, 255, 255, 255],
                "unselected untouched"
            );
            let partial = px.get_pixel(30, 10)[0];
            assert!((partial as i32 - 127).abs() <= 2, "{partial}");
        }
        let painted = doc.clone();
        assert_eq!(patch.label(), "Brush Stroke");
        assert!(
            patch.dirty_rect(&doc).right() <= 31,
            "limited to editable bounds"
        );

        let mut stack = UndoStack::new();
        stack.execute(&mut doc, Box::new(patch)).unwrap();
        assert_eq!(doc, painted);
        stack.undo(&mut doc);
        assert_eq!(doc, original);
        stack.redo(&mut doc);
        assert_eq!(doc, painted);
    }

    #[test]
    fn tail_is_shown_live_and_replaced_by_final_curve() {
        let mut doc = white_doc(200, 60);
        let (mut s, _) = ActiveStroke::begin(&mut doc, 0, BLACK, brush(6.0), (10.0, 30.0)).unwrap();
        s.add_point(&mut doc, (150.0, 30.0));
        // The last segment is not final yet, but it is already visible.
        assert_eq!(
            doc.layer(0).unwrap().pixels.get_pixel(100, 30).0,
            [0, 0, 0, 255]
        );
        // Turning the stroke upward replaces the provisional tail.
        s.add_point(&mut doc, (150.0, 5.0));
        let (patch, _, _) = s.finish(&mut doc);
        assert!(patch.is_some());
        let mut reference = white_doc(200, 60);
        stroke(
            &mut reference,
            BLACK,
            brush(6.0),
            &[(10.0, 30.0), (150.0, 30.0), (150.0, 5.0)],
        );
        assert_eq!(doc, reference, "no leftovers from the provisional tail");
    }

    #[test]
    fn cancel_restores_pixels() {
        let mut doc = white_doc(40, 40);
        let original = doc.clone();
        let (mut s, _) = ActiveStroke::begin(&mut doc, 0, BLACK, brush(8.0), (5.0, 5.0)).unwrap();
        s.add_point(&mut doc, (30.0, 30.0));
        s.add_point(&mut doc, (35.0, 5.0));
        assert_ne!(doc, original);
        s.cancel(&mut doc);
        assert_eq!(doc, original);
    }

    #[test]
    fn clone_copies_from_snapshot_with_offset() {
        let mut doc = Document::new("d", 40, 10, 72.0).unwrap();
        let pixels = RgbaImage::from_fn(40, 10, |x, _| {
            if x < 10 {
                Rgba([200, 0, 0, 255])
            } else {
                Rgba([0, 0, 200, 255])
            }
        });
        doc.add_layer(Layer::new("l", pixels)).unwrap();
        // Paint at x≈25 copying from x≈5 (offset -20).
        stroke(
            &mut doc,
            StrokeKind::Clone {
                offset: (-20.0, 0.0),
            },
            brush(4.0),
            &[(25.0, 5.0), (27.0, 5.0)],
        );
        let px = &doc.layer(0).unwrap().pixels;
        assert_eq!(px.get_pixel(25, 5).0, [200, 0, 0, 255]);
        assert_eq!(px.get_pixel(35, 5).0, [0, 0, 200, 255]);
        // Sources outside the image leave pixels unchanged.
        stroke(
            &mut doc,
            StrokeKind::Clone {
                offset: (-100.0, 0.0),
            },
            brush(4.0),
            &[(30.0, 5.0)],
        );
        assert_eq!(
            doc.layer(0).unwrap().pixels.get_pixel(30, 5).0,
            [0, 0, 200, 255]
        );
    }

    #[test]
    fn bucket_fills_region_with_opacity_and_selection() {
        let mut doc = Document::new("d", 9, 3, 72.0).unwrap();
        let pixels = RgbaImage::from_fn(9, 3, |x, _| {
            if x == 4 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });
        doc.add_layer(Layer::new("l", pixels)).unwrap();
        let original = doc.clone();
        let mut stack = UndoStack::new();
        stack
            .execute(
                &mut doc,
                Box::new(BucketFill::new(0, (1, 1), [255, 0, 0, 255], 32, true, 1.0)),
            )
            .unwrap();
        let px = |doc: &Document, x| doc.layer(0).unwrap().pixels.get_pixel(x, 1).0;
        assert_eq!(px(&doc, 0), [255, 0, 0, 255]);
        assert_eq!(px(&doc, 4), [0, 0, 0, 255], "barrier");
        assert_eq!(px(&doc, 6), [255, 255, 255, 255], "not contiguous");
        stack.undo(&mut doc);
        assert_eq!(doc, original);

        // Non-contiguous, half opacity, selection limited to x ≥ 6.
        doc.set_selection(Some(selection::rect_mask(9, 3, (6.0, 0.0), (9.0, 3.0))))
            .unwrap();
        stack
            .execute(
                &mut doc,
                Box::new(BucketFill::new(0, (1, 1), [255, 0, 0, 255], 32, false, 0.5)),
            )
            .unwrap();
        assert_eq!(px(&doc, 1), [255, 255, 255, 255], "outside selection");
        let half = px(&doc, 7);
        assert_eq!(half[0], 255);
        assert!((half[1] as i32 - 128).abs() <= 1);
        assert_eq!(stack.undo_label(), Some("Paint Bucket"));
        assert!(BucketFill::new(0, (99, 0), [0; 4], 0, true, 1.0)
            .apply(&mut doc)
            .is_err());
    }

    #[test]
    fn gradient_positions() {
        let lin = |p| gradient_position(GradientKind::Linear, (0.0, 0.0), (10.0, 0.0), p);
        assert_eq!(lin((0.0, 5.0)), 0.0);
        assert_eq!(lin((5.0, 99.0)), 0.5);
        assert_eq!(lin((20.0, 0.0)), 1.0);
        assert_eq!(lin((-5.0, 0.0)), 0.0);
        let rad = |p| gradient_position(GradientKind::Radial, (10.0, 10.0), (20.0, 10.0), p);
        assert_eq!(rad((10.0, 10.0)), 0.0);
        assert_eq!(rad((10.0, 15.0)), 0.5);
        assert_eq!(rad((0.0, 10.0)), 1.0);
    }

    #[test]
    fn gradient_fills_fg_to_bg_and_to_transparent() {
        let mut doc = white_doc(101, 3);
        let mut g = GradientFill::new(
            0,
            (0.5, 1.5),
            (100.5, 1.5),
            GradientKind::Linear,
            GradientColors::ForegroundToBackground,
            [0, 0, 0, 255],
            [255, 0, 0, 255],
            1.0,
        );
        g.apply(&mut doc).unwrap();
        let px = |doc: &Document, x| doc.layer(0).unwrap().pixels.get_pixel(x, 1).0;
        assert_eq!(px(&doc, 0), [0, 0, 0, 255]);
        assert_eq!(px(&doc, 100), [255, 0, 0, 255]);
        let mid = px(&doc, 50);
        assert!((mid[0] as i32 - 128).abs() <= 2 && mid[1] == 0);
        assert_eq!(g.label(), "Gradient");

        // Foreground → transparent over white: fades from black to white.
        let mut doc2 = white_doc(101, 3);
        let mut t = GradientFill::new(
            0,
            (0.5, 1.5),
            (100.5, 1.5),
            GradientKind::Radial,
            GradientColors::ForegroundToTransparent,
            [0, 0, 0, 255],
            [255, 255, 255, 255],
            0.5,
        );
        t.apply(&mut doc2).unwrap();
        assert_eq!(px(&doc2, 0)[0], 128, "black at 50% opacity over white");
        assert_eq!(px(&doc2, 100), [255, 255, 255, 255]);
        assert_eq!(px(&doc2, 0)[3], 255);

        // Too-short drag is rejected.
        let mut short = GradientFill {
            start: (1.0, 1.0),
            end: (1.2, 1.0),
            ..t
        };
        assert!(short.apply(&mut doc2).is_err());
    }

    #[test]
    fn gradient_respects_selection() {
        let mut doc = white_doc(10, 1);
        doc.set_selection(Some(selection::rect_mask(10, 1, (0.0, 0.0), (5.0, 1.0))))
            .unwrap();
        let mut g = GradientFill::new(
            0,
            (0.0, 0.5),
            (10.0, 0.5),
            GradientKind::Linear,
            GradientColors::ForegroundToBackground,
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            1.0,
        );
        g.apply(&mut doc).unwrap();
        let px = &doc.layer(0).unwrap().pixels;
        assert_eq!(px.get_pixel(2, 0).0, [0, 0, 0, 255]);
        assert_eq!(px.get_pixel(7, 0).0, [255, 255, 255, 255]);
        assert_eq!(g.dirty_rect(&doc), PixelRect::new(0, 0, 5, 1));
    }

    #[test]
    fn eyedropper_samples_composite_or_layer() {
        let mut doc = white_doc(4, 4);
        let mut top = Layer::new("top", RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255])));
        top.set_opacity(0.5);
        doc.add_layer(top).unwrap();
        let composite = sample_color(&doc, 0, 1, 1, true).unwrap();
        assert_eq!(composite[0], 255);
        assert!((composite[1] as i32 - 127).abs() <= 1);
        assert_eq!(
            sample_color(&doc, 0, 1, 1, false).unwrap(),
            [255, 255, 255, 255]
        );
        assert_eq!(
            sample_color(&doc, 1, 1, 1, false).unwrap(),
            [255, 0, 0, 255]
        );
        assert_eq!(sample_color(&doc, 0, 9, 1, true), None);
    }

    #[test]
    fn size_steps_are_clamped() {
        let b = brush(20.0);
        assert_eq!(b.stepped_size(true), 24.0);
        assert_eq!(b.stepped_size(false), 16.0);
        assert_eq!(brush(1.0).stepped_size(false), 1.0);
        assert_eq!(brush(500.0).stepped_size(true), 500.0);
        assert_eq!(brush(2.0).stepped_size(true), 3.0);
    }

    /// Left third fully selected, column 30 half selected, the rest unselected.
    fn left_selection(doc: &mut Document) {
        let (w, h) = (doc.width(), doc.height());
        let mut mask = GrayImage::new(w, h);
        for y in 0..h {
            for x in 0..30 {
                mask.put_pixel(x, y, Luma([255]));
            }
            mask.put_pixel(30, y, Luma([128]));
        }
        doc.set_selection(Some(mask)).unwrap();
    }

    fn pixel(doc: &Document, x: u32, y: u32) -> [u8; 4] {
        doc.layer(0).unwrap().pixels.get_pixel(x, y).0
    }

    #[test]
    fn eraser_respects_selection() {
        let mut doc = white_doc(60, 20);
        left_selection(&mut doc);
        stroke(
            &mut doc,
            StrokeKind::Eraser,
            brush(30.0),
            &[(5.0, 10.0), (55.0, 10.0)],
        );
        assert_eq!(pixel(&doc, 15, 10)[3], 0, "selected: erased");
        assert_eq!(
            pixel(&doc, 45, 10),
            [255, 255, 255, 255],
            "unselected: unchanged"
        );
        let partial = pixel(&doc, 30, 10)[3];
        assert!(
            (partial as i32 - 127).abs() <= 2,
            "half selected: half erased ({partial})"
        );
    }

    #[test]
    fn clone_respects_selection() {
        let mut doc = Document::new("d", 60, 20, 72.0).unwrap();
        let pixels = RgbaImage::from_fn(60, 20, |x, _| {
            if x >= 50 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });
        doc.add_layer(Layer::new("l", pixels)).unwrap();
        left_selection(&mut doc);
        // Copy from 25 px to the right, so x ≥ 25 picks up the black band at x ≥ 50.
        stroke(
            &mut doc,
            StrokeKind::Clone {
                offset: (25.0, 0.0),
            },
            brush(30.0),
            &[(28.0, 10.0), (40.0, 10.0)],
        );
        assert_eq!(
            pixel(&doc, 28, 10),
            [0, 0, 0, 255],
            "selected: cloned from x=53"
        );
        assert_eq!(
            pixel(&doc, 35, 10),
            [255, 255, 255, 255],
            "unselected: unchanged"
        );
        let partial = pixel(&doc, 30, 10)[0];
        assert!(
            (partial as i32 - 128).abs() <= 2,
            "half selected: half cloned ({partial})"
        );
    }

    #[test]
    fn bucket_and_gradient_interpolate_partial_selection() {
        let mut doc = white_doc(60, 20);
        left_selection(&mut doc);
        BucketFill::new(0, (5, 5), [0, 0, 0, 255], 32, true, 1.0)
            .apply(&mut doc)
            .unwrap();
        assert_eq!(pixel(&doc, 5, 5), [0, 0, 0, 255]);
        assert_eq!(pixel(&doc, 45, 5), [255, 255, 255, 255]);
        assert!((pixel(&doc, 30, 5)[0] as i32 - 127).abs() <= 2);

        let mut doc = white_doc(60, 20);
        left_selection(&mut doc);
        GradientFill::new(
            0,
            (0.0, 10.0),
            (60.0, 10.0),
            GradientKind::Linear,
            GradientColors::ForegroundToBackground,
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            1.0,
        )
        .apply(&mut doc)
        .unwrap();
        assert_eq!(pixel(&doc, 5, 5), [0, 0, 0, 255]);
        assert_eq!(pixel(&doc, 45, 5), [255, 255, 255, 255]);
        assert!((pixel(&doc, 30, 5)[0] as i32 - 127).abs() <= 2);
    }
}
