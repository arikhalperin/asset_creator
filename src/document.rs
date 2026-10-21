//! In-memory document model: a document holds metadata and an ordered stack of RGBA8 layers.
//!
//! Layer index 0 is the bottom of the stack; the last index is the top.

use std::path::PathBuf;

use image::{GrayImage, Rgba, RgbaImage};
use thiserror::Error;

/// DPI used when a file carries no resolution information.
pub const DEFAULT_DPI: f64 = 72.0;

#[derive(Debug, Error, PartialEq)]
pub enum DocumentError {
    #[error("document size must be positive, got {width}x{height}")]
    InvalidSize { width: u32, height: u32 },
    #[error("DPI must be a positive finite number, got {0}")]
    InvalidDpi(f64),
    #[error("layer is {layer_width}x{layer_height} but the document is {doc_width}x{doc_height}")]
    LayerSizeMismatch {
        layer_width: u32,
        layer_height: u32,
        doc_width: u32,
        doc_height: u32,
    },
    #[error("layer index {index} is out of range (layer count {len})")]
    IndexOutOfRange { index: usize, len: usize },
    #[error(
        "layer mask is {mask_width}x{mask_height} but the layer is {layer_width}x{layer_height}"
    )]
    MaskSizeMismatch {
        mask_width: u32,
        mask_height: u32,
        layer_width: u32,
        layer_height: u32,
    },
    #[error(
        "selection mask is {mask_width}x{mask_height} but the document is {doc_width}x{doc_height}"
    )]
    SelectionSizeMismatch {
        mask_width: u32,
        mask_height: u32,
        doc_width: u32,
        doc_height: u32,
    },
}

/// An axis-aligned rectangle in image pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn right(&self) -> u32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> u32 {
        self.y + self.height
    }

    /// The overlapping rectangle, or `None` when they do not overlap.
    pub fn intersect(&self, other: PixelRect) -> Option<PixelRect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then(|| PixelRect::new(x, y, right - x, bottom - y))
    }
}

/// How a layer combines with the layers below it (W3C Compositing and Blending).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Add,
    Darken,
    Lighten,
}

impl BlendMode {
    pub const ALL: [BlendMode; 7] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::Add,
        Self::Darken,
        Self::Lighten,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Overlay => "Overlay",
            Self::Add => "Add",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
        }
    }

    /// Identifier used in project files.
    pub fn key(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Multiply => "multiply",
            Self::Screen => "screen",
            Self::Overlay => "overlay",
            Self::Add => "add",
            Self::Darken => "darken",
            Self::Lighten => "lighten",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == key)
    }

    /// The separable blend function `B(Cb, Cs)` on unpremultiplied values in [0, 1].
    pub fn blend(self, cb: f32, cs: f32) -> f32 {
        let screen = |b: f32, s: f32| b + s - b * s;
        match self {
            Self::Normal => cs,
            Self::Multiply => cs * cb,
            Self::Screen => screen(cb, cs),
            Self::Overlay => {
                if cb <= 0.5 {
                    2.0 * cs * cb
                } else {
                    screen(cs, 2.0 * cb - 1.0)
                }
            }
            Self::Add => (cb + cs).min(1.0),
            Self::Darken => cb.min(cs),
            Self::Lighten => cb.max(cs),
        }
    }
}

/// A single raster layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    opacity: f32,
    pub blend_mode: BlendMode,
    mask: Option<GrayImage>,
    /// A disabled mask is kept but ignored when compositing.
    pub mask_enabled: bool,
    pub pixels: RgbaImage,
}

impl Layer {
    pub fn new(name: impl Into<String>, pixels: RgbaImage) -> Self {
        Self {
            name: name.into(),
            visible: true,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            mask: None,
            mask_enabled: true,
            pixels,
        }
    }

    /// The layer mask (0 hides, 255 shows), if any.
    pub fn mask(&self) -> Option<&GrayImage> {
        self.mask.as_ref()
    }

    pub fn mask_mut(&mut self) -> Option<&mut GrayImage> {
        self.mask.as_mut()
    }

