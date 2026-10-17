//! Command-pattern undo/redo. Every document edit is a [`Command`] executed
//! through an [`UndoStack`] (via `AppState::execute`).

use std::collections::VecDeque;

use image::{GrayImage, Rgba, RgbaImage};
use thiserror::Error;

use crate::document::{Document, PixelRect};

/// Default per-document history budget: 1 GB.
pub const DEFAULT_HISTORY_BUDGET: usize = 1 << 30;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CommandError {
    #[error("layer index {index} is out of range (layer count {len})")]
    NoSuchLayer { index: usize, len: usize },
    #[error("{0}")]
    Invalid(String),
}

/// A reversible document edit.
///
/// `apply` then `revert` must restore the document exactly; `revert` then
/// `apply` must reproduce the applied state.
pub trait Command {
    /// User-visible name, e.g. "Fill Layer".
    fn label(&self) -> &str;

    /// Performs the edit. On error the document must be left unchanged.
    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError>;

    /// Undoes a previous successful `apply`.
    fn revert(&mut self, doc: &mut Document);

    /// The image region this command changes.
    fn dirty_rect(&self, doc: &Document) -> PixelRect;

    /// Approximate bytes retained while this command is in history.
    fn memory_cost(&self) -> usize;

    /// Whether this command changes the selection (so views redraw its outline).
    fn changes_selection(&self) -> bool {
        false
    }
}

/// What an executed, undone, or redone command changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// Pixel region to refresh (empty when no pixels changed).
    pub dirty: PixelRect,
    pub selection_changed: bool,
}

impl Change {
    fn of(command: &dyn Command, doc: &Document) -> Self {
        Self {
            dirty: command.dirty_rect(doc),
            selection_changed: command.changes_selection(),
        }
    }
}

/// Undo and redo history for one document.
pub struct UndoStack {
    /// Commands with the memory cost recorded when they entered history.
    undo: VecDeque<(usize, Box<dyn Command>)>,
    redo: Vec<(usize, Box<dyn Command>)>,
    budget: usize,
    memory: usize,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::with_budget(DEFAULT_HISTORY_BUDGET)
    }
}

impl std::fmt::Debug for UndoStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UndoStack")
            .field("undo", &self.undo.len())
            .field("redo", &self.redo.len())
            .field("memory", &self.memory)
            .field("budget", &self.budget)
            .finish()
    }
}

impl UndoStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// A stack that drops the oldest steps once retained memory exceeds `budget` bytes.
    pub fn with_budget(budget: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            budget,
            memory: 0,
        }
    }

    /// Applies `command` to `doc` and records it, clearing redo history.
    /// Returns the region it changed.
    pub fn execute(
        &mut self,
        doc: &mut Document,
        mut command: Box<dyn Command>,
    ) -> Result<Change, CommandError> {
        command.apply(doc)?;
        let change = Change::of(command.as_ref(), doc);
        for (cost, _) in self.redo.drain(..) {
            self.memory -= cost;
        }
        let cost = command.memory_cost();
        self.memory += cost;
        self.undo.push_back((cost, command));
        while self.memory > self.budget && self.undo.len() > 1 {
            if let Some((cost, _)) = self.undo.pop_front() {
                self.memory -= cost;
            }
        }
        Ok(change)
    }

    /// Reverts the most recent command. Returns the region it changed.
    pub fn undo(&mut self, doc: &mut Document) -> Option<Change> {
        let (cost, mut command) = self.undo.pop_back()?;
        command.revert(doc);
        let change = Change::of(command.as_ref(), doc);
        self.redo.push((cost, command));
        Some(change)
    }

    /// Re-applies the most recently undone command. Returns the region it changed.
    pub fn redo(&mut self, doc: &mut Document) -> Option<Change> {
        let (cost, mut command) = self.redo.pop()?;
        if command.apply(doc).is_err() {
            // The document no longer fits this command; forget it.
            self.memory -= cost;
            return None;
        }
        let change = Change::of(command.as_ref(), doc);
        self.undo.push_back((cost, command));
        Some(change)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.back().map(|(_, c)| c.label())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|(_, c)| c.label())
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// Bytes currently retained by recorded commands.
    pub fn memory_used(&self) -> usize {
        self.memory
    }
}

