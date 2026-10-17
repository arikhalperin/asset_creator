//! Command-pattern undo/redo. Every document edit is a [`Command`] executed
//! through an [`UndoStack`] (via `AppState::execute`).

use std::collections::VecDeque;

use image::{Rgba, RgbaImage};
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
    ) -> Result<PixelRect, CommandError> {
        command.apply(doc)?;
        let dirty = command.dirty_rect(doc);
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
        Ok(dirty)
    }

    /// Reverts the most recent command. Returns the region it changed.
    pub fn undo(&mut self, doc: &mut Document) -> Option<PixelRect> {
        let (cost, mut command) = self.undo.pop_back()?;
        command.revert(doc);
        let dirty = command.dirty_rect(doc);
        self.redo.push((cost, command));
        Some(dirty)
    }

    /// Re-applies the most recently undone command. Returns the region it changed.
    pub fn redo(&mut self, doc: &mut Document) -> Option<PixelRect> {
        let (cost, mut command) = self.redo.pop()?;
        if command.apply(doc).is_err() {
            // The document no longer fits this command; forget it.
            self.memory -= cost;
            return None;
        }
        let dirty = command.dirty_rect(doc);
        self.undo.push_back((cost, command));
        Some(dirty)
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

/// Fills an entire layer with one RGBA color.
pub struct FillLayer {
    layer: usize,
    color: Rgba<u8>,
    previous: Option<RgbaImage>,
    cost: usize,
}

impl FillLayer {
    pub fn new(layer: usize, color: [u8; 4]) -> Self {
        Self {
            layer,
            color: Rgba(color),
            previous: None,
            cost: std::mem::size_of::<Self>(),
        }
    }
}

impl Command for FillLayer {
    fn label(&self) -> &str {
        "Fill Layer"
    }

    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        let len = doc.layer_count();
        let layer = doc.layer_mut(self.layer).ok_or(CommandError::NoSuchLayer {
            index: self.layer,
            len,
        })?;
        let filled = RgbaImage::from_pixel(layer.width(), layer.height(), self.color);
        self.cost = filled.as_raw().len();
        self.previous = Some(std::mem::replace(&mut layer.pixels, filled));
        Ok(())
    }

    fn revert(&mut self, doc: &mut Document) {
        if let (Some(previous), Some(layer)) = (self.previous.take(), doc.layer_mut(self.layer)) {
            layer.pixels = previous;
        }
    }

    fn dirty_rect(&self, doc: &Document) -> PixelRect {
        doc.bounds()
    }

    fn memory_cost(&self) -> usize {
        // The saved copy of the layer's previous pixels.
        self.cost
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

        let dirty = stack
            .execute(&mut doc, Box::new(FillLayer::new(0, [1, 2, 3, 4])))
            .unwrap();
        assert_eq!(dirty, doc.bounds());
        assert!(doc
            .layer(0)
            .unwrap()
            .pixels
            .pixels()
            .all(|p| p.0 == [1, 2, 3, 4]));
        assert_eq!(doc.layer(1), original.layer(1));
        let filled = doc.clone();

        assert_eq!(stack.undo(&mut doc), Some(doc.bounds()));
        assert_eq!(doc, original);
        assert_eq!(stack.redo(&mut doc), Some(doc.bounds()));
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
}
