//! Window-independent application state: the open documents, their undo
//! history, and which one is current. The egui layer drives these
//! operations; tests call them directly.
//!
//! Documents are only modified through [`AppState::execute`], [`AppState::undo`]
//! and [`AppState::redo`]; read access is public.

use std::path::Path;

use crate::document::{Document, DocumentError, Layer, PixelRect, DEFAULT_DPI};
use crate::filters::{self, Filter, Params};
use crate::frame_set::FrameSet;
use crate::io::{self, IoError};
use crate::layers::{self, LayerProps};
use crate::paint::PixelPatch;
use crate::paint::{ActiveStroke, BrushSettings, StrokeKind};
use crate::project::{self, ProjectError};
use crate::transform::{self, Affine, Interpolation, TransformSession};
use crate::undo::{
    target_image, write_target_region, Change, Command, CommandError, PaintTarget, UndoStack,
};

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
    edit_target: EditTarget,
    /// Layer the edit target was chosen on; another active layer edits pixels.
    target_layer: usize,
    /// Undo state id at the last save (or open/creation).
    saved_state: u64,
    dirty: Vec<PixelRect>,
    /// Bumps whenever the selection changes.
    pub selection_revision: u64,
    stroke: Option<ActiveStroke>,
    transform: Option<TransformSession>,
    filter: Option<FilterSession>,
    last_stroke_end: Option<(f32, f32)>,
    /// A preview-only layer (index, previously active layer) shown while a
    /// canvas-source filter runs; never in the undo history.
    temp_layer: Option<(usize, usize)>,
}

impl OpenDocument {
    fn new(id: DocId, document: Document) -> Self {
        Self {
            id,
            document,
            revision: 0,
            undo: UndoStack::new(),
            edit_target: EditTarget::Pixels,
            target_layer: 0,
            saved_state: 0,
            dirty: Vec::new(),
            selection_revision: 0,
            stroke: None,
            transform: None,
            filter: None,
            last_stroke_end: None,
            temp_layer: None,
        }
    }

    /// Index (bottom = 0) of the layer edits target.
    pub fn active_layer(&self) -> usize {
        self.document.active_layer()
    }

    /// Whether edits go to the active layer's pixels or its mask.
    pub fn edit_target(&self) -> EditTarget {
        self.edit_target
    }

    /// The active layer's pixels or mask, per the edit target.
    pub fn paint_target(&self) -> PaintTarget {
        let layer = self.document.active_layer();
        match self.edit_target {
            EditTarget::Pixels => PaintTarget::Pixels(layer),
            EditTarget::Mask => PaintTarget::Mask(layer),
        }
    }