/// A layer region saved before an edit, for exact revert.
#[derive(Debug, Clone)]
pub struct SavedRegion {
    pub layer: usize,
    pub rect: PixelRect,
    pixels: RgbaImage,
}

impl SavedRegion {
    /// Writes the saved pixels back.
    pub fn restore(&self, doc: &mut Document) {
        if let Some(layer) = doc.layer_mut(self.layer) {
            image::imageops::replace(
                &mut layer.pixels,
                &self.pixels,
                self.rect.x as i64,
                self.rect.y as i64,
            );
        }
    }

    pub fn bytes(&self) -> usize {
        self.pixels.as_raw().len()
    }
}

/// Moves `old` toward `new` by `mask / 255`, interpolating in premultiplied alpha.
pub fn blend_by_mask(old: Rgba<u8>, new: Rgba<u8>, mask: u8) -> Rgba<u8> {
    match mask {
        0 => old,
        255 => new,
        m => {
            let t = m as f32 / 255.0;
            let oa = old[3] as f32 / 255.0;
            let na = new[3] as f32 / 255.0;
            let a = oa + (na - oa) * t;
            if a <= 0.0 {
                return Rgba([0, 0, 0, 0]);
            }
            let mut out = [0u8; 4];
            for c in 0..3 {
                let premult = old[c] as f32 * oa + (new[c] as f32 * na - old[c] as f32 * oa) * t;
                out[c] = (premult / a).round().clamp(0.0, 255.0) as u8;
            }
            out[3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
            Rgba(out)
        }
    }
}

/// The shared path for every command that changes layer pixels: computes
/// `edit(x, y, old)` for each pixel of the editable region and applies it
/// through the selection mask (everywhere when nothing is selected).
/// Returns the region's previous pixels for revert.
pub fn apply_masked(
    doc: &mut Document,
    layer: usize,
    edit: impl Fn(u32, u32, Rgba<u8>) -> Rgba<u8>,
) -> Result<SavedRegion, CommandError> {
    let len = doc.layer_count();
    let rect = doc.editable_bounds();
    let (mask, target) = doc.selection_and_layer_mut(layer);
    let target = target.ok_or(CommandError::NoSuchLayer { index: layer, len })?;
    let saved = image::imageops::crop_imm(&target.pixels, rect.x, rect.y, rect.width, rect.height)
        .to_image();
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            let coverage = mask.map_or(255, |m| m.get_pixel(x, y)[0]);
            if coverage == 0 {
                continue;
            }
            let old = *target.pixels.get_pixel(x, y);
            let new = edit(x, y, old);
            target
                .pixels
                .put_pixel(x, y, blend_by_mask(old, new, coverage));
        }
    }
    Ok(SavedRegion {
        layer,
        rect,
        pixels: saved,
    })
}

/// Fills a layer with one RGBA color, limited to the selection.
pub struct FillLayer {
    layer: usize,
    color: Rgba<u8>,
    saved: Option<SavedRegion>,
    cost: usize,
    dirty: PixelRect,
}

impl FillLayer {
    pub fn new(layer: usize, color: [u8; 4]) -> Self {
        Self {
            layer,
            color: Rgba(color),
            saved: None,
            cost: std::mem::size_of::<Self>(),
            dirty: PixelRect::default(),
        }
    }
}

impl Command for FillLayer {
    fn label(&self) -> &str {
        "Fill Layer"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        let color = self.color;
        let saved = apply_masked(doc, self.layer, |_, _, _| color)?;
        self.cost = saved.bytes();
        self.dirty = saved.rect;
        self.saved = Some(saved);
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) {
        if let Some(saved) = self.saved.take() {
            saved.restore(doc);
        }
    }

    fn dirty_rect(&self, _doc: &Document) -> PixelRect {
        self.dirty
    }

    fn memory_cost(&self) -> usize {
        // The saved copy of the region's previous pixels.
        self.cost
    }
}

