//! Editing tools and their options, plus turning a finished selection
//! gesture into a selection command. Window-independent.

use crate::document::Document;
use crate::paint::{BrushSettings, GradientColors, GradientKind};
use crate::selection::{self, CombineMode};
use crate::undo::SetSelection;

/// The selectable tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tool {
    #[default]
    RectSelect,
    EllipseSelect,
    Lasso,
    PolygonLasso,
    MagicWand,
    Brush,
    Eraser,
    Eyedropper,
    PaintBucket,
    Gradient,
    CloneStamp,
}

impl Tool {
    pub const ALL: [Tool; 11] = [
        Self::RectSelect,
        Self::EllipseSelect,
        Self::Lasso,
        Self::PolygonLasso,
        Self::MagicWand,
        Self::Brush,
        Self::Eraser,
        Self::Eyedropper,
        Self::PaintBucket,
        Self::Gradient,
        Self::CloneStamp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::RectSelect => "Rectangle Select",
            Self::EllipseSelect => "Ellipse Select",
            Self::Lasso => "Lasso",
            Self::PolygonLasso => "Polygon Lasso",
            Self::MagicWand => "Magic Wand",
            Self::Brush => "Brush",
            Self::Eraser => "Eraser",
            Self::Eyedropper => "Eyedropper",
            Self::PaintBucket => "Paint Bucket",
            Self::Gradient => "Gradient",
            Self::CloneStamp => "Clone Stamp",
        }
    }

    /// Selection tools draw selections; the rest paint or sample.
    pub fn is_selection_tool(self) -> bool {
        matches!(
            self,
            Self::RectSelect
                | Self::EllipseSelect
                | Self::Lasso
                | Self::PolygonLasso
                | Self::MagicWand
        )
    }

    /// Tools that paint strokes with a round brush.
    pub fn is_brush_tool(self) -> bool {
        matches!(self, Self::Brush | Self::Eraser | Self::CloneStamp)
    }

    /// The tool after pressing G (Gradient ↔ Paint Bucket).
    pub fn after_g(self) -> Self {
        if self == Self::Gradient {
            Self::PaintBucket
        } else {
            Self::Gradient
        }
    }

    /// Shortcut key hint shown in the Tools panel.
    pub fn shortcut(self) -> &'static str {
        match self {
            Self::RectSelect | Self::EllipseSelect => "M",
            Self::Lasso | Self::PolygonLasso => "L",
            Self::MagicWand => "W",
            Self::Brush => "B",
            Self::Eraser => "E",
            Self::Eyedropper => "I",
            Self::PaintBucket | Self::Gradient => "G",
            Self::CloneStamp => "S",
        }
    }

    /// Whether the Anti-alias option applies.
    pub fn uses_antialias(self) -> bool {
        matches!(self, Self::EllipseSelect | Self::Lasso | Self::PolygonLasso)
    }

    /// The tool after pressing M.
    pub fn after_m(self) -> Self {
        if self == Self::RectSelect {
            Self::EllipseSelect
        } else {
            Self::RectSelect
        }
    }

    /// The tool after pressing L.
    pub fn after_l(self) -> Self {
        if self == Self::Lasso {
            Self::PolygonLasso
        } else {
            Self::Lasso
        }
    }

    /// Label of the undo step a selection made with this tool creates.
    pub fn selection_label(self) -> &'static str {
        match self {
            Self::RectSelect => "Rectangle Select",
            Self::EllipseSelect => "Ellipse Select",
            Self::Lasso => "Lasso Select",
            Self::PolygonLasso => "Polygon Select",
            Self::MagicWand => "Magic Wand",
            _ => "Selection",
        }
    }
}

/// Foreground and background colors (RGBA).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colors {
    pub foreground: [u8; 4],
    pub background: [u8; 4],
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            foreground: [0, 0, 0, 255],
            background: [255, 255, 255, 255],
        }
    }
}

impl Colors {
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.foreground, &mut self.background);
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Options shown in the tool-options strip; each tool keeps its own values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolOptions {
    /// Combine mode used when no modifier is held.
    pub mode: CombineMode,
    /// Soft edges for Ellipse, Lasso, and Polygon Lasso.
    pub antialias: bool,
    /// Magic Wand: max RGBA channel difference.
    pub tolerance: u8,
    /// Magic Wand: only the connected region.
    pub contiguous: bool,
    pub brush: BrushSettings,
    pub eraser: BrushSettings,
    pub clone: BrushSettings,
    /// Clone Stamp keeps its source offset across strokes.
    pub clone_aligned: bool,
    pub bucket_tolerance: u8,
    pub bucket_contiguous: bool,
    /// 0.01–1.0.
    pub bucket_opacity: f32,
    pub gradient_kind: GradientKind,
    pub gradient_colors: GradientColors,
    /// 0.01–1.0.
    pub gradient_opacity: f32,
    /// Eyedropper reads the visible composite (true) or the active layer.
    pub sample_all_layers: bool,
}

