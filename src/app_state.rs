//! Window-independent application state: the open documents, their undo
//! history, and which one is current. The egui layer drives these
//! operations; tests call them directly.
//!
//! Documents are only modified through [`AppState::execute`], [`AppState::undo`]
//! and [`AppState::redo`]; read access is public.

use std::path::Path;

use crate::document::{Document, DocumentError, PixelRect};
use crate::io::{self, IoError};
use crate::paint::{ActiveStroke, BrushSettings, StrokeKind};
use crate::undo::{Change, Command, CommandError, UndoStack};

/// Stable identifier of an open document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocId(pub u64);

/// An open document with its undo history, active layer, and the regions
/// changed since the view last refreshed.
#[derive(Debug)]
pub struct OpenDocument {
    pub id: DocId,
    pub document: Document,
    /// Bumps on every change.
    pub revision: u64,
    undo: UndoStack,
    active_layer: usize,
    dirty: Vec<PixelRect>,
    /// Bumps whenever the selection changes.
    pub selection_revision: u64,
    stroke: Option<ActiveStroke>,
    last_stroke_end: Option<(f32, f32)>,
}

impl OpenDocument {
    fn new(id: DocId, document: Document) -> Self {
        let active_layer = document.layer_count().saturating_sub(1);
        Self {
            id,
            document,
            revision: 0,
            undo: UndoStack::new(),
            active_layer,
            dirty: Vec::new(),
            selection_revision: 0,
            stroke: None,
            last_stroke_end: None,
        }
    }

    /// Index (bottom = 0) of the layer edits target.
    pub fn active_layer(&self) -> usize {
        self.active_layer
    }

    /// Whether a paint stroke is in progress.
    pub fn is_stroking(&self) -> bool {
        self.stroke.is_some()
    }

    /// Where the previous finished stroke ended (for Shift+click lines).
    pub fn last_stroke_end(&self) -> Option<(f32, f32)> {
        self.last_stroke_end
    }

    pub fn undo_stack(&self) -> &UndoStack {
        &self.undo
    }

    /// Marks a live-stroke region for refresh without recording history.
    fn repainted(&mut self, rect: Option<PixelRect>) {
        if let Some(rect) = rect {
            self.revision += 1;
            self.dirty.push(rect);
        }
    }

    /// Abandons an in-progress stroke, restoring its pixels.
    fn cancel_stroke(&mut self) {
        if let Some(stroke) = self.stroke.take() {
            let rect = stroke.cancel(&mut self.document);
            self.repainted(rect);
        }
    }

    fn changed(&mut self, change: Change) {
        self.revision += 1;
        if !change.dirty.is_empty() {
            self.dirty.push(change.dirty);
        }
        if change.selection_changed {
            self.selection_revision += 1;
        }
        self.active_layer = self
            .active_layer
            .min(self.document.layer_count().saturating_sub(1));
    }
}

/// One row of the Layers panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerRow {
    /// The layer's index in the document (bottom = 0).
    pub index: usize,
    pub name: String,
    pub visible: bool,
    pub active: bool,
}

#[derive(Debug, Default)]
pub struct AppState {
    documents: Vec<OpenDocument>,
    current: Option<DocId>,
    next_id: u64,
    untitled_count: u32,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open documents in the order they were opened.
    pub fn documents(&self) -> &[OpenDocument] {
        &self.documents
    }

    pub fn document(&self, id: DocId) -> Option<&OpenDocument> {
        self.documents.iter().find(|d| d.id == id)
    }

    fn document_mut(&mut self, id: DocId) -> Option<&mut OpenDocument> {
        self.documents.iter_mut().find(|d| d.id == id)
    }

    pub fn current_id(&self) -> Option<DocId> {
        self.current
    }

    pub fn current_document(&self) -> Option<&OpenDocument> {
        self.current.and_then(|id| self.document(id))
    }

    /// Makes `id` the current document. Unknown ids are ignored.
    pub fn set_current(&mut self, id: DocId) {
        if self.document(id).is_some() {
            self.current = Some(id);
        }
    }

    /// Creates a transparent "Untitled-N" document and makes it current.
    pub fn new_document(
        &mut self,
        width: u32,
        height: u32,
        dpi: f64,
    ) -> Result<DocId, DocumentError> {
        let name = format!("Untitled-{}", self.untitled_count + 1);
        let document = Document::new_blank(name, width, height, dpi)?;
        self.untitled_count += 1;
        Ok(self.insert(document))
    }

    /// Loads an image file into a new current document. On failure nothing changes.
    pub fn open(&mut self, path: &Path) -> Result<DocId, IoError> {
        let document = io::load_document(path)?;
        Ok(self.insert(document))
    }