/// Replaces the document's selection (from a tool or a Select menu operation).
pub struct SetSelection {
    label: String,
    new: Option<GrayImage>,
    old: Option<GrayImage>,
    cost: usize,
}

impl SetSelection {
    pub fn new(label: impl Into<String>, mask: Option<GrayImage>) -> Self {
        Self {
            label: label.into(),
            new: mask,
            old: None,
            cost: std::mem::size_of::<Self>(),
        }
    }
}

impl Command for SetSelection {
    fn label(&self) -> &str {
        &self.label
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        let previous = doc.selection().cloned();
        doc.set_selection(self.new.clone())
            .map_err(|e| CommandError::Invalid(e.to_string()))?;
        let size = |m: &Option<GrayImage>| m.as_ref().map_or(0, |m| m.as_raw().len());
        self.cost = size(&previous) + size(&self.new);
        self.old = previous;
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) {
        // The previous mask was valid for this document when captured.
        let _ = doc.set_selection(self.old.take());
    }

    fn dirty_rect(&self, _doc: &Document) -> PixelRect {
        // Selections change no pixels.
        PixelRect::default()
    }

    fn memory_cost(&self) -> usize {
        // Both masks are kept while in history.
        self.cost
    }

    fn changes_selection(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Layer;

    fn doc_with_pattern() -> Document {
        let mut doc = Document::new("d", 4, 3, 72.0).unwrap();
        let pixels = RgbaImage::from_fn(4, 3, |x, y| {
            Rgba([x as u8 * 60, y as u8 * 80, 9, (x + y) as u8 * 40])
        });
        doc.add_layer(Layer::new("base", pixels)).unwrap();
        doc.add_layer(Layer::transparent("top", 4, 3)).unwrap();
        doc
    }

    /// Records a fixed memory cost; sets a marker pixel on layer 0.
    struct Mark {
        cost: usize,
        value: u8,
        old: Option<Rgba<u8>>,
    }

    impl Command for Mark {
        fn label(&self) -> &str {
            "Mark"
        }
        fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
            let px = doc.layer_mut(0).unwrap().pixels.get_pixel_mut(0, 0);
            self.old = Some(*px);
            *px = Rgba([self.value; 4]);
            Ok(())
        }
        fn revert(&mut self, doc: &mut Document) {
            *doc.layer_mut(0).unwrap().pixels.get_pixel_mut(0, 0) = self.old.take().unwrap();
        }
        fn dirty_rect(&self, _doc: &Document) -> PixelRect {
            PixelRect::new(0, 0, 1, 1)
        }
        fn memory_cost(&self) -> usize {
            self.cost
        }
    }

    fn mark(cost: usize, value: u8) -> Box<dyn Command> {
        Box::new(Mark {
            cost,
            value,
            old: None,
        })
    }

    #[test]
    fn fill_layer_apply_undo_redo_is_exact() {
        let mut doc = doc_with_pattern();
        let original = doc.clone();
        let mut stack = UndoStack::new();

        let change = stack
            .execute(&mut doc, Box::new(FillLayer::new(0, [1, 2, 3, 4])))
            .unwrap();
        assert_eq!(change.dirty, doc.bounds());
        assert!(!change.selection_changed);
        assert!(doc
            .layer(0)
            .unwrap()
            .pixels
            .pixels()
            .all(|p| p.0 == [1, 2, 3, 4]));
        assert_eq!(doc.layer(1), original.layer(1));
        let filled = doc.clone();

        assert_eq!(stack.undo(&mut doc).map(|c| c.dirty), Some(doc.bounds()));
        assert_eq!(doc, original);
        assert_eq!(stack.redo(&mut doc).map(|c| c.dirty), Some(doc.bounds()));
        assert_eq!(doc, filled);
        stack.undo(&mut doc);
        assert_eq!(doc, original);
    }

    #[test]
    fn fill_invalid_layer_is_not_recorded() {
        let mut doc = doc_with_pattern();
        let original = doc.clone();
        let mut stack = UndoStack::new();
        let err = stack
            .execute(&mut doc, Box::new(FillLayer::new(5, [0; 4])))
            .unwrap_err();
        assert_eq!(err, CommandError::NoSuchLayer { index: 5, len: 2 });
        assert!(!stack.can_undo());
        assert_eq!(doc, original);
    }