    /// Sets or removes the mask; a mask of the wrong size is rejected.
    pub fn set_mask(&mut self, mask: Option<GrayImage>) -> Result<(), DocumentError> {
        if let Some(m) = &mask {
            if m.dimensions() != self.pixels.dimensions() {
                return Err(DocumentError::MaskSizeMismatch {
                    mask_width: m.width(),
                    mask_height: m.height(),
                    layer_width: self.width(),
                    layer_height: self.height(),
                });
            }
        }
        self.mask = mask;
        Ok(())
    }

    /// The mask value applied at a pixel (255 without an enabled mask).
    pub fn mask_at(&self, x: u32, y: u32) -> u8 {
        match (&self.mask, self.mask_enabled) {
            (Some(m), true) => m.get_pixel(x, y)[0],
            _ => 255,
        }
    }

    /// A fully transparent layer of the given size.
    pub fn transparent(name: impl Into<String>, width: u32, height: u32) -> Self {
        Self::new(name, RgbaImage::new(width, height))
    }

    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Sets opacity, clamped to `[0.0, 1.0]`. NaN becomes 0.
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = if opacity.is_nan() {
            0.0
        } else {
            opacity.clamp(0.0, 1.0)
        };
    }

    pub fn width(&self) -> u32 {
        self.pixels.width()
    }

    pub fn height(&self) -> u32 {
        self.pixels.height()
    }
}

/// An image document: metadata plus an ordered layer stack.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub name: String,
    pub source_path: Option<PathBuf>,
    width: u32,
    height: u32,
    dpi: f64,
    layers: Vec<Layer>,
    /// 8-bit selection mask; `None` means no selection (edits apply everywhere).
    selection: Option<GrayImage>,
    /// Index of the layer edits target (bottom = 0).
    active_layer: usize,
}

impl Document {
    /// Creates a document with no layers.
    pub fn new(
        name: impl Into<String>,
        width: u32,
        height: u32,
        dpi: f64,
    ) -> Result<Self, DocumentError> {
        if width == 0 || height == 0 {
            return Err(DocumentError::InvalidSize { width, height });
        }
        validate_dpi(dpi)?;
        Ok(Self {
            name: name.into(),
            source_path: None,
            width,
            height,
            dpi,
            layers: Vec::new(),
            selection: None,
            active_layer: 0,
        })
    }

    /// Creates a document with a single fully transparent "Background" layer.
    pub fn new_blank(
        name: impl Into<String>,
        width: u32,
        height: u32,
        dpi: f64,
    ) -> Result<Self, DocumentError> {
        let mut doc = Self::new(name, width, height, dpi)?;
        doc.add_layer(Layer::transparent("Background", width, height))?;
        Ok(doc)
    }