    /// Exports the current document. Returns `Ok(false)` when there is none.
    pub fn export_current(&self, path: &Path) -> Result<bool, IoError> {
        match self.current_document() {
            Some(open) => io::export_document(&open.document, path).map(|()| true),
            None => Ok(false),
        }
    }

    /// Closes a document and drops its history. If it was current, the most
    /// recently opened remaining document becomes current.
    pub fn close(&mut self, id: DocId) -> Option<Document> {
        let index = self.documents.iter().position(|d| d.id == id)?;
        let closed = self.documents.remove(index);
        if self.current == Some(id) {
            self.current = self.documents.last().map(|d| d.id);
        }
        Some(closed.document)
    }

    /// Applies `command` to a document and records it for undo.
    pub fn execute(&mut self, id: DocId, command: Box<dyn Command>) -> Result<(), CommandError> {
        let open = self
            .document_mut(id)
            .ok_or_else(|| CommandError::Invalid("no such document".to_owned()))?;
        open.cancel_stroke();
        let dirty = open.undo.execute(&mut open.document, command)?;
        open.changed(dirty);
        Ok(())
    }

    /// Undoes the document's most recent command. Returns whether anything changed.
    pub fn undo(&mut self, id: DocId) -> bool {
        let Some(open) = self.document_mut(id) else {
            return false;
        };
        open.cancel_stroke();
        match open.undo.undo(&mut open.document) {
            Some(dirty) => {
                open.changed(dirty);
                true
            }
            None => false,
        }
    }

    /// Redoes the document's most recently undone command. Returns whether anything changed.
    pub fn redo(&mut self, id: DocId) -> bool {
        let Some(open) = self.document_mut(id) else {
            return false;
        };
        open.cancel_stroke();
        match open.undo.redo(&mut open.document) {
            Some(dirty) => {
                open.changed(dirty);
                true
            }
            None => false,
        }
    }

    /// Starts a paint stroke on the active layer at `start`. The document
    /// shows the stroke live; [`Self::end_stroke`] records it as one undo
    /// step and [`Self::cancel_stroke`] discards it.
    pub fn begin_stroke(
        &mut self,
        id: DocId,
        kind: StrokeKind,
        brush: BrushSettings,
        start: (f32, f32),
    ) -> Result<(), CommandError> {
        let open = self
            .document_mut(id)
            .ok_or_else(|| CommandError::Invalid("no such document".to_owned()))?;
        open.cancel_stroke();
        let (stroke, dirty) =
            ActiveStroke::begin(&mut open.document, open.active_layer, kind, brush, start)?;
        open.stroke = Some(stroke);
        open.repainted(dirty);
        Ok(())
    }

    /// Extends the in-progress stroke to `p`.
    pub fn continue_stroke(&mut self, id: DocId, p: (f32, f32)) {
        if let Some(open) = self.document_mut(id) {
            if let Some(stroke) = &mut open.stroke {
                let dirty = stroke.add_point(&mut open.document, p);
                open.repainted(dirty);
            }
        }
    }

    /// Finishes the stroke and records it. Returns whether an undo step was added.
    pub fn end_stroke(&mut self, id: DocId) -> bool {
        let Some(open) = self.document_mut(id) else {
            return false;
        };
        let Some(stroke) = open.stroke.take() else {
            return false;
        };
        let (patch, dirty, end) = stroke.finish(&mut open.document);
        open.repainted(dirty);
        open.last_stroke_end = end;
        match patch {
            Some(patch) => {
                // The document already shows the stroke; applying writes the same pixels.
                match open.undo.execute(&mut open.document, Box::new(patch)) {
                    Ok(change) => {
                        open.changed(change);
                        true
                    }
                    Err(_) => false,
                }
            }
            None => false,
        }
    }

    /// Discards the in-progress stroke on a document, restoring its pixels.
    pub fn cancel_stroke(&mut self, id: DocId) {
        if let Some(open) = self.document_mut(id) {
            open.cancel_stroke();
        }
    }

    /// Discards in-progress strokes on every document.
    pub fn cancel_all_strokes(&mut self) {
        for open in &mut self.documents {
            open.cancel_stroke();
        }
    }

    /// Sets the layer edits target. Out-of-range indices are ignored.
    pub fn set_active_layer(&mut self, id: DocId, index: usize) {
        if let Some(open) = self.document_mut(id) {
            if index < open.document.layer_count() {
                open.active_layer = index;
            }
        }
    }

    /// Returns and clears the regions changed since the last call.
    pub fn take_dirty(&mut self, id: DocId) -> Vec<PixelRect> {
        self.document_mut(id)
            .map(|open| std::mem::take(&mut open.dirty))
            .unwrap_or_default()
    }