    #[test]
    fn execute_clears_redo_and_labels_follow() {
        let mut doc = doc_with_pattern();
        let mut stack = UndoStack::new();
        assert!(!stack.can_undo() && !stack.can_redo());
        assert_eq!(stack.undo(&mut doc), None);
        assert_eq!(stack.redo(&mut doc), None);

        stack
            .execute(&mut doc, Box::new(FillLayer::new(0, [9; 4])))
            .unwrap();
        stack.execute(&mut doc, mark(10, 7)).unwrap();
        assert_eq!(stack.undo_label(), Some("Mark"));
        stack.undo(&mut doc);
        assert_eq!(stack.undo_label(), Some("Fill Layer"));
        assert_eq!(stack.redo_label(), Some("Mark"));
        assert!(stack.can_redo());

        stack.execute(&mut doc, mark(10, 8)).unwrap();
        assert!(!stack.can_redo());
        assert_eq!(stack.redo_label(), None);
        assert_eq!((stack.undo_len(), stack.redo_len()), (2, 0));
    }

    #[test]
    fn sequences_undo_in_reverse_order() {
        let mut doc = doc_with_pattern();
        let original = doc.clone();
        let mut stack = UndoStack::new();
        stack.execute(&mut doc, mark(1, 10)).unwrap();
        stack.execute(&mut doc, mark(1, 20)).unwrap();
        stack
            .execute(&mut doc, Box::new(FillLayer::new(1, [5; 4])))
            .unwrap();
        while stack.undo(&mut doc).is_some() {}
        assert_eq!(doc, original);
        while stack.redo(&mut doc).is_some() {}
        assert_eq!(doc.layer(0).unwrap().pixels.get_pixel(0, 0).0, [20; 4]);
        assert!(doc.layer(1).unwrap().pixels.pixels().all(|p| p.0 == [5; 4]));
    }

    #[test]
    fn memory_budget_drops_oldest_but_keeps_latest() {
        let mut doc = doc_with_pattern();
        let mut stack = UndoStack::with_budget(100);
        stack.execute(&mut doc, mark(40, 1)).unwrap();
        stack.execute(&mut doc, mark(40, 2)).unwrap();
        assert_eq!(stack.undo_len(), 2);
        stack.execute(&mut doc, mark(40, 3)).unwrap();
        // 120 > 100: the oldest step goes.
        assert_eq!(stack.undo_len(), 2);
        assert_eq!(stack.memory_used(), 80);
        // A single step over budget is still kept.
        stack.execute(&mut doc, mark(500, 4)).unwrap();
        assert_eq!(stack.undo_len(), 1);
        assert_eq!(stack.memory_used(), 500);
        assert!(stack.undo(&mut doc).is_some());
    }

    #[test]
    fn memory_accounts_for_redo_history() {
        let mut doc = doc_with_pattern();
        let mut stack = UndoStack::with_budget(1000);
        stack.execute(&mut doc, mark(30, 1)).unwrap();
        stack.execute(&mut doc, mark(30, 2)).unwrap();
        stack.undo(&mut doc);
        assert_eq!(stack.memory_used(), 60);
        stack.execute(&mut doc, mark(5, 3)).unwrap();
        assert_eq!(stack.memory_used(), 35);
    }

    #[test]
    fn fill_memory_cost_is_layer_size() {
        let mut doc = Document::new_blank("b", 100, 50, 72.0).unwrap();
        let mut stack = UndoStack::new();
        stack
            .execute(&mut doc, Box::new(FillLayer::new(0, [1; 4])))
            .unwrap();
        assert_eq!(stack.memory_used(), 100 * 50 * 4);
    }