    /// The document cropped to `rect` (clipped to the canvas): every layer,
    /// layer mask, and the selection. `None` if nothing remains.
    pub fn cropped(&self, rect: PixelRect) -> Option<Self> {
        let r = rect.intersect(self.bounds())?;
        let crop_rgba = |img: &RgbaImage| {
            image::imageops::crop_imm(img, r.x, r.y, r.width, r.height).to_image()
        };
        let crop_gray = |img: &GrayImage| {
            image::imageops::crop_imm(img, r.x, r.y, r.width, r.height).to_image()
        };
        let mut doc = self.clone();
        doc.width = r.width;
        doc.height = r.height;
        for layer in &mut doc.layers {
            layer.pixels = crop_rgba(&layer.pixels);
            layer.mask = layer.mask.as_ref().map(crop_gray);
        }
        doc.selection = doc
            .selection
            .as_ref()
            .map(crop_gray)
            .filter(|m| m.pixels().any(|p| p[0] > 0));
        Some(doc)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn dpi(&self) -> f64 {
        self.dpi
    }

    pub fn set_dpi(&mut self, dpi: f64) -> Result<(), DocumentError> {
        validate_dpi(dpi)?;
        self.dpi = dpi;
        Ok(())
    }

    /// Index (bottom = 0) of the layer edits target.
    pub fn active_layer(&self) -> usize {
        self.active_layer.min(self.layers.len().saturating_sub(1))
    }

    /// Sets the active layer; out-of-range indices are ignored.
    pub fn set_active_layer(&mut self, index: usize) {
        if index < self.layers.len() {
            self.active_layer = index;
        }
    }

    /// Replaces the whole layer stack (sizes are checked) and sets the active layer.
    pub fn replace_layers(
        &mut self,
        layers: Vec<Layer>,
        active: usize,
    ) -> Result<(), DocumentError> {
        for layer in &layers {
            self.check_layer_size(layer)?;
        }
        self.layers = layers;
        self.active_layer = active.min(self.layers.len().saturating_sub(1));
        Ok(())
    }

    /// The selection mask, if anything is selected.
    pub fn selection(&self) -> Option<&GrayImage> {
        self.selection.as_ref()
    }

    /// Replaces the selection. A mask with no non-zero pixel clears it; a mask
    /// of the wrong size is rejected and leaves the selection unchanged.
    pub fn set_selection(&mut self, mask: Option<GrayImage>) -> Result<(), DocumentError> {
        if let Some(mask) = &mask {
            if mask.dimensions() != (self.width, self.height) {
                return Err(DocumentError::SelectionSizeMismatch {
                    mask_width: mask.width(),
                    mask_height: mask.height(),
                    doc_width: self.width,
                    doc_height: self.height,
                });
            }
        }
        self.selection = mask.and_then(crate::selection::normalize);
        Ok(())
    }

    /// The region edits can affect: the selection's bounding box, or the
    /// whole document without a selection.
    pub fn editable_bounds(&self) -> PixelRect {
        match &self.selection {
            Some(mask) => crate::selection::bounds(mask).unwrap_or_default(),
            None => self.bounds(),
        }
    }

    /// The selection together with mutable access to one layer.
    pub fn selection_and_layer_mut(
        &mut self,
        index: usize,
    ) -> (Option<&GrayImage>, Option<&mut Layer>) {
        (self.selection.as_ref(), self.layers.get_mut(index))
    }

    /// Layers bottom-to-top.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn layer(&self, index: usize) -> Option<&Layer> {
        self.layers.get(index)
    }

    pub fn layer_mut(&mut self, index: usize) -> Option<&mut Layer> {
        self.layers.get_mut(index)
    }

    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// Appends a layer on top of the stack.
    pub fn add_layer(&mut self, layer: Layer) -> Result<(), DocumentError> {
        self.insert_layer(self.layers.len(), layer)
    }

    /// Inserts a layer at `index` (0 = bottom, `layer_count()` = top).
    pub fn insert_layer(&mut self, index: usize, layer: Layer) -> Result<(), DocumentError> {
        self.check_layer_size(&layer)?;
        if index > self.layers.len() {
            return Err(DocumentError::IndexOutOfRange {
                index,
                len: self.layers.len(),
            });
        }
        self.layers.insert(index, layer);
        Ok(())
    }

    pub fn remove_layer(&mut self, index: usize) -> Result<Layer, DocumentError> {
        self.check_index(index)?;
        let removed = self.layers.remove(index);
        self.active_layer = self.active_layer.min(self.layers.len().saturating_sub(1));
        Ok(removed)
    }

    /// Moves the layer at `from` so it ends up at index `to`, keeping the
    /// relative order of all other layers.
    pub fn move_layer(&mut self, from: usize, to: usize) -> Result<(), DocumentError> {
        self.check_index(from)?;
        self.check_index(to)?;
        let layer = self.layers.remove(from);
        self.layers.insert(to, layer);
        Ok(())
    }

    /// Blends visible layers bottom-to-top with normal (source-over) compositing.
    ///
    /// Where nothing is underneath, a layer's pixels are copied as-is (with alpha
    /// scaled by opacity), so a single full-opacity layer composites to exactly
    /// its own pixels, including colour values under fully transparent pixels.
    pub fn composite(&self) -> RgbaImage {
        self.composite_region(self.bounds())
    }

    /// The whole document as a rectangle.
    pub fn bounds(&self) -> PixelRect {
        PixelRect::new(0, 0, self.width, self.height)
    }

    /// Composites only `rect` (clipped to the document), returning an image
    /// of the clipped rectangle's size. Same blending as [`Self::composite`].
    pub fn composite_region(&self, rect: PixelRect) -> RgbaImage {
        let rect = rect.intersect(self.bounds()).unwrap_or_default();
        let mut out = RgbaImage::new(rect.width, rect.height);
        for layer in self.layers.iter().filter(|l| l.visible && l.opacity > 0.0) {
            composite_layer_onto(&mut out, layer, rect);
        }
        out
    }

