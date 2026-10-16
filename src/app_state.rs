//! Window-independent application state: the open documents and which one is
//! current. The egui layer drives these operations; tests call them directly.

use std::path::Path;

use crate::document::{Document, DocumentError};
use crate::io::{self, IoError};

/// Stable identifier of an open document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocId(pub u64);

/// An open document plus a revision counter that bumps on every change, so
/// views know when to refresh cached textures.
#[derive(Debug)]
pub struct OpenDocument {
    pub id: DocId,
    pub document: Document,
    pub revision: u64,
}

/// One row of the Layers panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerRow {
    pub name: String,
    pub visible: bool,
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

    /// Mutable access to a document. Bumps its revision so views refresh.
    pub fn document_mut(&mut self, id: DocId) -> Option<&mut Document> {
        let open = self.documents.iter_mut().find(|d| d.id == id)?;
        open.revision += 1;
        Some(&mut open.document)
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

    /// Closes a document. If it was current, the most recently opened remaining
    /// document becomes current.
    pub fn close(&mut self, id: DocId) -> Option<Document> {
        let index = self.documents.iter().position(|d| d.id == id)?;
        let closed = self.documents.remove(index);
        if self.current == Some(id) {
            self.current = self.documents.last().map(|d| d.id);
        }
        Some(closed.document)
    }

    /// The current document's layers, top-most first.
    pub fn current_layer_rows(&self) -> Vec<LayerRow> {
        self.current_document()
            .map(|open| {
                open.document
                    .layers()
                    .iter()
                    .rev()
                    .map(|layer| LayerRow {
                        name: layer.name.clone(),
                        visible: layer.visible,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn insert(&mut self, document: Document) -> DocId {
        let id = DocId(self.next_id);
        self.next_id += 1;
        self.documents.push(OpenDocument {
            id,
            document,
            revision: 0,
        });
        self.current = Some(id);
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Layer;

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
    fn document_mut_bumps_revision() {
        let mut state = AppState::new();
        let a = state.new_document(1, 1, 72.0).unwrap();
        assert_eq!(state.document(a).unwrap().revision, 0);
        state.document_mut(a).unwrap().name = "renamed".into();
        assert_eq!(state.document(a).unwrap().revision, 1);
        assert_eq!(state.document(a).unwrap().document.name, "renamed");
    }

    #[test]
    fn layer_rows_list_current_document_top_first() {
        let mut state = AppState::new();
        assert!(state.current_layer_rows().is_empty());
        let a = state.new_document(2, 2, 72.0).unwrap();
        let doc = state.document_mut(a).unwrap();
        doc.add_layer(Layer::transparent("Top", 2, 2)).unwrap();
        let mut hidden = Layer::transparent("Hidden", 2, 2);
        hidden.visible = false;
        doc.insert_layer(1, hidden).unwrap();
        let rows = state.current_layer_rows();
        let rows: Vec<_> = rows.iter().map(|r| (r.name.as_str(), r.visible)).collect();
        assert_eq!(
            rows,
            [("Top", true), ("Hidden", false), ("Background", true)]
        );

        let b = state.new_document(2, 2, 72.0).unwrap();
        assert_eq!(state.current_id(), Some(b));
        assert_eq!(state.current_layer_rows().len(), 1);
    }
}