    #[test]
    fn fill_respects_selection_mask() {
        let mut doc = Document::new("d", 6, 4, 72.0).unwrap();
        let base = RgbaImage::from_pixel(6, 4, Rgba([10, 20, 30, 255]));
        doc.add_layer(Layer::new("base", base)).unwrap();
        let mut mask = GrayImage::new(6, 4);
        mask.put_pixel(1, 1, image::Luma([255]));
        mask.put_pixel(2, 1, image::Luma([128]));
        mask.put_pixel(3, 2, image::Luma([255]));
        doc.set_selection(Some(mask)).unwrap();
        let original = doc.clone();
        let mut stack = UndoStack::new();

        let change = stack
            .execute(&mut doc, Box::new(FillLayer::new(0, [250, 0, 0, 255])))
            .unwrap();
        assert_eq!(change.dirty, PixelRect::new(1, 1, 3, 2), "selection bounds");
        let px = |x, y| doc.layer(0).unwrap().pixels.get_pixel(x, y).0;
        assert_eq!(px(1, 1), [250, 0, 0, 255]);
        assert_eq!(px(3, 2), [250, 0, 0, 255]);
        assert_eq!(px(0, 0), [10, 20, 30, 255]);
        assert_eq!(px(2, 2), [10, 20, 30, 255], "inside bounds but unselected");
        let half = px(2, 1);
        assert!(
            (half[0] as i32 - 130).abs() <= 1 && (half[1] as i32 - 10).abs() <= 1,
            "{half:?}"
        );
        // Saved region is only the bounding box.
        assert_eq!(stack.memory_used(), 3 * 2 * 4);

        stack.undo(&mut doc);
        assert_eq!(doc, original);
    }

    #[test]
    fn blend_by_mask_interpolates_premultiplied() {
        let old = Rgba([0, 0, 0, 0]);
        let new = Rgba([200, 100, 50, 255]);
        assert_eq!(blend_by_mask(old, new, 0), old);
        assert_eq!(blend_by_mask(old, new, 255), new);
        // Fading in over transparent keeps the new colour, scaling alpha.
        assert_eq!(blend_by_mask(old, new, 128), Rgba([200, 100, 50, 128]));
        let mid = blend_by_mask(Rgba([0, 0, 0, 255]), Rgba([255, 255, 255, 255]), 51);
        assert_eq!(mid, Rgba([51, 51, 51, 255]));
    }

    #[test]
    fn set_selection_undo_redo_is_exact_and_changes_no_pixels() {
        let mut doc = doc_with_pattern();
        let pixels = doc.clone();
        let mut stack = UndoStack::new();
        let mask = crate::selection::rect_mask(4, 3, (0.0, 0.0), (2.0, 2.0));
        let change = stack
            .execute(
                &mut doc,
                Box::new(SetSelection::new("Rectangle Select", Some(mask.clone()))),
            )
            .unwrap();
        assert!(change.selection_changed && change.dirty.is_empty());
        assert_eq!(doc.selection(), Some(&mask));
        assert_eq!(doc.layers(), pixels.layers());
        assert_eq!(stack.undo_label(), Some("Rectangle Select"));
        assert_eq!(stack.memory_used(), 12);

        let all = crate::selection::select_all(4, 3);
        stack
            .execute(
                &mut doc,
                Box::new(SetSelection::new("Select All", Some(all.clone()))),
            )
            .unwrap();
        stack
            .execute(&mut doc, Box::new(SetSelection::new("Deselect", None)))
            .unwrap();
        assert!(doc.selection().is_none());
        // Old mask (12 bytes) is kept for the deselect.
        assert_eq!(stack.memory_used(), 12 + 24 + 12);

        stack.undo(&mut doc);
        assert_eq!(doc.selection(), Some(&all));
        stack.undo(&mut doc);
        assert_eq!(doc.selection(), Some(&mask));
        stack.undo(&mut doc);
        assert_eq!(doc, pixels);
        stack.redo(&mut doc);
        assert_eq!(doc.selection(), Some(&mask));
    }

    #[test]
    fn wrong_size_selection_is_rejected() {
        let mut doc = doc_with_pattern();
        let mut stack = UndoStack::new();
        let bad = SetSelection::new("Bad", Some(GrayImage::new(2, 2)));
        assert!(stack.execute(&mut doc, Box::new(bad)).is_err());
        assert!(!stack.can_undo());
    }
}