    fn check_layer_size(&self, layer: &Layer) -> Result<(), DocumentError> {
        if layer.width() != self.width || layer.height() != self.height {
            return Err(DocumentError::LayerSizeMismatch {
                layer_width: layer.width(),
                layer_height: layer.height(),
                doc_width: self.width,
                doc_height: self.height,
            });
        }
        Ok(())
    }

    fn check_index(&self, index: usize) -> Result<(), DocumentError> {
        if index >= self.layers.len() {
            return Err(DocumentError::IndexOutOfRange {
                index,
                len: self.layers.len(),
            });
        }
        Ok(())
    }
}

fn validate_dpi(dpi: f64) -> Result<(), DocumentError> {
    if dpi.is_finite() && dpi > 0.0 {
        Ok(())
    } else {
        Err(DocumentError::InvalidDpi(dpi))
    }
}

/// Composites `layer` (its blend mode, opacity, and enabled mask) onto `out`,
/// which holds the region `rect` of the document.
pub fn composite_layer_onto(out: &mut RgbaImage, layer: &Layer, rect: PixelRect) {
    let opacity = layer.opacity;
    let use_mask = layer.mask_enabled && layer.mask.is_some();
    for (x, y, dst) in out.enumerate_pixels_mut() {
        let (lx, ly) = (rect.x + x, rect.y + y);
        let src = *layer.pixels.get_pixel(lx, ly);
        let factor = if use_mask {
            opacity * layer.mask_at(lx, ly) as f32 / 255.0
        } else {
            opacity
        };
        *dst = blend_pixel(*dst, src, layer.blend_mode, factor);
    }
}

/// Composites `src` (alpha scaled by `opacity`) onto `dst` with `mode`:
/// `Cs' = (1 − αb)·Cs + αb·B(Cb, Cs)`, then source-over.
pub fn blend_pixel(dst: Rgba<u8>, src: Rgba<u8>, mode: BlendMode, opacity: f32) -> Rgba<u8> {
    if mode == BlendMode::Normal || dst[3] == 0 {
        // Normal, or nothing underneath (B is irrelevant when αb = 0).
        return blend_over(dst, src, opacity);
    }
    let src_a = src[3] as f32 / 255.0 * opacity;
    if src_a <= 0.0 {
        return dst;
    }
    let dst_a = dst[3] as f32 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    let mut out = [0u8; 4];
    for c in 0..3 {
        let cb = dst[c] as f32 / 255.0;
        let cs = src[c] as f32 / 255.0;
        let mixed = (1.0 - dst_a) * cs + dst_a * mode.blend(cb, cs);
        let value = (src_a * mixed + dst_a * cb * (1.0 - src_a)) / out_a;
        out[c] = to_u8(value * 255.0);
    }
    out[3] = to_u8(out_a * 255.0);
    Rgba(out)
}

/// Source-over compositing of `src` (alpha scaled by `opacity`) onto `dst`.
pub fn blend_over(dst: Rgba<u8>, src: Rgba<u8>, opacity: f32) -> Rgba<u8> {
    let src_a = src[3] as f32 / 255.0 * opacity;
    if dst[3] == 0 {
        // Nothing underneath: keep the source colour untouched.
        let a = if opacity >= 1.0 {
            src[3]
        } else {
            to_u8(src_a * 255.0)
        };
        return Rgba([src[0], src[1], src[2], a]);
    }
    if src_a <= 0.0 {
        return dst;
    }
    let dst_a = dst[3] as f32 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    let mut out = [0u8; 4];
    for c in 0..3 {
        let value = (src[c] as f32 * src_a + dst[c] as f32 * dst_a * (1.0 - src_a)) / out_a;
        out[c] = to_u8(value);
    }
    out[3] = to_u8(out_a * 255.0);
    Rgba(out)
}