impl ToolOptions {
    /// Brush settings of `tool`, if it is a brush tool.
    pub fn brush_for(&self, tool: Tool) -> Option<&BrushSettings> {
        match tool {
            Tool::Brush => Some(&self.brush),
            Tool::Eraser => Some(&self.eraser),
            Tool::CloneStamp => Some(&self.clone),
            _ => None,
        }
    }

    pub fn brush_for_mut(&mut self, tool: Tool) -> Option<&mut BrushSettings> {
        match tool {
            Tool::Brush => Some(&mut self.brush),
            Tool::Eraser => Some(&mut self.eraser),
            Tool::CloneStamp => Some(&mut self.clone),
            _ => None,
        }
    }
}

impl Default for ToolOptions {
    fn default() -> Self {
        Self {
            mode: CombineMode::Replace,
            antialias: true,
            tolerance: 32,
            contiguous: true,
            brush: BrushSettings::default(),
            eraser: BrushSettings::default(),
            clone: BrushSettings::default(),
            clone_aligned: true,
            bucket_tolerance: 32,
            bucket_contiguous: true,
            bucket_opacity: 1.0,
            gradient_kind: GradientKind::Linear,
            gradient_colors: GradientColors::ForegroundToBackground,
            gradient_opacity: 1.0,
            sample_all_layers: true,
        }
    }
}

/// The geometry a finished gesture produced, in image coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectionShape {
    Rect((f32, f32), (f32, f32)),
    Ellipse((f32, f32), (f32, f32)),
    Polygon(Vec<(f32, f32)>),
    Wand(u32, u32),
    /// A click without dragging: clears the selection in Replace mode.
    Clear,
}

/// A finished selection gesture.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionRequest {
    pub tool: Tool,
    pub shape: SelectionShape,
    pub mode: CombineMode,
}