    /// The current document's layers, top-most first.
    pub fn current_layer_rows(&self) -> Vec<LayerRow> {
        self.current_document()
            .map(|open| {
                open.document
                    .layers()
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(index, layer)| LayerRow {
                        index,
                        name: layer.name.clone(),
                        visible: layer.visible,
                        active: index == open.active_layer,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn insert(&mut self, document: Document) -> DocId {
        let id = DocId(self.next_id);
        self.next_id += 1;
        self.documents.push(OpenDocument::new(id, document));
        self.current = Some(id);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Layer;
    use crate::undo::FillLayer;

    /// Adds a layer on top (test-only command).
    struct AddLayer(Option<Layer>, usize);

    impl Command for AddLayer {
        fn label(&self) -> &str {
            "Add Layer"
        }
        fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
            let layer = self.0.take().expect("layer");
            self.1 = doc.layer_count();
            doc.add_layer(layer)
                .map_err(|e| CommandError::Invalid(e.to_string()))
        }
        fn revert(&mut self, doc: &mut Document) {
            self.0 = doc.remove_layer(self.1).ok();
        }
        fn dirty_rect(&self, doc: &Document) -> PixelRect {
            doc.bounds()
        }
        fn memory_cost(&self) -> usize {
            0
        }
    }

    fn add(state: &mut AppState, id: DocId, layer: Layer) {
        state
            .execute(id, Box::new(AddLayer(Some(layer), 0)))
            .unwrap();
    }

    #[test]
    fn new_documents_are_numbered_and_current() {
        let mut state = AppState::new();
        let a = state.new_document(4, 4, 72.0).unwrap();
        let b = state.new_document(8, 2, 150.0).unwrap();
        assert_ne!(a, b);
        assert_eq!(state.current_id(), Some(b));
        assert_eq!(state.document(a).unwrap().document.name, "Untitled-1");
        assert_eq!(state.document(b).unwrap().document.name, "Untitled-2");
        assert!(state.new_document(0, 4, 72.0).is_err());
        assert_eq!(state.documents().len(), 2);
    }

    #[test]
    fn closing_current_falls_back_to_latest_remaining() {
        let mut state = AppState::new();
        let a = state.new_document(1, 1, 72.0).unwrap();
        let b = state.new_document(1, 1, 72.0).unwrap();
        let c = state.new_document(1, 1, 72.0).unwrap();
        state.set_current(a);
        assert!(state.close(b).is_some());
        assert_eq!(state.current_id(), Some(a));
        state.close(a);
        assert_eq!(state.current_id(), Some(c));
        state.close(c);
        assert_eq!(state.current_id(), None);
        assert!(state.close(c).is_none());
    }

    #[test]
    fn set_current_ignores_unknown_ids() {
        let mut state = AppState::new();
        let a = state.new_document(1, 1, 72.0).unwrap();
        state.set_current(DocId(999));
        assert_eq!(state.current_id(), Some(a));
    }

    #[test]
    fn execute_undo_redo_track_revision_and_dirty() {
        let mut state = AppState::new();
        let a = state.new_document(3, 2, 72.0).unwrap();
        let original = state.document(a).unwrap().document.clone();
        assert!(state.take_dirty(a).is_empty());

        state
            .execute(a, Box::new(FillLayer::new(0, [9, 8, 7, 6])))
            .unwrap();
        let open = state.document(a).unwrap();
        assert_eq!(open.revision, 1);
        assert!(open.undo_stack().can_undo());
        assert_eq!(state.take_dirty(a), vec![PixelRect::new(0, 0, 3, 2)]);
        assert!(state.take_dirty(a).is_empty());

        assert!(state.undo(a));
        assert_eq!(state.document(a).unwrap().document, original);
        assert_eq!(state.document(a).unwrap().revision, 2);
        assert!(!state.undo(a));
        assert!(state.redo(a));
        assert!(!state.redo(a));
        assert_eq!(state.take_dirty(a).len(), 2);
        assert!(state
            .document(a)
            .unwrap()
            .document
            .layer(0)
            .unwrap()
            .pixels
            .pixels()
            .all(|p| p.0 == [9, 8, 7, 6]));
    }

    #[test]
    fn failed_command_changes_nothing() {
        let mut state = AppState::new();
        let a = state.new_document(2, 2, 72.0).unwrap();
        assert!(state
            .execute(a, Box::new(FillLayer::new(3, [1; 4])))
            .is_err());
        assert!(state
            .execute(DocId(42), Box::new(FillLayer::new(0, [1; 4])))
            .is_err());
        let open = state.document(a).unwrap();
        assert_eq!(open.revision, 0);
        assert!(!open.undo_stack().can_undo());
        assert!(state.take_dirty(a).is_empty());
    }

    #[test]
    fn active_layer_defaults_to_top_and_is_settable() {
        let mut state = AppState::new();
        let a = state.new_document(2, 2, 72.0).unwrap();
        assert_eq!(state.document(a).unwrap().active_layer(), 0);
        add(&mut state, a, Layer::transparent("Top", 2, 2));
        state.set_active_layer(a, 1);
        assert_eq!(state.document(a).unwrap().active_layer(), 1);
        state.set_active_layer(a, 7);
        assert_eq!(state.document(a).unwrap().active_layer(), 1);
        // Undoing the layer addition keeps the active index in range.
        state.undo(a);
        assert_eq!(state.document(a).unwrap().active_layer(), 0);
    }

    #[test]
    fn layer_rows_list_current_document_top_first() {
        let mut state = AppState::new();
        assert!(state.current_layer_rows().is_empty());
        let a = state.new_document(2, 2, 72.0).unwrap();
        add(&mut state, a, Layer::transparent("Middle", 2, 2));
        let mut hidden = Layer::transparent("Top", 2, 2);
        hidden.visible = false;
        add(&mut state, a, hidden);
        state.set_active_layer(a, 1);
        let rows = state.current_layer_rows();
        let rows: Vec<_> = rows
            .iter()
            .map(|r| (r.index, r.name.as_str(), r.visible, r.active))
            .collect();
        assert_eq!(
            rows,
            [
                (2, "Top", false, false),
                (1, "Middle", true, true),
                (0, "Background", true, false)
            ]
        );

        let b = state.new_document(2, 2, 72.0).unwrap();
        assert_eq!(state.current_id(), Some(b));
        assert_eq!(state.current_layer_rows().len(), 1);
    }

    #[test]
    fn stroke_lifecycle_records_one_step() {
        let mut state = AppState::new();
        let a = state.new_document(50, 20, 72.0).unwrap();
        let original = state.document(a).unwrap().document.clone();
        let black = StrokeKind::Brush {
            color: [0, 0, 0, 255],
        };
        state
            .begin_stroke(a, black, BrushSettings::default(), (5.0, 10.0))
            .unwrap();
        assert!(state.document(a).unwrap().is_stroking());
        state.continue_stroke(a, (25.0, 10.0));
        state.continue_stroke(a, (45.0, 10.0));
        assert!(!state.take_dirty(a).is_empty(), "live repaint marks tiles");
        assert!(
            !state.document(a).unwrap().undo_stack().can_undo(),
            "not recorded yet"
        );
        assert!(state.end_stroke(a));
        let open = state.document(a).unwrap();
        assert!(!open.is_stroking());
        assert_eq!(open.undo_stack().undo_label(), Some("Brush Stroke"));
        assert_eq!(open.undo_stack().undo_len(), 1);
        assert_eq!(open.last_stroke_end(), Some((45.0, 10.0)));
        assert_eq!(
            open.document.layer(0).unwrap().pixels.get_pixel(25, 10).0,
            [0, 0, 0, 255]
        );
        assert!(state.undo(a));
        assert_eq!(state.document(a).unwrap().document, original);
    }

    #[test]
    fn cancelled_or_interrupted_strokes_record_nothing() {
        let mut state = AppState::new();
        let a = state.new_document(30, 30, 72.0).unwrap();
        state
            .execute(a, Box::new(FillLayer::new(0, [9, 9, 9, 255])))
            .unwrap();
        let filled = state.document(a).unwrap().document.clone();
        let black = StrokeKind::Brush {
            color: [0, 0, 0, 255],
        };
        state
            .begin_stroke(a, black, BrushSettings::default(), (15.0, 15.0))
            .unwrap();
        state.cancel_stroke(a);
        assert_eq!(state.document(a).unwrap().document, filled);
        assert!(!state.end_stroke(a));

        // Undo during a stroke abandons the stroke first, then undoes the fill.
        state
            .begin_stroke(a, black, BrushSettings::default(), (15.0, 15.0))
            .unwrap();
        assert!(state.undo(a));
        assert!(!state.document(a).unwrap().is_stroking());
        assert!(state
            .document(a)
            .unwrap()
            .document
            .layer(0)
            .unwrap()
            .pixels
            .pixels()
            .all(|p| p[3] == 0));

        state
            .begin_stroke(a, black, BrushSettings::default(), (15.0, 15.0))
            .unwrap();
        state.cancel_all_strokes();
        assert!(!state.document(a).unwrap().is_stroking());
        assert_eq!(state.document(a).unwrap().undo_stack().undo_len(), 0);
    }
}