fn to_u8(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(color))
    }

    fn names(doc: &Document) -> Vec<&str> {
        doc.layers().iter().map(|l| l.name.as_str()).collect()
    }

    #[test]
    fn new_document_stores_metadata() {
        let doc = Document::new("art", 64, 32, 300.0).unwrap();
        assert_eq!((doc.width(), doc.height(), doc.dpi()), (64, 32, 300.0));
        assert_eq!(doc.name, "art");
        assert_eq!(doc.source_path, None);
        assert_eq!(doc.layer_count(), 0);
    }

    #[test]
    fn rejects_zero_size() {
        assert_eq!(
            Document::new("x", 0, 10, 72.0),
            Err(DocumentError::InvalidSize {
                width: 0,
                height: 10
            })
        );
        assert!(Document::new("x", 10, 0, 72.0).is_err());
    }

    #[test]
    fn rejects_invalid_dpi() {
        for dpi in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(Document::new("x", 1, 1, dpi).is_err(), "dpi {dpi} accepted");
        }
        let mut doc = Document::new("x", 1, 1, 72.0).unwrap();
        assert!(doc.set_dpi(0.0).is_err());
        assert_eq!(doc.dpi(), 72.0);
        doc.set_dpi(150.0).unwrap();
        assert_eq!(doc.dpi(), 150.0);
    }

    #[test]
    fn new_blank_has_one_transparent_layer() {
        let doc = Document::new_blank("blank", 8, 4, DEFAULT_DPI).unwrap();
        assert_eq!(doc.layer_count(), 1);
        let layer = doc.layer(0).unwrap();
        assert_eq!(layer.name, "Background");
        assert_eq!((layer.width(), layer.height()), (8, 4));
        assert!(layer.visible);
        assert_eq!(layer.opacity(), 1.0);
        assert!(layer.pixels.pixels().all(|p| p.0 == [0, 0, 0, 0]));
    }

    #[test]
    fn layer_buffers_must_match_document_size() {
        let mut doc = Document::new("x", 4, 4, 72.0).unwrap();
        let err = doc
            .add_layer(Layer::transparent("wrong", 4, 5))
            .unwrap_err();
        assert_eq!(
            err,
            DocumentError::LayerSizeMismatch {
                layer_width: 4,
                layer_height: 5,
                doc_width: 4,
                doc_height: 4
            }
        );
        assert!(doc
            .insert_layer(0, Layer::transparent("wrong", 3, 4))
            .is_err());
        assert_eq!(doc.layer_count(), 0);
    }

    #[test]
    fn add_insert_remove_preserve_order() {
        let mut doc = Document::new("x", 2, 2, 72.0).unwrap();
        doc.add_layer(Layer::transparent("a", 2, 2)).unwrap();
        doc.add_layer(Layer::transparent("c", 2, 2)).unwrap();
        doc.insert_layer(1, Layer::transparent("b", 2, 2)).unwrap();
        doc.insert_layer(3, Layer::transparent("d", 2, 2)).unwrap();
        assert_eq!(names(&doc), ["a", "b", "c", "d"]);

        assert!(doc.insert_layer(9, Layer::transparent("z", 2, 2)).is_err());

        let removed = doc.remove_layer(1).unwrap();
        assert_eq!(removed.name, "b");
        assert_eq!(names(&doc), ["a", "c", "d"]);
        assert_eq!(
            doc.remove_layer(3),
            Err(DocumentError::IndexOutOfRange { index: 3, len: 3 })
        );
    }

    #[test]
    fn move_layer_reorders() {
        let mut doc = Document::new("x", 1, 1, 72.0).unwrap();
        for name in ["a", "b", "c", "d"] {
            doc.add_layer(Layer::transparent(name, 1, 1)).unwrap();
        }
        doc.move_layer(0, 3).unwrap();
        assert_eq!(names(&doc), ["b", "c", "d", "a"]);
        doc.move_layer(3, 1).unwrap();
        assert_eq!(names(&doc), ["b", "a", "c", "d"]);
        doc.move_layer(2, 2).unwrap();
        assert_eq!(names(&doc), ["b", "a", "c", "d"]);
        assert!(doc.move_layer(4, 0).is_err());
        assert!(doc.move_layer(0, 4).is_err());
        assert_eq!(names(&doc), ["b", "a", "c", "d"]);
    }

    #[test]
    fn opacity_is_clamped() {
        let mut layer = Layer::transparent("l", 1, 1);
        layer.set_opacity(1.5);
        assert_eq!(layer.opacity(), 1.0);
        layer.set_opacity(-0.5);
        assert_eq!(layer.opacity(), 0.0);
        layer.set_opacity(f32::NAN);
        assert_eq!(layer.opacity(), 0.0);
        layer.set_opacity(0.25);
        assert_eq!(layer.opacity(), 0.25);
    }

    #[test]
    fn composite_of_single_layer_is_exact() {
        let mut pixels = RgbaImage::new(3, 1);
        pixels.put_pixel(0, 0, Rgba([10, 20, 30, 0]));
        pixels.put_pixel(1, 0, Rgba([200, 100, 50, 128]));
        pixels.put_pixel(2, 0, Rgba([1, 2, 3, 255]));
        let mut doc = Document::new("x", 3, 1, 72.0).unwrap();
        doc.add_layer(Layer::new("only", pixels.clone())).unwrap();
        assert_eq!(doc.composite(), pixels);
    }

    #[test]
    fn composite_empty_document_is_transparent() {
        let doc = Document::new("x", 2, 2, 72.0).unwrap();
        assert!(doc.composite().pixels().all(|p| p.0 == [0, 0, 0, 0]));
    }

    #[test]
    fn opaque_top_layer_covers_bottom() {
        let mut doc = Document::new("x", 2, 2, 72.0).unwrap();
        doc.add_layer(Layer::new("bottom", solid(2, 2, [255, 0, 0, 255])))
            .unwrap();
        doc.add_layer(Layer::new("top", solid(2, 2, [0, 0, 255, 255])))
            .unwrap();
        assert!(doc.composite().pixels().all(|p| p.0 == [0, 0, 255, 255]));
    }

    #[test]
    fn hidden_and_zero_opacity_layers_do_not_contribute() {
        let mut doc = Document::new("x", 1, 1, 72.0).unwrap();
        doc.add_layer(Layer::new("bottom", solid(1, 1, [255, 0, 0, 255])))
            .unwrap();
        let mut hidden = Layer::new("hidden", solid(1, 1, [0, 255, 0, 255]));
        hidden.visible = false;
        doc.add_layer(hidden).unwrap();
        let mut faded = Layer::new("faded", solid(1, 1, [0, 0, 255, 255]));
        faded.set_opacity(0.0);
        doc.add_layer(faded).unwrap();
        assert_eq!(doc.composite().get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn layer_opacity_blends_over_opaque_background() {
        let mut doc = Document::new("x", 1, 1, 72.0).unwrap();
        doc.add_layer(Layer::new("white", solid(1, 1, [255, 255, 255, 255])))
            .unwrap();
        let mut black = Layer::new("black", solid(1, 1, [0, 0, 0, 255]));
        black.set_opacity(0.5);
        doc.add_layer(black).unwrap();
        let p = doc.composite().get_pixel(0, 0).0;
        assert_eq!(p[3], 255);
        for c in &p[..3] {
            assert!((127..=128).contains(c), "channel {c}");
        }
    }

    #[test]
    fn translucent_layers_combine_alpha() {
        let mut doc = Document::new("x", 1, 1, 72.0).unwrap();
        doc.add_layer(Layer::new("a", solid(1, 1, [255, 0, 0, 128])))
            .unwrap();
        doc.add_layer(Layer::new("b", solid(1, 1, [0, 0, 255, 128])))
            .unwrap();
        let p = doc.composite().get_pixel(0, 0).0;
        // out_a = 0.502 + 0.502 * 0.498 ≈ 0.752 → 192
        assert_eq!(p[3], 192);
        // blue dominates because it is on top
        assert!(p[2] > p[0]);
        assert_eq!(p[1], 0);
    }

    #[test]
    fn composite_region_matches_full_composite() {
        let mut doc = Document::new("x", 5, 4, 72.0).unwrap();
        let base = RgbaImage::from_fn(5, 4, |x, y| Rgba([(x * 50) as u8, (y * 60) as u8, 7, 255]));
        doc.add_layer(Layer::new("base", base)).unwrap();
        let mut top = Layer::new("top", solid(5, 4, [0, 0, 255, 100]));
        top.set_opacity(0.7);
        doc.add_layer(top).unwrap();
        let full = doc.composite();
        let region = doc.composite_region(PixelRect::new(1, 2, 3, 2));
        assert_eq!((region.width(), region.height()), (3, 2));
        for (x, y, p) in region.enumerate_pixels() {
            assert_eq!(p, full.get_pixel(x + 1, y + 2));
        }
        // Clipped to the document.
        let clipped = doc.composite_region(PixelRect::new(4, 3, 10, 10));
        assert_eq!((clipped.width(), clipped.height()), (1, 1));
        let outside = doc.composite_region(PixelRect::new(9, 9, 2, 2));
        assert_eq!((outside.width(), outside.height()), (0, 0));
    }

    #[test]
    fn selection_mask_is_validated_and_normalized() {
        let mut doc = Document::new("x", 4, 3, 72.0).unwrap();
        assert!(doc.selection().is_none());
        assert_eq!(doc.editable_bounds(), PixelRect::new(0, 0, 4, 3));

        let mut mask = GrayImage::new(4, 3);
        mask.put_pixel(1, 2, image::Luma([200]));
        doc.set_selection(Some(mask.clone())).unwrap();
        assert_eq!(doc.selection(), Some(&mask));
        assert_eq!(doc.editable_bounds(), PixelRect::new(1, 2, 1, 1));

        let err = doc.set_selection(Some(GrayImage::new(3, 3))).unwrap_err();
        assert!(matches!(err, DocumentError::SelectionSizeMismatch { .. }));
        assert_eq!(doc.selection(), Some(&mask), "unchanged after rejection");

        doc.set_selection(Some(GrayImage::new(4, 3))).unwrap();
        assert!(doc.selection().is_none(), "empty mask clears");
        // Selection is not composited.
        doc.add_layer(Layer::transparent("l", 4, 3)).unwrap();
        doc.set_selection(Some(crate::selection::select_all(4, 3)))
            .unwrap();
        assert!(doc.composite().pixels().all(|p| p.0 == [0; 4]));
    }

    /// Independent f64 reference of the W3C formulas.
    fn reference(dst: [u8; 4], src: [u8; 4], mode: BlendMode, opacity: f64) -> [u8; 4] {
        let ab = dst[3] as f64 / 255.0;
        let as_ = src[3] as f64 / 255.0 * opacity;
        if ab == 0.0 {
            return [src[0], src[1], src[2], (as_ * 255.0).round() as u8];
        }
        let ao = as_ + ab * (1.0 - as_);
        let mut out = [0u8; 4];
        for c in 0..3 {
            let cb = dst[c] as f64 / 255.0;
            let cs = src[c] as f64 / 255.0;
            let b = match mode {
                BlendMode::Normal => cs,
                BlendMode::Multiply => cb * cs,
                BlendMode::Screen => cb + cs - cb * cs,
                BlendMode::Overlay => {
                    if cb <= 0.5 {
                        2.0 * cb * cs
                    } else {
                        let s2 = 2.0 * cb - 1.0;
                        cs + s2 - cs * s2
                    }
                }
                BlendMode::Add => (cb + cs).min(1.0),
                BlendMode::Darken => cb.min(cs),
                BlendMode::Lighten => cb.max(cs),
            };
            let mixed = (1.0 - ab) * cs + ab * b;
            let co = (as_ * mixed + ab * cb * (1.0 - as_)) / ao;
            out[c] = (co * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        out[3] = (ao * 255.0).round() as u8;
        out
    }

    #[test]
    fn blend_modes_match_reference_formulas() {
        let values = [0u8, 1, 37, 64, 127, 128, 200, 254, 255];
        let alphas = [0u8, 1, 64, 128, 200, 255];
        let mut checked = 0;
        for mode in BlendMode::ALL {
            for &b in &values {
                for &s in &values {
                    for &ab in &alphas {
                        for &as_ in &alphas {
                            for opacity in [1.0, 0.5] {
                                let dst = [b, 255 - b, b / 2, ab];
                                let src = [s, s / 3, 255 - s, as_];
                                let got = blend_pixel(Rgba(dst), Rgba(src), mode, opacity as f32).0;
                                if as_ == 0 && ab > 0 {
                                    assert_eq!(got, dst);
                                    continue;
                                }
                                let want = reference(dst, src, mode, opacity);
                                for c in 0..4 {
                                    assert!(
                                        got[c].abs_diff(want[c]) <= 1,
                                        "{mode:?} dst {dst:?} src {src:?} op {opacity}: {got:?} vs {want:?}"
                                    );
                                }
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 20_000, "{checked}");
    }

    #[test]
    fn known_blend_values_on_opaque_backdrop() {
        let gray = Rgba([128, 128, 128, 255]);
        let white = Rgba([255, 255, 255, 255]);
        let black = Rgba([0, 0, 0, 255]);
        let red = Rgba([255, 0, 0, 255]);
        assert_eq!(blend_pixel(white, red, BlendMode::Multiply, 1.0), red);
        assert_eq!(blend_pixel(black, red, BlendMode::Screen, 1.0), red);
        assert_eq!(blend_pixel(gray, gray, BlendMode::Add, 1.0).0[0], 255);
        assert_eq!(blend_pixel(gray, white, BlendMode::Darken, 1.0), gray);
        assert_eq!(blend_pixel(gray, black, BlendMode::Lighten, 1.0), gray);
        // Overlay keeps black and white backdrops.
        assert_eq!(blend_pixel(black, gray, BlendMode::Overlay, 1.0), black);
        assert_eq!(blend_pixel(white, gray, BlendMode::Overlay, 1.0), white);
        for mode in BlendMode::ALL {
            assert_eq!(BlendMode::from_key(mode.key()), Some(mode));
        }
        assert_eq!(BlendMode::from_key("dissolve"), None);
    }

    #[test]
    fn composite_uses_blend_mode_and_enabled_mask() {
        let mut doc = Document::new("x", 2, 1, 72.0).unwrap();
        doc.add_layer(Layer::new("white", solid(2, 1, [255, 255, 255, 255])))
            .unwrap();
        let mut top = Layer::new("red", solid(2, 1, [255, 0, 0, 255]));
        top.blend_mode = BlendMode::Multiply;
        let mut mask = GrayImage::new(2, 1);
        mask.put_pixel(0, 0, image::Luma([255]));
        top.set_mask(Some(mask)).unwrap();
        doc.add_layer(top).unwrap();
        let c = doc.composite();
        assert_eq!(
            c.get_pixel(0, 0).0,
            [255, 0, 0, 255],
            "multiplied where mask shows"
        );
        assert_eq!(c.get_pixel(1, 0).0, [255, 255, 255, 255], "hidden by mask");
        doc.layer_mut(1).unwrap().mask_enabled = false;
        assert_eq!(
            doc.composite().get_pixel(1, 0).0,
            [255, 0, 0, 255],
            "disabled mask ignored"
        );
        assert!(doc
            .layer_mut(1)
            .unwrap()
            .set_mask(Some(GrayImage::new(3, 3)))
            .is_err());
    }

    #[test]
    fn active_layer_is_kept_in_range() {
        let mut doc = Document::new("x", 1, 1, 72.0).unwrap();
        for name in ["a", "b", "c"] {
            doc.add_layer(Layer::transparent(name, 1, 1)).unwrap();
        }
        doc.set_active_layer(2);
        assert_eq!(doc.active_layer(), 2);
        doc.set_active_layer(9);
        assert_eq!(doc.active_layer(), 2);
        doc.remove_layer(2).unwrap();
        assert_eq!(doc.active_layer(), 1);
        doc.replace_layers(vec![Layer::transparent("z", 1, 1)], 5)
            .unwrap();
        assert_eq!(doc.active_layer(), 0);
        assert!(doc
            .replace_layers(vec![Layer::transparent("bad", 2, 2)], 0)
            .is_err());
    }

    #[test]
    fn pixel_rect_intersection() {
        let a = PixelRect::new(0, 0, 10, 10);
        assert_eq!(
            a.intersect(PixelRect::new(5, 5, 10, 10)),
            Some(PixelRect::new(5, 5, 5, 5))
        );
        assert_eq!(a.intersect(PixelRect::new(10, 0, 5, 5)), None);
        assert!(PixelRect::new(1, 1, 0, 3).is_empty());
    }

    #[test]
    fn opacity_scales_alpha_on_empty_canvas() {
        let mut doc = Document::new("x", 1, 1, 72.0).unwrap();
        let mut layer = Layer::new("l", solid(1, 1, [10, 20, 30, 255]));
        layer.set_opacity(0.5);
        doc.add_layer(layer).unwrap();
        assert_eq!(doc.composite().get_pixel(0, 0).0, [10, 20, 30, 128]);
    }
}