/// Builds the command for a finished gesture, or `None` when it would not
/// change the selection.
pub fn selection_command(
    doc: &Document,
    active_layer: usize,
    request: &SelectionRequest,
    options: &ToolOptions,
) -> Option<SetSelection> {
    let (w, h) = (doc.width(), doc.height());
    let shape = match &request.shape {
        SelectionShape::Clear => {
            return (request.mode == CombineMode::Replace && doc.selection().is_some())
                .then(|| SetSelection::new("Deselect", None));
        }
        SelectionShape::Rect(a, b) => selection::rect_mask(w, h, *a, *b),
        SelectionShape::Ellipse(a, b) => selection::ellipse_mask(w, h, *a, *b, options.antialias),
        SelectionShape::Polygon(points) => selection::polygon_mask(w, h, points, options.antialias),
        SelectionShape::Wand(x, y) => {
            let layer = doc.layer(active_layer)?;
            if *x >= w || *y >= h {
                return None;
            }
            selection::magic_wand(
                &layer.pixels,
                (*x, *y),
                options.tolerance,
                options.contiguous,
            )
        }
    };
    let result = selection::combine(doc.selection(), &shape, request.mode);
    (result.as_ref() != doc.selection())
        .then(|| SetSelection::new(request.tool.selection_label(), result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Layer;
    use crate::undo::Command;
    use image::{Rgba, RgbaImage};

    fn request(tool: Tool, shape: SelectionShape, mode: CombineMode) -> SelectionRequest {
        SelectionRequest { tool, shape, mode }
    }

    fn apply(doc: &mut Document, request: &SelectionRequest, options: &ToolOptions) -> bool {
        match selection_command(doc, 0, request, options) {
            Some(mut cmd) => {
                cmd.apply(doc).unwrap();
                true
            }
            None => false,
        }
    }

    #[test]
    fn shortcuts_toggle_tools() {
        assert_eq!(Tool::RectSelect.after_m(), Tool::EllipseSelect);
        assert_eq!(Tool::EllipseSelect.after_m(), Tool::RectSelect);
        assert_eq!(Tool::MagicWand.after_m(), Tool::RectSelect);
        assert_eq!(Tool::Lasso.after_l(), Tool::PolygonLasso);
        assert_eq!(Tool::PolygonLasso.after_l(), Tool::Lasso);
        assert_eq!(Tool::RectSelect.after_l(), Tool::Lasso);
        assert_eq!(Tool::default(), Tool::RectSelect);
        assert!(Tool::Lasso.uses_antialias() && !Tool::MagicWand.uses_antialias());
    }

    #[test]
    fn paint_tool_defaults_and_shortcuts() {
        let o = ToolOptions::default();
        for tool in [Tool::Brush, Tool::Eraser, Tool::CloneStamp] {
            let b = o.brush_for(tool).unwrap();
            assert_eq!(
                (b.size, b.hardness, b.opacity, b.pixel),
                (20.0, 1.0, 1.0, false)
            );
        }
        assert!(o.brush_for(Tool::Gradient).is_none());
        assert!(o.clone_aligned && o.bucket_contiguous && o.sample_all_layers);
        assert_eq!(
            (o.bucket_tolerance, o.bucket_opacity, o.gradient_opacity),
            (32, 1.0, 1.0)
        );
        assert_eq!(o.gradient_kind, GradientKind::Linear);
        assert_eq!(o.gradient_colors, GradientColors::ForegroundToBackground);
        assert_eq!(Tool::RectSelect.after_g(), Tool::Gradient);
        assert_eq!(Tool::Gradient.after_g(), Tool::PaintBucket);
        assert_eq!(Tool::PaintBucket.after_g(), Tool::Gradient);
        assert!(Tool::MagicWand.is_selection_tool() && !Tool::Brush.is_selection_tool());
        assert!(Tool::CloneStamp.is_brush_tool() && !Tool::PaintBucket.is_brush_tool());
        let mut c = Colors::default();
        assert_eq!(
            (c.foreground, c.background),
            ([0, 0, 0, 255], [255, 255, 255, 255])
        );
        c.foreground = [1, 2, 3, 4];
        c.swap();
        assert_eq!(c.background, [1, 2, 3, 4]);
        c.reset();
        assert_eq!(c, Colors::default());
        // Separate per-tool values.
        let mut o = ToolOptions::default();
        o.brush_for_mut(Tool::Brush).unwrap().size = 5.0;
        assert_eq!(o.eraser.size, 20.0);
    }

    #[test]
    fn defaults() {
        let o = ToolOptions::default();
        assert_eq!(
            (o.mode, o.antialias, o.tolerance, o.contiguous),
            (CombineMode::Replace, true, 32, true)
        );
    }

    #[test]
    fn requests_build_and_combine_selections() {
        let mut doc = Document::new_blank("d", 20, 20, 72.0).unwrap();
        let o = ToolOptions::default();
        let rect = request(
            Tool::RectSelect,
            SelectionShape::Rect((0.0, 0.0), (10.0, 10.0)),
            CombineMode::Replace,
        );
        let cmd = selection_command(&doc, 0, &rect, &o).unwrap();
        assert_eq!(cmd.label(), "Rectangle Select");
        assert!(apply(&mut doc, &rect, &o));
        assert_eq!(
            selection::bounds(doc.selection().unwrap()).unwrap().width,
            10
        );

        // Same selection again is a no-op.
        assert!(selection_command(&doc, 0, &rect, &o).is_none());

        let add = request(
            Tool::RectSelect,
            SelectionShape::Rect((10.0, 0.0), (20.0, 10.0)),
            CombineMode::Add,
        );
        assert!(apply(&mut doc, &add, &o));
        assert_eq!(
            selection::bounds(doc.selection().unwrap()).unwrap().width,
            20
        );

        let sub = request(
            Tool::EllipseSelect,
            SelectionShape::Ellipse((0.0, 0.0), (20.0, 10.0)),
            CombineMode::Subtract,
        );
        assert_eq!(
            selection_command(&doc, 0, &sub, &o).unwrap().label(),
            "Ellipse Select"
        );
        assert!(apply(&mut doc, &sub, &o));
        assert_eq!(doc.selection().unwrap().get_pixel(10, 5)[0], 0);
        assert_eq!(doc.selection().unwrap().get_pixel(0, 0)[0], 255);

        // Clear only in Replace mode with an existing selection.
        let clear_add = request(Tool::RectSelect, SelectionShape::Clear, CombineMode::Add);
        assert!(selection_command(&doc, 0, &clear_add, &o).is_none());
        let clear = request(Tool::Lasso, SelectionShape::Clear, CombineMode::Replace);
        assert_eq!(
            selection_command(&doc, 0, &clear, &o).unwrap().label(),
            "Deselect"
        );
        assert!(apply(&mut doc, &clear, &o));
        assert!(doc.selection().is_none());
        assert!(selection_command(&doc, 0, &clear, &o).is_none());
    }

    #[test]
    fn wand_uses_active_layer_and_options() {
        let mut doc = Document::new("d", 4, 1, 72.0).unwrap();
        let pixels = RgbaImage::from_fn(4, 1, |x, _| {
            if x == 2 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 0, 0, 255])
            }
        });
        doc.add_layer(Layer::new("l", pixels)).unwrap();
        let mut o = ToolOptions::default();
        let wand = request(
            Tool::MagicWand,
            SelectionShape::Wand(0, 0),
            CombineMode::Replace,
        );
        assert!(apply(&mut doc, &wand, &o));
        assert_eq!(doc.selection().unwrap().as_raw(), &vec![255, 255, 0, 0]);
        o.contiguous = false;
        assert!(apply(&mut doc, &wand, &o));
        assert_eq!(doc.selection().unwrap().as_raw(), &vec![255, 255, 0, 255]);
        let outside = request(
            Tool::MagicWand,
            SelectionShape::Wand(9, 0),
            CombineMode::Replace,
        );
        assert!(selection_command(&doc, 0, &outside, &o).is_none());
    }
}