    /// Whether the document differs from its last saved (or opened) state.
    pub fn is_dirty(&self) -> bool {
        self.undo.state_id() != self.saved_state
            || self.stroke.is_some()
            || self.transform.is_some()
            || self.filter.is_some()
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

    /// Abandons an in-progress stroke or transform, restoring the document.
    fn cancel_stroke(&mut self) {
        if let Some(stroke) = self.stroke.take() {
            let rect = stroke.cancel(&mut self.document);
            self.repainted(rect);
        }
        if let Some(session) = self.transform.take() {
            let rect = session.cancel(&mut self.document);
            self.repainted(rect);
            self.selection_revision += 1;
        }
        if let Some(session) = self.filter.take() {
            write_target_region(
                &mut self.document,
                session.target,
                session.rect,
                &session.before,
            );
            self.repainted(Some(session.rect));
        }
        if let Some((index, previous)) = self.temp_layer.take() {
            let _ = self.document.remove_layer(index);
            self.document.set_active_layer(previous);
            self.edit_target = EditTarget::Pixels;
            self.target_layer = previous;
            self.repainted(Some(self.document.bounds()));
        }
    }

    /// Whether a stroke, transform, or filter preview is showing unrecorded
    /// changes (Save and Export wait for it to finish).
    pub fn has_live_edit(&self) -> bool {
        self.stroke.is_some() || self.transform.is_some() || self.filter.is_some()
    }

    /// Whether a preview-only layer (Remove Background from the canvas) is
    /// in the document; layer edits and history wait until it ends.
    pub fn has_temp_layer(&self) -> bool {
        self.temp_layer.is_some()
    }

    /// Whether a filter preview is in progress.
    pub fn is_filtering(&self) -> bool {
        self.filter.is_some()
    }

    /// The transform session in progress, if any.
    pub fn transform(&self) -> Option<&TransformSession> {
        self.transform.as_ref()
    }

    fn changed(&mut self, change: Change) {
        self.revision += 1;
        if !change.dirty.is_empty() {
            self.dirty.push(change.dirty);
        }
        if change.selection_changed {
            self.selection_revision += 1;
        }
        self.normalize_target();
    }

    /// Falls back to editing pixels when the active layer changed or has no mask.
    fn normalize_target(&mut self) {
        let active = self.document.active_layer();
        let has_mask = self
            .document
            .layer(active)
            .is_some_and(|l| l.mask().is_some());
        if !has_mask || active != self.target_layer {
            self.edit_target = EditTarget::Pixels;
        }
        self.target_layer = active;
    }
}

/// A filter dialog in progress: the target as it was, for previews and cancel.
#[derive(Debug)]
struct FilterSession {
    target: PaintTarget,
    snapshot: image::RgbaImage,
    rect: PixelRect,
    before: image::RgbaImage,
}

/// Why a file could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error(transparent)]
    Image(#[from] IoError),
    #[error(transparent)]
    Project(#[from] ProjectError),
}

/// What paint tools edit on the active layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditTarget {
    #[default]
    Pixels,
    Mask,
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
    /// Frames extracted from videos this session.
    pub frame_set: FrameSet,
    /// Assets extracted this session.
    pub assets: crate::asset_library::AssetLibrary,
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

    /// Opens `image` as a new current document with one "Background" layer
    /// (e.g. a frame grabbed from a video). It has no file yet.
    pub fn new_document_from_image(
        &mut self,
        name: impl Into<String>,
        image: image::RgbaImage,
    ) -> Result<DocId, DocumentError> {
        let mut document = Document::new(name, image.width(), image.height(), DEFAULT_DPI)?;
        document.add_layer(Layer::new("Background", image))?;
        Ok(self.insert(document))
    }

    /// Opens animation frames (same size) as a new document with one layer
    /// per frame, "Frame 1" at the bottom.
    pub fn new_document_from_frames(
        &mut self,
        name: impl Into<String>,
        frames: Vec<image::RgbaImage>,
    ) -> Result<DocId, DocumentError> {
        let (w, h) = frames.first().map(|f| f.dimensions()).unwrap_or((1, 1));
        let mut document = Document::new(name, w, h, DEFAULT_DPI)?;
        for (i, frame) in frames.into_iter().enumerate() {
            document.add_layer(Layer::new(format!("Frame {}", i + 1), frame))?;
        }
        Ok(self.insert(document))
    }

    /// Loads an image file into a new current document. On failure nothing changes.
    pub fn open(&mut self, path: &Path) -> Result<DocId, OpenError> {
        let document = if project::is_project_path(path) {
            project::load_project(path)?
        } else {
            io::load_document(path)?
        };
        Ok(self.insert(document))
    }

    /// Writes the document to a `.gac` file, adopting its name and path, and
    /// marks it saved.
    pub fn save(&mut self, id: DocId, path: &Path) -> Result<(), ProjectError> {
        let open = self
            .document(id)
            .ok_or_else(|| ProjectError::Manifest("no such document".to_owned()))?;
        if open.has_live_edit() {
            return Err(ProjectError::Manifest(
                "finish the current edit before saving".to_owned(),
            ));
        }
        project::save_project(&open.document, path)?;
        self.set_document_path(id, path);
        self.mark_saved(id);
        Ok(())
    }

    /// The `.gac` file a document was opened from or saved to, if any.
    pub fn project_path(&self, id: DocId) -> Option<std::path::PathBuf> {
        self.document(id)?
            .document
            .source_path
            .clone()
            .filter(|p| project::is_project_path(p))
    }

    /// Previews layer properties live (e.g. while dragging the opacity
    /// slider) without recording history; finish with [`Self::commit_layer_props`].
    pub fn preview_layer_props(&mut self, id: DocId, index: usize, props: &LayerProps) {
        if let Some(open) = self.document_mut(id).filter(|o| o.temp_layer.is_none()) {
            if let Some(layer) = open.document.layer_mut(index) {
                props.apply_to(layer);
                let bounds = open.document.bounds();
                open.repainted(Some(bounds));
            }
        }
    }

    /// Records the change from `before` to the layer's current properties as one step.
    pub fn commit_layer_props(
        &mut self,
        id: DocId,
        index: usize,
        before: LayerProps,
    ) -> Result<(), CommandError> {
        let open = self
            .document(id)
            .ok_or_else(|| CommandError::Invalid("no such document".to_owned()))?;
        let layer = open
            .document
            .layer(index)
            .ok_or(CommandError::NoSuchLayer {
                index,
                len: open.document.layer_count(),
            })?;
        let after = LayerProps::of(layer);
        if after == before {
            return Ok(());
        }
        self.execute(
            id,
            Box::new(layers::SetLayerProps::new(index, before, after)),
        )
    }

    /// Starts a filter preview on the active layer's pixels or mask.
    pub fn begin_filter(&mut self, id: DocId) -> Result<(), CommandError> {
        let open = self
            .document_mut(id)
            .ok_or_else(|| CommandError::Invalid("no such document".to_owned()))?;
        open.cancel_stroke();
        let target = open.paint_target();
        let snapshot = target_image(&open.document, target)?;
        let rect = open.document.editable_bounds();
        let before = image::imageops::crop_imm(&snapshot, rect.x, rect.y, rect.width, rect.height)
            .to_image();
        open.filter = Some(FilterSession {
            target,
            snapshot,
            rect,
            before,
        });
        Ok(())
    }

    /// Shows `filter` with `params` on the canvas (or the original when `show` is false).
    pub fn preview_filter(&mut self, id: DocId, filter: &dyn Filter, params: &Params, show: bool) {
        let Some(open) = self.document_mut(id) else {
            return;
        };
        let Some(session) = &open.filter else {
            return;
        };
        let pixels = if show {
            filters::render_filter(
                &session.snapshot,
                open.document.selection(),
                session.rect,
                filter,
                params,
            )
        } else {
            session.before.clone()
        };
        let (target, rect) = (session.target, session.rect);
        write_target_region(&mut open.document, target, rect, &pixels);
        open.repainted(Some(rect));
    }

    /// Applies `filter` as one undo step and ends the preview. Returns whether a step was added.
    pub fn commit_filter(&mut self, id: DocId, filter: &dyn Filter, params: &Params) -> bool {
        let Some(open) = self.document_mut(id) else {
            return false;
        };
        let Some(session) = open.filter.take() else {
            return false;
        };
        let after = filters::render_filter(
            &session.snapshot,
            open.document.selection(),
            session.rect,
            filter,
            params,
        );
        // Show the final result even if the preview was off, then record it.
        write_target_region(&mut open.document, session.target, session.rect, &after);
        open.repainted(Some(session.rect));
        if after == session.before {
            return false;
        }
        let patch = PixelPatch::on_target(
            filter.name(),
            session.target,
            session.rect,
            session.before,
            after,
        );
        match open.undo.execute(&mut open.document, Box::new(patch)) {
            Ok(change) => {
                open.changed(change);
                true
            }
            Err(_) => false,
        }
    }

    /// Ends the filter preview, restoring the target.
    pub fn cancel_filter(&mut self, id: DocId) {
        self.cancel_stroke(id);
    }

    /// Starts a filter preview on a temporary `layer` inserted above the
    /// active one (outside the undo history); see [`Self::end_layer_filter`].
    pub fn begin_layer_filter(&mut self, id: DocId, layer: Layer) -> Result<(), CommandError> {
        let open = self
            .document_mut(id)
            .ok_or_else(|| CommandError::Invalid("no such document".to_owned()))?;
        open.cancel_stroke();
        let previous = open.document.active_layer();
        let index = previous + 1;
        open.document
            .insert_layer(index, layer)
            .map_err(|e| CommandError::Invalid(e.to_string()))?;
        open.document.set_active_layer(index);
        open.edit_target = EditTarget::Pixels;
        open.target_layer = index;
        open.repainted(Some(open.document.bounds()));
        // The filter session snapshots the (now active) temporary layer; only
        // then is the layer marked temporary (starting a session cancels
        // previous ones, which would remove it).
        let started = self.begin_filter(id);
        let Some(open) = self.document_mut(id) else {
            return started;
        };
        open.temp_layer = Some((index, previous));
        if started.is_err() {
            open.cancel_stroke();
        }
        started
    }

    /// Ends a [`Self::begin_layer_filter`] preview: the temporary layer is
    /// removed and, with `keep` (undo label, layer name), `filter` applied to
    /// it is added as a new layer in one undo step. Returns whether a layer
    /// was added.
    pub fn end_layer_filter(
        &mut self,
        id: DocId,
        filter: &dyn Filter,
        params: &Params,
        keep: Option<(&'static str, &str)>,
    ) -> bool {
        let Some(open) = self.document_mut(id) else {
            return false;
        };
        let (Some((index, previous)), Some(session)) = (open.temp_layer, &open.filter) else {
            open.cancel_stroke();
            return false;
        };
        let pixels = filters::render_filter(
            &session.snapshot,
            open.document.selection(),
            session.rect,
            filter,
            params,
        );
        let mut layer = open.document.layer(index).cloned();
        if let Some(layer) = &mut layer {
            let r = session.rect;
            image::imageops::replace(&mut layer.pixels, &pixels, r.x as i64, r.y as i64);
        }
        open.cancel_stroke();
        let (Some((label, name)), Some(mut layer)) = (keep, layer) else {
            return false;
        };
        layer.name = name.to_owned();
        self.execute(
            id,
            Box::new(layers::insert_layer(label, previous + 1, layer)),
        )
        .is_ok()
    }

    /// Applies a filter immediately (filters without parameters).
    pub fn apply_filter(
        &mut self,
        id: DocId,
        filter: &dyn Filter,
        params: &Params,
    ) -> Result<bool, CommandError> {
        self.begin_filter(id)?;
        Ok(self.commit_filter(id, filter, params))
    }

    /// Starts a transform session on the active layer (Free Transform or Move).
    pub fn begin_transform(
        &mut self,
        id: DocId,
        interpolation: Interpolation,
    ) -> Result<(), CommandError> {
        let open = self
            .document_mut(id)
            .ok_or_else(|| CommandError::Invalid("no such document".to_owned()))?;
        open.cancel_stroke();
        open.transform = Some(TransformSession::begin(
            &open.document,
            interpolation,
            None,
        )?);
        Ok(())
    }

    /// Updates the in-progress transform.
    pub fn update_transform(&mut self, id: DocId, affine: Affine) {
        if let Some(open) = self.document_mut(id) {
            if let Some(session) = &mut open.transform {
                let rect = session.set_affine(&mut open.document, affine);
                open.repainted(rect);
                if open.document.selection().is_some() {
                    open.selection_revision += 1;
                }
            }
        }
    }

    /// Changes the in-progress transform's interpolation.
    pub fn set_transform_interpolation(&mut self, id: DocId, interpolation: Interpolation) {
        if let Some(open) = self.document_mut(id) {
            if let Some(session) = &mut open.transform {
                let rect = session.set_interpolation(&mut open.document, interpolation);
                open.repainted(rect);
            }
        }
    }

    /// Records the in-progress transform as one step. Returns whether a step was added.
    pub fn commit_transform(&mut self, id: DocId, label: &'static str) -> bool {
        let Some(open) = self.document_mut(id) else {
            return false;
        };
        let Some(session) = open.transform.take() else {
            return false;
        };
        let Some(patch) = session.commit(&open.document, label) else {
            return false;
        };
        match open.undo.execute(&mut open.document, Box::new(patch)) {
            Ok(change) => {
                open.changed(change);
                open.selection_revision += 1;
                true
            }
            Err(_) => false,
        }
    }

    /// Discards the in-progress transform, restoring the layer.
    pub fn cancel_transform(&mut self, id: DocId) {
        self.cancel_stroke(id);
    }

    /// Edit → Flip Horizontal / Vertical. Returns whether a step was added.
    pub fn flip(&mut self, id: DocId, horizontal: bool) -> Result<bool, CommandError> {
        let open = self
            .document_mut(id)
            .ok_or_else(|| CommandError::Invalid("no such document".to_owned()))?;
        open.cancel_stroke();
        match transform::flip(&mut open.document, horizontal)? {
            Some(patch) => self.execute(id, Box::new(patch)).map(|()| true),
            None => Ok(false),
        }
    }

    /// Exports the current document. Returns `Ok(false)` when there is none.
    pub fn export_current(&self, path: &Path) -> Result<bool, IoError> {
        match self.current_document() {
            Some(open) if open.has_live_edit() => Err(IoError::Encode(
                "finish the current edit before exporting".to_owned(),
            )),
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
        if open.temp_layer.is_some() {
            // The preview layer must never reach the history.
            return Err(CommandError::Invalid(
                "finish or cancel Remove Background first".to_owned(),
            ));
        }
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
        let layer = open.document.active_layer();
        let (target, kind) = match open.edit_target {
            EditTarget::Pixels => (PaintTarget::Pixels(layer), kind),
            EditTarget::Mask => (PaintTarget::Mask(layer), kind.for_mask()),
        };
        let (stroke, dirty) =
            ActiveStroke::begin_on(&mut open.document, target, kind, brush, start)?;
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

    /// Sets the layer edits target (its pixels). Out-of-range indices are ignored.
    pub fn set_active_layer(&mut self, id: DocId, index: usize) {
        if let Some(open) = self.document_mut(id) {
            if index < open.document.layer_count() && index != open.document.active_layer() {
                open.cancel_stroke();
                open.document.set_active_layer(index);
                open.edit_target = EditTarget::Pixels;
                open.target_layer = index;
            }
        }
    }

    /// Chooses pixels or mask of the active layer as the edit target
    /// (mask only if the layer has one).
    pub fn set_edit_target(&mut self, id: DocId, target: EditTarget) {
        if let Some(open) = self.document_mut(id) {
            open.cancel_stroke();
            open.edit_target = target;
            open.target_layer = open.document.active_layer();
            open.normalize_target();
        }
    }

    /// Records the current state as saved (clean).
    pub fn mark_saved(&mut self, id: DocId) {
        if let Some(open) = self.document_mut(id) {
            open.saved_state = open.undo.state_id();
        }
    }

    /// Updates a document's name and file path (after Save As).
    pub fn set_document_path(&mut self, id: DocId, path: &Path) {
        if let Some(open) = self.document_mut(id) {
            open.document.source_path = Some(path.to_path_buf());
            open.document.name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| open.document.name.clone());
            open.revision += 1;
        }
    }

    /// Ids of documents with unsaved changes, in open order.
    pub fn dirty_documents(&self) -> Vec<DocId> {
        self.documents
            .iter()
            .filter(|d| d.is_dirty())
            .map(|d| d.id)
            .collect()
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
                        active: index == open.document.active_layer(),
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

    #[test]
    fn dirty_tracking_follows_saved_state() {
        let mut state = AppState::new();
        let a = state.new_document(4, 4, 72.0).unwrap();
        assert!(
            !state.document(a).unwrap().is_dirty(),
            "new documents start clean"
        );
        state
            .execute(a, Box::new(FillLayer::new(0, [1, 2, 3, 255])))
            .unwrap();
        assert!(state.document(a).unwrap().is_dirty());
        assert_eq!(state.dirty_documents(), vec![a]);
        state.undo(a);
        assert!(
            !state.document(a).unwrap().is_dirty(),
            "undo back to the saved state"
        );
        state.redo(a);
        state.mark_saved(a);
        assert!(!state.document(a).unwrap().is_dirty());
        state.undo(a);
        assert!(state.document(a).unwrap().is_dirty());
    }

    #[test]
    fn save_and_open_project_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.gac");
        let mut state = AppState::new();
        let a = state.new_document(6, 4, 96.0).unwrap();
        add(&mut state, a, Layer::transparent("Top", 6, 4));
        state
            .execute(a, Box::new(FillLayer::new(1, [9, 9, 9, 200])))
            .unwrap();
        assert!(state.project_path(a).is_none());
        state.save(a, &path).unwrap();
        let open = state.document(a).unwrap();
        assert!(!open.is_dirty());
        assert_eq!(open.document.name, "scene.gac");
        assert_eq!(state.project_path(a).as_deref(), Some(path.as_path()));
        let saved = state.document(a).unwrap().document.clone();
        let b = state.open(&path).unwrap();
        let reopened = &state.document(b).unwrap().document;
        assert_eq!(reopened.layers(), saved.layers());
        assert_eq!(reopened.active_layer(), saved.active_layer());
        assert!(!state.document(b).unwrap().is_dirty());
        let bad = dir.path().join("bad.gac");
        std::fs::write(&bad, b"nope").unwrap();
        assert!(matches!(state.open(&bad), Err(OpenError::Project(_))));
    }

    #[test]
    fn transform_sessions_commit_cancel_and_yield_to_undo() {
        let mut state = AppState::new();
        let a = state.new_document(10, 10, 72.0).unwrap();
        state
            .execute(a, Box::new(FillLayer::new(0, [255, 0, 0, 255])))
            .unwrap();
        let filled = state.document(a).unwrap().document.clone();
        state.begin_transform(a, Interpolation::Nearest).unwrap();
        let mut affine = state.document(a).unwrap().transform().unwrap().affine();
        affine.translate = (3.0, 0.0);
        state.update_transform(a, affine);
        assert!(state.document(a).unwrap().is_dirty());
        assert_eq!(
            state
                .document(a)
                .unwrap()
                .document
                .layer(0)
                .unwrap()
                .pixels
                .get_pixel(1, 1)[3],
            0
        );
        state.cancel_transform(a);
        assert_eq!(state.document(a).unwrap().document, filled);

        state.begin_transform(a, Interpolation::Nearest).unwrap();
        state.update_transform(a, affine);
        assert!(state.commit_transform(a, "Move"));
        assert_eq!(
            state.document(a).unwrap().undo_stack().undo_label(),
            Some("Move")
        );
        state.undo(a);
        assert_eq!(state.document(a).unwrap().document, filled);

        // Undo during a session cancels it first.
        state.begin_transform(a, Interpolation::Nearest).unwrap();
        state.update_transform(a, affine);
        state.undo(a);
        assert!(state.document(a).unwrap().transform().is_none());
        assert!(state
            .document(a)
            .unwrap()
            .document
            .layer(0)
            .unwrap()
            .pixels
            .pixels()
            .all(|p| p[3] == 0));
    }

    #[test]
    fn flip_and_live_layer_props_record_one_step_each() {
        let mut state = AppState::new();
        let a = state.new_document(4, 1, 72.0).unwrap();
        state
            .execute(a, Box::new(FillLayer::new(0, [255, 255, 255, 255])))
            .unwrap();
        let steps = state.document(a).unwrap().undo_stack().undo_len();
        // A symmetric layer does not change when flipped.
        assert!(!state.flip(a, true).unwrap());
        state
            .begin_stroke(
                a,
                StrokeKind::Brush {
                    color: [0, 0, 0, 255],
                },
                BrushSettings {
                    size: 1.0,
                    pixel: true,
                    ..Default::default()
                },
                (0.5, 0.5),
            )
            .unwrap();
        state.end_stroke(a);
        let steps = steps + 1;
        assert!(state.flip(a, true).unwrap());
        assert_eq!(
            state
                .document(a)
                .unwrap()
                .document
                .layer(0)
                .unwrap()
                .pixels
                .get_pixel(3, 0)[0],
            0
        );
        assert_eq!(
            state.document(a).unwrap().undo_stack().undo_label(),
            Some("Flip Horizontal")
        );

        let before = LayerProps::of(state.document(a).unwrap().document.layer(0).unwrap());
        for opacity in [0.9, 0.6, 0.3] {
            let mut p = before.clone();
            p.opacity = opacity;
            state.preview_layer_props(a, 0, &p);
        }
        assert_eq!(
            state.document(a).unwrap().undo_stack().undo_len(),
            steps + 1,
            "previews not recorded"
        );
        state.commit_layer_props(a, 0, before.clone()).unwrap();
        let open = state.document(a).unwrap();
        assert_eq!(open.undo_stack().undo_len(), steps + 2);
        assert_eq!(open.undo_stack().undo_label(), Some("Layer Opacity"));
        assert_eq!(open.document.layer(0).unwrap().opacity(), 0.3);
        state.undo(a);
        assert_eq!(
            state
                .document(a)
                .unwrap()
                .document
                .layer(0)
                .unwrap()
                .opacity(),
            1.0
        );
    }

    #[test]
    fn mask_edit_target_falls_back_to_pixels() {
        let mut state = AppState::new();
        let a = state.new_document(4, 4, 72.0).unwrap();
        state.set_edit_target(a, EditTarget::Mask);
        assert_eq!(
            state.document(a).unwrap().edit_target(),
            EditTarget::Pixels,
            "no mask yet"
        );
        let cmd = layers::add_mask(&state.document(a).unwrap().document, false).unwrap();
        state.execute(a, Box::new(cmd)).unwrap();
        state.set_edit_target(a, EditTarget::Mask);
        assert_eq!(
            state.document(a).unwrap().paint_target(),
            PaintTarget::Mask(0)
        );
        // A brush stroke on the mask paints gray and leaves pixels alone.
        let pixels = state
            .document(a)
            .unwrap()
            .document
            .layer(0)
            .unwrap()
            .pixels
            .clone();
        state
            .begin_stroke(
                a,
                StrokeKind::Brush {
                    color: [0, 0, 0, 255],
                },
                BrushSettings::default(),
                (2.0, 2.0),
            )
            .unwrap();
        state.end_stroke(a);
        let layer = state
            .document(a)
            .unwrap()
            .document
            .layer(0)
            .unwrap()
            .clone();
        assert_eq!(layer.pixels, pixels);
        assert_eq!(layer.mask().unwrap().get_pixel(2, 2)[0], 0);
        state.undo(a);
        state.undo(a);
        assert_eq!(
            state.document(a).unwrap().edit_target(),
            EditTarget::Pixels,
            "mask removed"
        );
    }

    #[test]
    fn edit_target_returns_to_pixels_when_active_layer_changes() {
        let mut state = AppState::new();
        let a = state.new_document(4, 4, 72.0).unwrap();
        let cmd = layers::add_mask(&state.document(a).unwrap().document, false).unwrap();
        state.execute(a, Box::new(cmd)).unwrap();
        state.set_edit_target(a, EditTarget::Mask);
        // Duplicate makes the (masked) copy active: editing goes back to pixels.
        let dup = layers::duplicate_layer(&state.document(a).unwrap().document).unwrap();
        state.execute(a, Box::new(dup)).unwrap();
        assert_eq!(state.document(a).unwrap().active_layer(), 1);
        assert_eq!(state.document(a).unwrap().edit_target(), EditTarget::Pixels);
        state.set_edit_target(a, EditTarget::Mask);
        // Undo of the duplicate changes the active layer too.
        state.undo(a);
        assert_eq!(state.document(a).unwrap().edit_target(), EditTarget::Pixels);
    }

    #[test]
    fn filter_preview_commit_and_cancel() {
        let mut state = AppState::new();
        let a = state.new_document(8, 4, 72.0).unwrap();
        state
            .execute(a, Box::new(FillLayer::new(0, [10, 20, 30, 255])))
            .unwrap();
        let before = state.document(a).unwrap().document.clone();
        let invert = filters::find("invert").unwrap();
        let params = Params::default();

        state.begin_filter(a).unwrap();
        state.preview_filter(a, invert, &params, true);
        assert_eq!(
            state
                .document(a)
                .unwrap()
                .document
                .layer(0)
                .unwrap()
                .pixels
                .get_pixel(0, 0)
                .0,
            [245, 235, 225, 255]
        );
        assert!(state.document(a).unwrap().is_dirty());
        state.preview_filter(a, invert, &params, false);
        assert_eq!(
            state.document(a).unwrap().document,
            before,
            "preview off shows the original"
        );
        state.cancel_filter(a);
        assert_eq!(state.document(a).unwrap().document, before);
        assert!(!state.document(a).unwrap().is_filtering());

        // OK applies even with the preview off; one labelled step.
        state.begin_filter(a).unwrap();
        state.preview_filter(a, invert, &params, false);
        assert!(state.commit_filter(a, invert, &params));
        let open = state.document(a).unwrap();
        assert_eq!(open.undo_stack().undo_label(), Some("Invert"));
        assert_eq!(
            open.document.layer(0).unwrap().pixels.get_pixel(3, 3).0,
            [245, 235, 225, 255]
        );
        state.undo(a);
        assert_eq!(state.document(a).unwrap().document, before);

        // Undo/redo during a preview cancels it.
        state.begin_filter(a).unwrap();
        state.preview_filter(a, invert, &params, true);
        state.redo(a);
        assert!(!state.document(a).unwrap().is_filtering());
        assert!(state.apply_filter(a, invert, &params).unwrap());
        assert_eq!(
            state.document(a).unwrap().document,
            before,
            "inverted twice"
        );
    }
}
