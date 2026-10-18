//! The egui front end: menu bar, dockable panels, document tabs, status bar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Key, KeyboardShortcut, Modifiers, TextureHandle};
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

use crate::app_state::{AppState, DocId, EditTarget};
use crate::canvas::{self, CanvasState, ViewAction};
use crate::canvas::{CanvasEvent, TransformBox};
use crate::document::DEFAULT_DPI;
use crate::filter_dialog::{self, DialogOutcome, FilterDialog};
use crate::filters::{self, ParamValue, Params};
use crate::io::FileFormat;
use crate::layers::{self, LayerProps};
use crate::layers_panel::{layers_panel, LayerAction, LayerOp};
use crate::paint::{
    self, mask_gray, BucketFill, GradientColors, GradientFill, GradientKind, StrokeKind,
};
use crate::project;
use crate::selection::{self, CombineMode};
use crate::tools::{self, Colors, SelectionRequest, Tool, ToolOptions};
use crate::transform::{self, Affine, Handle, Interpolation};
use crate::undo::{Command, FillLayer, PaintTarget, SetSelection};
use crate::video::{self, VideoSource};
use crate::video_ui::{
    frame_set_panel, ExtractDialog, ExtractOutcome, FrameSetAction, FrameSetView, Transport,
    VideoAction, VideoId, VideoTab,
};

pub const APP_TITLE: &str = "Game Asset Creator";

/// Largest width/height accepted by File → New.
pub const MAX_NEW_DOCUMENT_SIDE: u32 = 16384;

const SHORTCUT_NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const SHORTCUT_OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SHORTCUT_EXPORT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::E);
const SHORTCUT_CLOSE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
const SHORTCUT_QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
const SHORTCUT_UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const SHORTCUT_REDO: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z);
const SHORTCUT_REDO_ALT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const SHORTCUT_ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
const SHORTCUT_ZOOM_IN_ALT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus);
const SHORTCUT_ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
const SHORTCUT_FIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0);
const SHORTCUT_ACTUAL_SIZE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num1);

/// Fill color used before the user picks one.
pub const DEFAULT_FILL_COLOR: [u8; 4] = [0, 0, 0, 255];

const SHORTCUT_SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SHORTCUT_SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const SHORTCUT_NEW_LAYER: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::N);
const SHORTCUT_DUPLICATE_LAYER: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND, Key::J);
const SHORTCUT_MERGE_DOWN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::E);
const SHORTCUT_LAYER_UP: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND, Key::CloseBracket);
const SHORTCUT_LAYER_DOWN: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND, Key::OpenBracket);
const SHORTCUT_FREE_TRANSFORM: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::T);
/// Shown when painting with the Clone Stamp before choosing a source.
pub const CLONE_SOURCE_HINT: &str = "Option/Alt-click to set a Clone Stamp source";
const SHORTCUT_SELECT_ALL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::A);
const SHORTCUT_DESELECT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::D);
const SHORTCUT_INVERT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::I);

/// Select → Grow / Shrink / Feather.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadiusOp {
    Grow,
    Shrink,
    Feather,
}

impl RadiusOp {
    pub fn title(self) -> &'static str {
        match self {
            Self::Grow => "Grow Selection",
            Self::Shrink => "Shrink Selection",
            Self::Feather => "Feather Selection",
        }
    }

    pub fn default_radius(self) -> u32 {
        match self {
            Self::Grow | Self::Shrink => 2,
            Self::Feather => 5,
        }
    }
}

/// State of the Grow/Shrink/Feather radius dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct RadiusDialog {
    pub op: RadiusOp,
    pub radius: String,
    pub error: Option<String>,
}

impl RadiusDialog {
    /// The radius if it is a whole number from 1 to 100.
    pub fn parse(&self) -> Result<u32, String> {
        match self.radius.trim().parse::<u32>() {
            Ok(r) if (1..=100).contains(&r) => Ok(r),
            _ => Err("Radius must be a whole number from 1 to 100".to_owned()),
        }
    }
}

/// The dockable side panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    Tools,
    Layers,
    AssetLibrary,
    FrameSet,
}

impl Panel {
    pub const ALL: [Panel; 4] = [
        Panel::Tools,
        Panel::Layers,
        Panel::AssetLibrary,
        Panel::FrameSet,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Panel::Tools => "Tools",
            Panel::Layers => "Layers",
            Panel::AssetLibrary => "Asset Library",
            Panel::FrameSet => "Frame Set",
        }
    }
}

/// Anything that can live in a dock tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Panel(Panel),
    Document(DocId),
    Video(VideoId),
}

/// Text fields of the File → New dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct NewDocumentForm {
    pub width: String,
    pub height: String,
    pub dpi: String,
    pub error: Option<String>,
}

impl Default for NewDocumentForm {
    fn default() -> Self {
        Self {
            width: "512".to_owned(),
            height: "512".to_owned(),
            dpi: format!("{DEFAULT_DPI}"),
            error: None,
        }
    }
}

impl NewDocumentForm {
    /// Validates the fields, returning `(width, height, dpi)`.
    pub fn parse(&self) -> Result<(u32, u32, f64), String> {
        let side = |label: &str, text: &str| -> Result<u32, String> {
            match text.trim().parse::<u32>() {
                Ok(v) if (1..=MAX_NEW_DOCUMENT_SIDE).contains(&v) => Ok(v),
                _ => Err(format!(
                    "{label} must be a whole number from 1 to {MAX_NEW_DOCUMENT_SIDE}"
                )),
            }
        };
        let width = side("Width", &self.width)?;
        let height = side("Height", &self.height)?;
        let dpi = match self.dpi.trim().parse::<f64>() {
            Ok(v) if v.is_finite() && v > 0.0 => v,
            _ => return Err("DPI must be a positive number".to_owned()),
        };
        Ok((width, height, dpi))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    New,
    Open,
    Export,
    Close,
    Quit,
    Undo,
    Redo,
    FillLayer,
    View(ViewAction),
    SelectAll,
    Deselect,
    InvertSelection,
    Radius(RadiusOp),
    SetTool(Tool),
    SwapColors,
    ResetColors,
    BrushSize { grow: bool },
    SetPanelOpen(Panel, bool),
    Save,
    SaveAs,
    Layer(LayerOp),
    FreeTransform,
    Flip { horizontal: bool },
    CommitTransform,
    CancelTransform,
    Nudge(i32, i32),
    Filter(&'static str),
    Video(Transport),
}

/// A pending "save changes?" question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsavedPrompt {
    /// Documents still to ask about; the first is being asked now.
    pub queue: Vec<DocId>,
    /// Whether the app quits once every document is handled.
    pub quitting: bool,
}

/// The user's answer to an [`UnsavedPrompt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptAnswer {
    Save,
    DontSave,
    Cancel,
}

/// The application: state, dock layout, canvases, and dialogs.
pub struct EditorApp {
    pub state: AppState,
    dock: DockState<Tab>,
    canvases: HashMap<DocId, CanvasState>,
    checker: Option<TextureHandle>,
    show_pixel_grid: bool,
    new_dialog: Option<NewDocumentForm>,
    /// Color being edited in the open Fill Layer dialog.
    fill_dialog: Option<[u8; 4]>,
    last_fill_color: [u8; 4],
    error: Option<String>,
    configured_context: bool,
    tool: Tool,
    tool_options: ToolOptions,
    radius_dialog: Option<RadiusDialog>,
    /// Current document last frame, to cancel gestures when it changes.
    last_current: Option<DocId>,
    colors: Colors,
    /// Transient hint shown in the status bar.
    status_message: Option<String>,
    /// Free Transform handle drag: handle, grab point, transform at grab.
    transform_drag: Option<(Handle, (f32, f32), Affine)>,
    /// Move tool drag start (image coordinates).
    move_start: Option<(f32, f32)>,
    /// Layers row being renamed and its text.
    rename: Option<(usize, String)>,
    /// Layer properties before the current opacity drag.
    opacity_edit: Option<(DocId, usize, LayerProps)>,
    prompt: Option<UnsavedPrompt>,
    /// Set when the app may close (all unsaved changes handled).
    quit_now: bool,
    /// Document with a Free Transform session (as opposed to a Move drag).
    free_transform: Option<DocId>,
    /// The open filter dialog and the document it previews on.
    filter_dialog: Option<(DocId, FilterDialog)>,
    videos: HashMap<VideoId, VideoTab>,
    next_video: u64,
    /// Video tab with keyboard focus (gets Space / arrows / Home / End).
    focused_video: Option<VideoId>,
    extract_dialog: Option<ExtractDialog>,
    frame_set_view: FrameSetView,
    /// For background workers to request repaints.
    repaint: Option<egui::Context>,
}

impl Default for EditorApp {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorApp {
    pub fn new() -> Self {
        Self {
            state: AppState::new(),
            dock: default_layout(),
            canvases: HashMap::new(),
            checker: None,
            show_pixel_grid: true,
            new_dialog: None,
            fill_dialog: None,
            last_fill_color: DEFAULT_FILL_COLOR,
            error: None,
            configured_context: false,
            tool: Tool::default(),
            tool_options: ToolOptions::default(),
            radius_dialog: None,
            last_current: None,
            colors: Colors::default(),
            status_message: None,
            transform_drag: None,
            move_start: None,
            rename: None,
            opacity_edit: None,
            prompt: None,
            quit_now: false,
            free_transform: None,
            filter_dialog: None,
            videos: HashMap::new(),
            next_video: 0,
            focused_video: None,
            extract_dialog: None,
            frame_set_view: FrameSetView::default(),
            repaint: None,
        }
    }

    /// Lets background workers (video decoding) request repaints; set
    /// automatically on the first frame.
    pub fn set_repaint_context(&mut self, ctx: &egui::Context) {
        self.repaint = Some(ctx.clone());
    }

    /// Opens a video in a new focused tab, or shows an error.
    pub fn open_video(&mut self, path: &Path) -> Option<VideoId> {
        match VideoSource::open(path) {
            Ok(source) => {
                self.next_video += 1;
                let id = VideoId(self.next_video);
                self.videos
                    .insert(id, VideoTab::new(source, self.repaint.as_ref()));
                self.add_center_tab(Tab::Video(id));
                self.focused_video = Some(id);
                Some(id)
            }
            Err(err) => {
                self.error = Some(format!("Could not open {}:\n{err}", path.display()));
                None
            }
        }
    }

    pub fn video(&self, id: VideoId) -> Option<&VideoTab> {
        self.videos.get(&id)
    }

    pub fn video_mut(&mut self, id: VideoId) -> Option<&mut VideoTab> {
        self.videos.get_mut(&id)
    }

    /// Open video ids in dock order.
    pub fn video_tabs(&self) -> Vec<VideoId> {
        self.dock
            .iter_all_tabs()
            .filter_map(|(_, tab)| match tab {
                Tab::Video(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    pub fn focused_video(&self) -> Option<VideoId> {
        self.focused_video
    }

    /// Closes a video tab, stopping its decoder.
    pub fn close_video(&mut self, id: VideoId) {
        if let Some(path) = self.dock.find_tab(&Tab::Video(id)) {
            self.dock.remove_tab(path);
        }
        self.forget_video(id);
    }

    fn forget_video(&mut self, id: VideoId) {
        self.videos.remove(&id);
        if self.focused_video == Some(id) {
            self.focused_video = None;
        }
        if self.extract_dialog.as_ref().is_some_and(|d| d.video == id) {
            self.cancel_extract();
        }
    }

    /// Grab Frame: opens the exact current frame as a new document once it
    /// is decoded (see [`Self::pump_videos`]).
    pub fn grab_frame(&mut self, id: VideoId) -> bool {
        match self.videos.get_mut(&id) {
            Some(tab) => {
                tab.grab();
                true
            }
            None => false,
        }
    }

    /// Handles background results of every video tab and the extraction job;
    /// returns documents opened from grabbed frames.
    pub fn pump_videos(&mut self, ctx: &egui::Context) -> Vec<DocId> {
        let mut grabbed = Vec::new();
        for tab in self.videos.values_mut() {
            if let Some(frame) = tab.poll(ctx) {
                grabbed.push(frame);
            }
        }
        let mut opened = Vec::new();
        for frame in grabbed {
            if let Some(id) = self.open_image_document(frame.name, frame.image) {
                opened.push(id);
            }
        }
        if let Some(dialog) = &mut self.extract_dialog {
            match dialog.take_result() {
                Some(Ok(frames)) => {
                    let added = frames.len();
                    self.state.frame_set.extend(frames);
                    self.extract_dialog = None;
                    self.status_message = Some(format!(
                        "Added {added} frame{} to the Frame Set",
                        if added == 1 { "" } else { "s" }
                    ));
                    self.show_panel(Panel::FrameSet);
                }
                Some(Err(message)) => dialog.error = Some(message),
                None => {}
            }
        }
        opened
    }

    /// Opens `image` as a new document in a focused tab.
    fn open_image_document(&mut self, name: String, image: image::RgbaImage) -> Option<DocId> {
        match self.state.new_document_from_image(name, image) {
            Ok(id) => {
                self.add_document_tab(id);
                Some(id)
            }
            Err(err) => {
                self.error = Some(err.to_string());
                None
            }
        }
    }

    /// Extract Frames…: opens the dialog for a video.
    pub fn open_extract_dialog(&mut self, id: VideoId) -> bool {
        if self.extract_dialog.is_some() {
            return false;
        }
        let Some(tab) = self.videos.get(&id) else {
            return false;
        };
        self.extract_dialog = Some(ExtractDialog::new(id, tab.info().clone()));
        true
    }

    pub fn extract_dialog(&self) -> Option<&ExtractDialog> {
        self.extract_dialog.as_ref()
    }

    pub fn extract_dialog_mut(&mut self) -> Option<&mut ExtractDialog> {
        self.extract_dialog.as_mut()
    }

    /// Starts the extraction with the dialog's settings.
    pub fn start_extract(&mut self) -> bool {
        let notify: video::player::Notify = match &self.repaint {
            Some(ctx) => {
                let ctx = ctx.clone();
                std::sync::Arc::new(move || ctx.request_repaint())
            }
            None => std::sync::Arc::new(|| {}),
        };
        match &mut self.extract_dialog {
            Some(dialog) if !dialog.running() => {
                dialog.start(notify);
                true
            }
            _ => false,
        }
    }

    /// Cancel in the Extract Frames dialog: stops any job and closes it.
    pub fn cancel_extract(&mut self) {
        if let Some(mut dialog) = self.extract_dialog.take() {
            dialog.cancel_job();
        }
    }

    fn show_extract_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.extract_dialog else {
            return;
        };
        match dialog.show(ctx) {
            ExtractOutcome::Open => {}
            ExtractOutcome::Start => {
                self.start_extract();
            }
            ExtractOutcome::Cancel => self.cancel_extract(),
        }
    }

    pub fn frame_set_view(&self) -> &FrameSetView {
        &self.frame_set_view
    }

    pub fn frame_set_view_mut(&mut self) -> &mut FrameSetView {
        &mut self.frame_set_view
    }

    /// Runs a Frame Set panel action; `Open` returns the new document.
    pub fn frame_set_action(&mut self, action: FrameSetAction) -> Option<DocId> {
        match action {
            FrameSetAction::Open(id) => {
                let entry = self.state.frame_set.get(id)?;
                let (name, image) = (entry.document_name(), entry.image());
                self.open_image_document(name, image)
            }
            FrameSetAction::Remove(ids) => {
                self.state.frame_set.remove(&ids);
                None
            }
            FrameSetAction::Clear => {
                self.state.frame_set.clear();
                None
            }
        }
    }

    fn show_panel(&mut self, panel: Panel) {
        self.set_panel_open(panel, true);
        if let Some(path) = self.dock.find_tab(&Tab::Panel(panel)) {
            let _ = self.dock.set_active_tab(path);
        }
    }

    /// Filter → <filter>: opens its preview dialog, or applies it immediately
    /// when it has no parameters. Returns whether anything happened.
    pub fn open_filter(&mut self, filter_id: &str) -> bool {
        let (Some(filter), Some(id)) = (filters::find(filter_id), self.state.current_id()) else {
            return false;
        };
        if self.transform_active() || self.filter_dialog.is_some() {
            return false;
        }
        if filter.params().is_empty() {
            return match self.state.apply_filter(id, filter, &Params::default()) {
                Ok(changed) => changed,
                Err(err) => {
                    self.error = Some(format!("{}: {err}", filter.name()));
                    false
                }
            };
        }
        if let Err(err) = self.state.begin_filter(id) {
            self.error = Some(format!("{}: {err}", filter.name()));
            return false;
        }
        self.filter_dialog = Some((id, FilterDialog::new(filter)));
        self.refresh_filter_preview();
        true
    }

    /// The open filter dialog, if any.
    pub fn filter_dialog(&self) -> Option<&FilterDialog> {
        self.filter_dialog.as_ref().map(|(_, d)| d)
    }

    /// Sets a parameter of the open filter dialog and refreshes the preview.
    pub fn set_filter_param(&mut self, key: &'static str, value: ParamValue) {
        if let Some((_, dialog)) = &mut self.filter_dialog {
            dialog.params.set(key, value);
            let schema = dialog.filter().params();
            dialog.params.clamp(&schema);
        }
        self.refresh_filter_preview();
    }

    /// Turns the dialog's Preview checkbox on or off.
    pub fn set_filter_preview(&mut self, on: bool) {
        if let Some((_, dialog)) = &mut self.filter_dialog {
            dialog.preview = on;
        }
        self.refresh_filter_preview();
    }

    /// The dialog's Reset button.
    pub fn reset_filter(&mut self) {
        if let Some((_, dialog)) = &mut self.filter_dialog {
            dialog.reset();
        }
        self.refresh_filter_preview();
    }

    /// The dialog's OK button. Returns whether an undo step was added.
    pub fn filter_ok(&mut self) -> bool {
        let Some((id, dialog)) = self.filter_dialog.take() else {
            return false;
        };
        self.state
            .commit_filter(id, dialog.filter(), &dialog.params)
    }

    /// The dialog's Cancel button.
    pub fn filter_cancel(&mut self) {
        if let Some((id, _)) = self.filter_dialog.take() {
            self.state.cancel_filter(id);
        }
    }

    fn refresh_filter_preview(&mut self) {
        if let Some((id, dialog)) = &self.filter_dialog {
            self.state
                .preview_filter(*id, dialog.filter(), &dialog.params, dialog.preview);
        }
    }

    fn show_filter_dialog_window(&mut self, ctx: &egui::Context) {
        // The preview may have been cancelled elsewhere (undo, document switch).
        if let Some((id, _)) = &self.filter_dialog {
            if !self.state.document(*id).is_some_and(|o| o.is_filtering()) {
                self.filter_dialog = None;
            }
        }
        let Some((_, dialog)) = &mut self.filter_dialog else {
            return;
        };
        match filter_dialog::show(ctx, dialog) {
            DialogOutcome::Open { changed: true } => self.refresh_filter_preview(),
            DialogOutcome::Open { changed: false } => {}
            DialogOutcome::Ok => {
                self.filter_ok();
            }
            DialogOutcome::Cancel => self.filter_cancel(),
        }
    }

    /// Runs a Layer menu / Layers panel operation on the current document.
    /// Returns whether it changed anything.
    pub fn layer_op(&mut self, op: LayerOp) -> bool {
        let Some(open) = self.state.current_document() else {
            return false;
        };
        let id = open.id;
        let doc = &open.document;
        let command: Option<Box<dyn Command>> = match op {
            LayerOp::New => Some(Box::new(layers::new_layer(doc))),
            LayerOp::Duplicate => {
                layers::duplicate_layer(doc).map(|c| Box::new(c) as Box<dyn Command>)
            }
            LayerOp::Delete => layers::delete_layer(doc).map(|c| Box::new(c) as Box<dyn Command>),
            LayerOp::MoveUp => {
                layers::move_layer(doc, true).map(|c| Box::new(c) as Box<dyn Command>)
            }
            LayerOp::MoveDown => {
                layers::move_layer(doc, false).map(|c| Box::new(c) as Box<dyn Command>)
            }
            LayerOp::MergeDown => layers::merge_down(doc).map(|c| Box::new(c) as Box<dyn Command>),
            LayerOp::Flatten => Some(Box::new(layers::flatten(doc))),
            LayerOp::AddMask => {
                layers::add_mask(doc, false).map(|c| Box::new(c) as Box<dyn Command>)
            }
            LayerOp::AddMaskFromSelection => {
                layers::add_mask(doc, true).map(|c| Box::new(c) as Box<dyn Command>)
            }
            LayerOp::DeleteMask => {
                layers::delete_mask(doc).map(|c| Box::new(c) as Box<dyn Command>)
            }
            LayerOp::ApplyMask => layers::apply_mask(doc).map(|c| Box::new(c) as Box<dyn Command>),
            LayerOp::ToggleMask => layers::set_props(doc, doc.active_layer(), |p| {
                p.mask_enabled = !p.mask_enabled
            })
            .ok()
            .flatten()
            .map(|c| Box::new(c) as Box<dyn Command>),
            LayerOp::EditPixels => {
                self.state.set_edit_target(id, EditTarget::Pixels);
                return true;
            }
            LayerOp::EditMask => {
                self.state.set_edit_target(id, EditTarget::Mask);
                return true;
            }
        };
        let Some(command) = command else {
            return false;
        };
        match self.state.execute(id, command) {
            Ok(()) => {
                if matches!(op, LayerOp::AddMask | LayerOp::AddMaskFromSelection) {
                    self.state.set_edit_target(id, EditTarget::Mask);
                }
                true
            }
            Err(err) => {
                self.error = Some(format!("{}: {err}", op.label()));
                false
            }
        }
    }

    /// Applies a Layers panel action.
    pub fn layer_action(&mut self, action: LayerAction) {
        let Some(id) = self.state.current_id() else {
            return;
        };
        let active = self
            .state
            .current_document()
            .map_or(0, |o| o.active_layer());
        match action {
            LayerAction::Select(index) => self.state.set_active_layer(id, index),
            LayerAction::ToggleVisible(index) => {
                self.set_layer_props(id, index, |p| p.visible = !p.visible);
            }
            LayerAction::StartRename(index) => {
                let name = self
                    .state
                    .document(id)
                    .and_then(|o| o.document.layer(index))
                    .map(|l| l.name.clone())
                    .unwrap_or_default();
                self.rename = Some((index, name));
            }
            LayerAction::Rename(index, name) => {
                self.rename = None;
                if !name.trim().is_empty() {
                    self.set_layer_props(id, index, |p| p.name = name.trim().to_owned());
                }
            }
            LayerAction::CancelRename => self.rename = None,
            LayerAction::EditMask(index) => {
                self.state.set_active_layer(id, index);
                self.state.set_edit_target(id, EditTarget::Mask);
            }
            LayerAction::Blend(mode) => {
                self.set_layer_props(id, active, |p| p.blend_mode = mode);
            }
            LayerAction::OpacityPreview(opacity) => {
                let Some(layer) = self
                    .state
                    .document(id)
                    .and_then(|o| o.document.layer(active))
                else {
                    return;
                };
                let before = match &self.opacity_edit {
                    Some((doc, index, props)) if *doc == id && *index == active => props.clone(),
                    _ => LayerProps::of(layer),
                };
                let mut props = LayerProps::of(layer);
                props.opacity = opacity;
                self.opacity_edit = Some((id, active, before));
                self.state.preview_layer_props(id, active, &props);
            }
            LayerAction::OpacityCommit => {
                if let Some((doc, index, before)) = self.opacity_edit.take() {
                    let _ = self.state.commit_layer_props(doc, index, before);
                }
            }
            LayerAction::Op(op) => {
                self.layer_op(op);
            }
        }
    }

    fn set_layer_props(&mut self, id: DocId, index: usize, edit: impl FnOnce(&mut LayerProps)) {
        let Some(open) = self.state.document(id) else {
            return;
        };
        if let Ok(Some(command)) = layers::set_props(&open.document, index, edit) {
            if let Err(err) = self.state.execute(id, Box::new(command)) {
                self.error = Some(format!("Could not change the layer: {err}"));
            }
        }
    }

    /// Edit → Free Transform on the current document.
    pub fn begin_free_transform(&mut self) -> bool {
        let Some(id) = self.state.current_id() else {
            return false;
        };
        self.transform_drag = None;
        match self
            .state
            .begin_transform(id, self.tool_options.transform_interpolation)
        {
            Ok(()) => {
                self.free_transform = Some(id);
                true
            }
            Err(err) => {
                self.error = Some(format!("Could not transform: {err}"));
                false
            }
        }
    }

    /// Whether the current document has a Free Transform or move in progress.
    pub fn transform_active(&self) -> bool {
        self.state
            .current_document()
            .is_some_and(|o| o.transform().is_some() && self.free_transform == Some(o.id))
    }

    /// Commits the current Free Transform (Enter).
    pub fn commit_free_transform(&mut self) -> bool {
        self.transform_drag = None;
        self.state
            .current_id()
            .is_some_and(|id| self.state.commit_transform(id, "Free Transform"))
    }

    /// Cancels the current Free Transform (Escape).
    pub fn cancel_free_transform(&mut self) {
        self.transform_drag = None;
        if let Some(id) = self.state.current_id() {
            self.state.cancel_transform(id);
        }
    }

    /// Edit → Flip Horizontal / Vertical.
    pub fn flip_current(&mut self, horizontal: bool) -> bool {
        let Some(id) = self.state.current_id() else {
            return false;
        };
        match self.state.flip(id, horizontal) {
            Ok(changed) => changed,
            Err(err) => {
                self.error = Some(format!("Could not flip: {err}"));
                false
            }
        }
    }

    /// Moves the active layer content by whole pixels as one "Move" step.
    pub fn nudge(&mut self, dx: i32, dy: i32) -> bool {
        let Some(id) = self.state.current_id() else {
            return false;
        };
        if self
            .state
            .begin_transform(id, Interpolation::Nearest)
            .is_err()
        {
            return false;
        }
        let Some(mut affine) = self
            .state
            .document(id)
            .and_then(|o| o.transform())
            .map(|t| t.affine())
        else {
            return false;
        };
        affine.translate = (dx as f32, dy as f32);
        self.state.update_transform(id, affine);
        self.state.commit_transform(id, "Move")
    }

    /// Saves the current document to `path` as `.gac`.
    pub fn save_to(&mut self, path: &Path) -> bool {
        let Some(id) = self.state.current_id() else {
            return false;
        };
        self.save_document_to(id, path)
    }

    fn save_document_to(&mut self, id: DocId, path: &Path) -> bool {
        let mut path = path.to_path_buf();
        if !project::is_project_path(&path) {
            path.set_extension(project::EXTENSION);
        }
        match self.state.save(id, &path) {
            Ok(()) => true,
            Err(err) => {
                self.error = Some(format!("Could not save {}:\n{err}", path.display()));
                false
            }
        }
    }

    /// File → Save: writes to the document's `.gac` file, or asks where.
    fn save_document(&mut self, id: DocId, ask: bool) -> bool {
        let path = match self.state.project_path(id) {
            Some(path) if !ask => Some(path),
            _ => {
                let name = self
                    .state
                    .document(id)
                    .map(|o| o.document.name.clone())
                    .unwrap_or_default();
                save_project_dialog(&name)
            }
        };
        path.is_some_and(|p| self.save_document_to(id, &p))
    }

    /// Closes a document, asking first if it has unsaved changes.
    pub fn request_close(&mut self, id: DocId) {
        if self.state.document(id).is_some_and(|o| o.is_dirty()) {
            self.prompt = Some(UnsavedPrompt {
                queue: vec![id],
                quitting: false,
            });
        } else {
            self.close_document(id);
        }
    }

    /// Quits, asking about each document with unsaved changes first.
    pub fn request_quit(&mut self) {
        let dirty = self.state.dirty_documents();
        if dirty.is_empty() {
            self.quit_now = true;
        } else {
            self.prompt = Some(UnsavedPrompt {
                queue: dirty,
                quitting: true,
            });
        }
    }

    /// The "save changes?" question being asked, if any.
    pub fn unsaved_prompt(&self) -> Option<&UnsavedPrompt> {
        self.prompt.as_ref()
    }

    /// Whether the app has decided to close.
    pub fn wants_quit(&self) -> bool {
        self.quit_now
    }

    /// Answers the current "save changes?" question.
    pub fn answer_prompt(&mut self, answer: PromptAnswer) {
        let Some(mut prompt) = self.prompt.take() else {
            return;
        };
        let Some(&id) = prompt.queue.first() else {
            return;
        };
        match answer {
            PromptAnswer::Cancel => return,
            PromptAnswer::Save => {
                if !self.save_document(id, false) {
                    return; // Save cancelled or failed: stop.
                }
                self.close_document(id);
            }
            PromptAnswer::DontSave => self.close_document(id),
        }
        prompt.queue.remove(0);
        if prompt.queue.is_empty() {
            if prompt.quitting {
                self.quit_now = true;
            }
        } else {
            self.prompt = Some(prompt);
        }
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// Switches tools, abandoning any gesture in progress.
    pub fn set_tool(&mut self, tool: Tool) {
        if tool != self.tool {
            self.tool = tool;
            self.status_message = None;
            self.state.cancel_all_strokes();
            self.canvases
                .values_mut()
                .for_each(CanvasState::cancel_gesture);
        }
    }

    pub fn colors(&self) -> &Colors {
        &self.colors
    }

    pub fn colors_mut(&mut self) -> &mut Colors {
        &mut self.colors
    }

    /// The transient status-bar hint, if any.
    pub fn status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
    }

    /// Carries out what a canvas tool asked for on document `id`.
    pub fn handle_canvas_event(&mut self, id: DocId, event: CanvasEvent) {
        let Some(open) = self.state.document(id) else {
            return;
        };
        let active = open.active_layer();
        match event {
            CanvasEvent::Selection(request) => {
                if Some(id) == self.state.current_id() {
                    self.apply_selection_request(&request);
                }
            }
            CanvasEvent::StrokeBegin { point, straight } => {
                let start = if straight {
                    open.last_stroke_end().unwrap_or(point)
                } else {
                    point
                };
                let Some(brush) = self.tool_options.brush_for(self.tool).copied() else {
                    return;
                };
                let kind = match self.tool {
                    Tool::Brush => StrokeKind::Brush {
                        color: self.colors.foreground,
                    },
                    Tool::Eraser => StrokeKind::Eraser,
                    Tool::CloneStamp => {
                        let Some(canvas) = self.canvases.get_mut(&id) else {
                            return;
                        };
                        let Some(source) = canvas.clone_source() else {
                            self.status_message = Some(CLONE_SOURCE_HINT.to_owned());
                            return;
                        };
                        let offset = match canvas.clone_offset() {
                            Some(offset) if self.tool_options.clone_aligned => offset,
                            _ => (source.0 - start.0, source.1 - start.1),
                        };
                        canvas.set_clone_offset(offset);
                        StrokeKind::Clone { offset }
                    }
                    _ => return,
                };
                self.status_message = None;
                if let Err(err) = self.state.begin_stroke(id, kind, brush, start) {
                    self.error = Some(format!("Could not paint: {err}"));
                    return;
                }
                if straight {
                    self.state.continue_stroke(id, point);
                }
            }
            CanvasEvent::StrokeMove(p) => self.state.continue_stroke(id, p),
            CanvasEvent::StrokeEnd => {
                self.state.end_stroke(id);
            }
            CanvasEvent::CloneSource(p) => {
                if let Some(canvas) = self.canvases.get_mut(&id) {
                    canvas.set_clone_source(p);
                }
                self.status_message = None;
            }
            CanvasEvent::Pick { pixel, background } => {
                let all = self.tool_options.sample_all_layers;
                if let Some(color) =
                    paint::sample_color(&open.document, active, pixel.0, pixel.1, all)
                {
                    if background {
                        self.colors.background = color;
                    } else {
                        self.colors.foreground = color;
                    }
                }
            }
            CanvasEvent::Bucket(seed) => {
                let target = open.paint_target();
                let color = target_color(target, self.colors.foreground);
                let o = &self.tool_options;
                let command = BucketFill::new(
                    active,
                    seed,
                    color,
                    o.bucket_tolerance,
                    o.bucket_contiguous,
                    o.bucket_opacity,
                )
                .on_target(target);
                let _ = self.state.execute(id, Box::new(command));
            }
            CanvasEvent::Gradient { start, end } => {
                let target = open.paint_target();
                let o = &self.tool_options;
                let command = GradientFill::new(
                    active,
                    start,
                    end,
                    o.gradient_kind,
                    o.gradient_colors,
                    target_color(target, self.colors.foreground),
                    target_color(target, self.colors.background),
                    o.gradient_opacity,
                )
                .on_target(target);
                // Drags shorter than a pixel are rejected by the command.
                let _ = self.state.execute(id, Box::new(command));
            }
            CanvasEvent::MoveBegin(point) => {
                if self
                    .state
                    .begin_transform(id, Interpolation::Nearest)
                    .is_ok()
                {
                    self.move_start = Some(point);
                }
            }
            CanvasEvent::MoveDrag(point) => {
                let (Some(start), Some(mut affine)) =
                    (self.move_start, open.transform().map(|t| t.affine()))
                else {
                    return;
                };
                // The Move tool moves by whole pixels, so moves stay exact.
                affine.translate = ((point.0 - start.0).round(), (point.1 - start.1).round());
                self.state.update_transform(id, affine);
            }
            CanvasEvent::MoveEnd => {
                if self.move_start.take().is_some() {
                    self.state.commit_transform(id, "Move");
                }
            }
            CanvasEvent::TransformGrab { handle, point, .. } => {
                if let Some(t) = open.transform() {
                    self.transform_drag = Some((handle, point, t.affine()));
                }
            }
            CanvasEvent::TransformDrag { point, shift } => {
                let (Some((handle, start, initial)), Some(t)) =
                    (self.transform_drag, open.transform())
                else {
                    return;
                };
                let affine =
                    transform::drag(&initial, t.content_bounds(), handle, start, point, shift);
                self.state.update_transform(id, affine);
            }
            CanvasEvent::TransformRelease => self.transform_drag = None,
        }
    }

    pub fn tool_options(&self) -> &ToolOptions {
        &self.tool_options
    }

    pub fn tool_options_mut(&mut self) -> &mut ToolOptions {
        &mut self.tool_options
    }

    /// Applies a finished selection gesture to the current document.
    /// Returns whether the selection changed.
    pub fn apply_selection_request(&mut self, request: &SelectionRequest) -> bool {
        let Some(open) = self.state.current_document() else {
            return false;
        };
        let command = tools::selection_command(
            &open.document,
            open.active_layer(),
            request,
            &self.tool_options,
        );
        self.execute_selection(command)
    }

    fn execute_selection(&mut self, command: Option<SetSelection>) -> bool {
        let (Some(command), Some(id)) = (command, self.state.current_id()) else {
            return false;
        };
        match self.state.execute(id, Box::new(command)) {
            Ok(()) => true,
            Err(err) => {
                self.error = Some(format!("Could not change the selection: {err}"));
                false
            }
        }
    }

    /// Select → All.
    pub fn select_all(&mut self) -> bool {
        let Some(open) = self.state.current_document() else {
            return false;
        };
        let doc = &open.document;
        let all = selection::select_all(doc.width(), doc.height());
        let command =
            (doc.selection() != Some(&all)).then(|| SetSelection::new("Select All", Some(all)));
        self.execute_selection(command)
    }

    /// Select → None.
    pub fn deselect(&mut self) -> bool {
        let has_selection = self
            .state
            .current_document()
            .is_some_and(|open| open.document.selection().is_some());
        self.execute_selection(has_selection.then(|| SetSelection::new("Deselect", None)))
    }

    /// Select → Invert.
    pub fn invert_selection(&mut self) -> bool {
        let Some(open) = self.state.current_document() else {
            return false;
        };
        let doc = &open.document;
        let inverted = selection::invert(doc.selection(), doc.width(), doc.height());
        self.execute_selection(Some(SetSelection::new("Invert Selection", inverted)))
    }

    /// Opens the Grow/Shrink/Feather dialog when there is a selection.
    pub fn open_radius_dialog(&mut self, op: RadiusOp) {
        let has_selection = self
            .state
            .current_document()
            .is_some_and(|open| open.document.selection().is_some());
        if has_selection {
            self.radius_dialog = Some(RadiusDialog {
                op,
                radius: op.default_radius().to_string(),
                error: None,
            });
        }
    }

    pub fn radius_dialog(&self) -> Option<&RadiusDialog> {
        self.radius_dialog.as_ref()
    }

    pub fn set_radius_text(&mut self, text: &str) {
        if let Some(dialog) = &mut self.radius_dialog {
            dialog.radius = text.to_owned();
        }
    }

    pub fn cancel_radius_dialog(&mut self) {
        self.radius_dialog = None;
    }

    /// Applies the dialog's operation. Invalid input keeps the dialog open
    /// with an inline error. Returns whether the selection changed.
    pub fn submit_radius_dialog(&mut self) -> bool {
        let Some(dialog) = self.radius_dialog.take() else {
            return false;
        };
        let radius = match dialog.parse() {
            Ok(r) => r,
            Err(message) => {
                self.radius_dialog = Some(RadiusDialog {
                    error: Some(message),
                    ..dialog
                });
                return false;
            }
        };
        let Some(mask) = self
            .state
            .current_document()
            .and_then(|o| o.document.selection())
        else {
            return false;
        };
        let result = match dialog.op {
            RadiusOp::Grow => selection::grow(mask, radius),
            RadiusOp::Shrink => selection::shrink(mask, radius),
            RadiusOp::Feather => selection::feather(mask, radius),
        };
        let command =
            (result.as_ref() != Some(mask)).then(|| SetSelection::new(dialog.op.title(), result));
        self.execute_selection(command)
    }

    /// The canvas state of an open document.
    pub fn canvas(&self, id: DocId) -> Option<&CanvasState> {
        self.canvases.get(&id)
    }

    pub fn show_pixel_grid(&self) -> bool {
        self.show_pixel_grid
    }

    pub fn set_show_pixel_grid(&mut self, show: bool) {
        self.show_pixel_grid = show;
    }

    /// Queues a zoom/fit command for the current document's canvas.
    pub fn view_action(&mut self, action: ViewAction) {
        if let Some(id) = self.state.current_id() {
            if let Some(canvas) = self.canvases.get_mut(&id) {
                canvas.request(action);
            }
        }
    }

    pub fn undo_current(&mut self) -> bool {
        self.state
            .current_id()
            .is_some_and(|id| self.state.undo(id))
    }

    pub fn redo_current(&mut self) -> bool {
        self.state
            .current_id()
            .is_some_and(|id| self.state.redo(id))
    }

    /// Opens Edit → Fill Layer… with the last used color.
    pub fn open_fill_dialog(&mut self) {
        if self.state.current_document().is_some() {
            self.fill_dialog = Some(self.last_fill_color);
        }
    }

    /// The color in the open Fill Layer dialog, if it is open.
    pub fn fill_dialog_color(&self) -> Option<[u8; 4]> {
        self.fill_dialog
    }

    pub fn set_fill_dialog_color(&mut self, color: [u8; 4]) {
        if let Some(current) = &mut self.fill_dialog {
            *current = color;
        }
    }

    pub fn cancel_fill_dialog(&mut self) {
        self.fill_dialog = None;
    }

    /// Fills the current document's active layer with the dialog color as
    /// one undoable step, remembering the color. Returns whether it filled.
    pub fn submit_fill_dialog(&mut self) -> bool {
        let Some(color) = self.fill_dialog.take() else {
            return false;
        };
        self.last_fill_color = color;
        let Some(open) = self.state.current_document() else {
            return false;
        };
        let (id, layer) = (open.id, open.active_layer());
        match self
            .state
            .execute(id, Box::new(FillLayer::new(layer, color)))
        {
            Ok(()) => true,
            Err(err) => {
                self.error = Some(format!("Could not fill layer: {err}"));
                false
            }
        }
    }

    pub fn dock(&self) -> &DockState<Tab> {
        &self.dock
    }

    /// The error message currently shown, if any.
    pub fn error_message(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn dismiss_error(&mut self) {
        self.error = None;
    }

    pub fn new_dialog(&self) -> Option<&NewDocumentForm> {
        self.new_dialog.as_ref()
    }

    /// Opens the File → New dialog with default values.
    pub fn show_new_dialog(&mut self) {
        self.new_dialog = Some(NewDocumentForm::default());
    }

    /// Submits the File → New dialog. Invalid values keep the dialog open with
    /// an inline error; valid values create the document and close it.
    pub fn submit_new_dialog(&mut self, form: NewDocumentForm) -> Option<DocId> {
        match form.parse() {
            Ok((width, height, dpi)) => {
                self.new_dialog = None;
                self.create_document(width, height, dpi)
            }
            Err(message) => {
                self.new_dialog = Some(NewDocumentForm {
                    error: Some(message),
                    ..form
                });
                None
            }
        }
    }

    /// Creates a blank document in a new focused tab.
    pub fn create_document(&mut self, width: u32, height: u32, dpi: f64) -> Option<DocId> {
        match self.state.new_document(width, height, dpi) {
            Ok(id) => {
                self.add_document_tab(id);
                Some(id)
            }
            Err(err) => {
                self.error = Some(err.to_string());
                None
            }
        }
    }

    /// Opens an image file in a new focused tab, or shows an error.
    /// Videos open in a video tab instead (and return `None`).
    pub fn open_path(&mut self, path: &Path) -> Option<DocId> {
        if video::is_video_path(path) {
            self.open_video(path);
            return None;
        }
        match self.state.open(path) {
            Ok(id) => {
                self.add_document_tab(id);
                Some(id)
            }
            Err(err) => {
                self.error = Some(format!("Could not open {}:\n{err}", path.display()));
                None
            }
        }
    }

    /// Exports the current document to `path`, showing an error on failure.
    /// Returns whether a file was written.
    pub fn export_to(&mut self, path: &Path) -> bool {
        match self.state.export_current(path) {
            Ok(written) => written,
            Err(err) => {
                self.error = Some(format!("Could not export {}:\n{err}", path.display()));
                false
            }
        }
    }

    /// Closes a document and its tab.
    pub fn close_document(&mut self, id: DocId) {
        if let Some(path) = self.dock.find_tab(&Tab::Document(id)) {
            self.dock.remove_tab(path);
        }
        self.forget_document(id);
    }

    /// File → Close: closes the current document, asking first if it has
    /// unsaved changes.
    pub fn close_current(&mut self) {
        if let Some(video) = self.focused_video {
            self.close_video(video);
            return;
        }
        if let Some(id) = self.state.current_id() {
            self.request_close(id);
        }
    }

    pub fn is_panel_open(&self, panel: Panel) -> bool {
        self.dock.find_tab(&Tab::Panel(panel)).is_some()
    }

    pub fn set_panel_open(&mut self, panel: Panel, open: bool) {
        let tab = Tab::Panel(panel);
        match (self.dock.find_tab(&tab), open) {
            (Some(path), false) => {
                self.dock.remove_tab(path);
            }
            (None, true) => {
                let leaf = self
                    .dock
                    .iter_leaves()
                    .find(|(_, leaf)| leaf.tabs().iter().any(|t| matches!(t, Tab::Panel(_))))
                    .map(|(path, _)| path);
                match leaf {
                    Some(path) => self.append_to_leaf(path, tab),
                    None => self.add_beside_main(tab, Side::Left),
                }
            }
            _ => {}
        }
    }

    /// Open document ids in dock order.
    pub fn document_tabs(&self) -> Vec<DocId> {
        self.dock
            .iter_all_tabs()
            .filter_map(|(_, tab)| match tab {
                Tab::Document(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    /// Draws one frame of the whole UI.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        if !self.configured_context {
            // Cmd+= / Cmd+- / Cmd+0 zoom the canvas, not the whole UI.
            ui.ctx().options_mut(|o| {
                o.zoom_with_keyboard = false;
                // Quitting goes through the app so unsaved changes are asked about.
                o.quit_shortcuts.clear();
            });
            self.configured_context = true;
        }
        if self.repaint.is_none() {
            self.repaint = Some(ui.ctx().clone());
        }
        self.pump_videos(ui.ctx());
        let checker = self
            .checker
            .get_or_insert_with(|| canvas::checker_texture(ui.ctx()))
            .clone();
        let mut actions = self.read_shortcuts(ui.ctx());
        if ui.ctx().input(|i| i.viewport().close_requested()) && !self.quit_now {
            // The window close button asks about unsaved changes first.
            if !self.state.dirty_documents().is_empty() {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if self.prompt.is_none() {
                    self.request_quit();
                }
            }
        }

        // Route edited regions to the canvases' tile caches.
        let ids: Vec<DocId> = self.state.documents().iter().map(|d| d.id).collect();
        for id in ids {
            let dirty = self.state.take_dirty(id);
            if let Some(canvas) = self.canvases.get_mut(&id) {
                for rect in dirty {
                    canvas.mark_dirty(rect);
                }
            }
        }

        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| self.menu_bar(ui, &mut actions));
        });
        let transform_active = self.transform_active();
        egui::Panel::top("tool_options").show(ui, |ui| {
            tool_options_strip(
                ui,
                self.tool,
                &mut self.tool_options,
                transform_active,
                &mut actions,
            );
        });
        if transform_active {
            // The Interpolation choice applies to the open Free Transform live.
            let wanted = self.tool_options.transform_interpolation;
            if let Some(open) = self.state.current_document() {
                if open.transform().is_some_and(|t| t.interpolation != wanted) {
                    let id = open.id;
                    self.state.set_transform_interpolation(id, wanted);
                }
            }
        }
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.label(self.status_text());
        });

        let mut closed = Vec::new();
        let mut close_requests = Vec::new();
        let mut layer_actions = Vec::new();
        let mut tool_click = None;
        let mut requests = Vec::new();
        let mut closed_videos = Vec::new();
        let mut video_actions = Vec::new();
        let mut frame_set_actions = Vec::new();
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(0.0))
            .show(ui, |ui| {
                let mut viewer = Viewer {
                    state: &self.state,
                    canvases: &mut self.canvases,
                    checker: &checker,
                    show_pixel_grid: self.show_pixel_grid,
                    closed: &mut closed,
                    close_requests: &mut close_requests,
                    layer_actions: &mut layer_actions,
                    rename: &mut self.rename,
                    free_transform: self.free_transform,
                    tool: self.tool,
                    tool_options: &self.tool_options,
                    tool_click: &mut tool_click,
                    colors: &mut self.colors,
                    requests: &mut requests,
                    videos: &mut self.videos,
                    closed_videos: &mut closed_videos,
                    video_actions: &mut video_actions,
                    frame_set_view: &mut self.frame_set_view,
                    frame_set_actions: &mut frame_set_actions,
                };
                DockArea::new(&mut self.dock)
                    .show_leaf_close_all_buttons(false)
                    .show_leaf_collapse_buttons(false)
                    .show_inside(ui, &mut viewer);
            });
        for id in closed {
            self.forget_document(id);
        }
        for id in closed_videos {
            self.forget_video(id);
        }
        match self.dock.find_active_focused() {
            Some((_, Tab::Document(id))) => {
                let id = *id;
                self.state.set_current(id);
                self.focused_video = None;
            }
            Some((_, Tab::Video(id))) => self.focused_video = Some(*id),
            _ => {}
        }
        for (id, action) in video_actions {
            match action {
                VideoAction::Grab => {
                    self.grab_frame(id);
                }
                VideoAction::Extract => {
                    self.open_extract_dialog(id);
                }
            }
        }
        for action in frame_set_actions {
            self.frame_set_action(action);
        }
        if self.state.current_id() != self.last_current {
            // Switching documents abandons an open polygon, drag, or stroke.
            self.state.cancel_all_strokes();
            self.canvases
                .values_mut()
                .for_each(CanvasState::cancel_gesture);
            self.last_current = self.state.current_id();
        }
        for id in close_requests {
            self.request_close(id);
        }
        for action in layer_actions {
            self.layer_action(action);
        }
        for (id, event) in requests {
            self.handle_canvas_event(id, event);
        }
        if let Some(tool) = tool_click {
            self.set_tool(tool);
        }

        self.show_new_dialog_window(ui.ctx());
        self.show_fill_dialog_window(ui.ctx());
        self.show_radius_dialog_window(ui.ctx());
        self.show_filter_dialog_window(ui.ctx());
        self.show_extract_dialog_window(ui.ctx());
        self.show_prompt_window(ui.ctx());
        self.show_error_window(ui.ctx());

        for action in actions {
            self.run(action, ui.ctx());
        }
        if self.quit_now {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn show_prompt_window(&mut self, ctx: &egui::Context) {
        let Some(prompt) = &self.prompt else {
            return;
        };
        let Some(&id) = prompt.queue.first() else {
            return;
        };
        let name = self
            .state
            .document(id)
            .map(|o| o.document.name.clone())
            .unwrap_or_default();
        let mut answer = None;
        egui::Window::new("Unsaved Changes")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("Save changes to \"{name}\" before closing?"));
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        answer = Some(PromptAnswer::Save);
                    }
                    if ui.button("Don't Save").clicked() {
                        answer = Some(PromptAnswer::DontSave);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(PromptAnswer::Cancel);
                    }
                });
            });
        if let Some(answer) = answer {
            self.answer_prompt(answer);
        }
    }

    fn read_shortcuts(&self, ctx: &egui::Context) -> Vec<Action> {
        // A focused text field keeps its own editing shortcuts (undo, etc.).
        let typing = ctx.text_edit_focused();
        ctx.input_mut(|input| {
            // Most specific first: Cmd+Shift+X before Cmd+X, since extra
            // Shift is ignored when matching.
            let mut bindings = vec![
                (SHORTCUT_EXPORT, Action::Export),
                (SHORTCUT_SAVE_AS, Action::SaveAs),
                (SHORTCUT_SAVE, Action::Save),
                (SHORTCUT_NEW_LAYER, Action::Layer(LayerOp::New)),
                (SHORTCUT_NEW, Action::New),
                (SHORTCUT_OPEN, Action::Open),
                (SHORTCUT_CLOSE, Action::Close),
                (SHORTCUT_QUIT, Action::Quit),
            ];
            if !typing {
                bindings.extend([
                    (SHORTCUT_REDO, Action::Redo),
                    (SHORTCUT_UNDO, Action::Undo),
                    (SHORTCUT_REDO_ALT, Action::Redo),
                    (SHORTCUT_ZOOM_IN, Action::View(ViewAction::ZoomIn)),
                    (SHORTCUT_ZOOM_IN_ALT, Action::View(ViewAction::ZoomIn)),
                    (SHORTCUT_ZOOM_OUT, Action::View(ViewAction::ZoomOut)),
                    (SHORTCUT_FIT, Action::View(ViewAction::Fit)),
                    (SHORTCUT_ACTUAL_SIZE, Action::View(ViewAction::ActualSize)),
                    (SHORTCUT_INVERT, Action::InvertSelection),
                    (SHORTCUT_SELECT_ALL, Action::SelectAll),
                    (SHORTCUT_DESELECT, Action::Deselect),
                    (SHORTCUT_DUPLICATE_LAYER, Action::Layer(LayerOp::Duplicate)),
                    (SHORTCUT_MERGE_DOWN, Action::Layer(LayerOp::MergeDown)),
                    (SHORTCUT_LAYER_UP, Action::Layer(LayerOp::MoveUp)),
                    (SHORTCUT_LAYER_DOWN, Action::Layer(LayerOp::MoveDown)),
                    (SHORTCUT_FREE_TRANSFORM, Action::FreeTransform),
                ]);
            }
            let mut actions: Vec<Action> = bindings
                .into_iter()
                .filter(|(shortcut, _)| input.consume_shortcut(shortcut))
                .map(|(_, action)| action)
                .collect();
            if !typing && self.focused_video.is_some() && !input.modifiers.command {
                for (key, command) in [
                    (Key::Space, Transport::PlayPause),
                    (Key::ArrowLeft, Transport::Step(-1)),
                    (Key::ArrowRight, Transport::Step(1)),
                    (Key::Home, Transport::Start),
                    (Key::End, Transport::End),
                ] {
                    if input.consume_key(Modifiers::NONE, key) {
                        actions.push(Action::Video(command));
                    }
                }
            }
            if !typing && self.transform_active() {
                if input.consume_key(Modifiers::NONE, Key::Enter) {
                    actions.push(Action::CommitTransform);
                }
                if input.consume_key(Modifiers::NONE, Key::Escape) {
                    actions.push(Action::CancelTransform);
                }
            }
            if !typing
                && !input.modifiers.command
                && self.tool == Tool::Move
                && !self.transform_active()
            {
                let step = if input.modifiers.shift { 10 } else { 1 };
                for (key, dx, dy) in [
                    (Key::ArrowLeft, -step, 0),
                    (Key::ArrowRight, step, 0),
                    (Key::ArrowUp, 0, -step),
                    (Key::ArrowDown, 0, step),
                ] {
                    if input.consume_key(input.modifiers, key) {
                        actions.push(Action::Nudge(dx, dy));
                    }
                }
            }
            if !typing && !input.modifiers.command {
                // Single-key tool shortcuts.
                let tool_keys = [
                    (Key::M, Tool::after_m(self.tool)),
                    (Key::L, Tool::after_l(self.tool)),
                    (Key::W, Tool::MagicWand),
                    (Key::B, Tool::Brush),
                    (Key::E, Tool::Eraser),
                    (Key::I, Tool::Eyedropper),
                    (Key::G, Tool::after_g(self.tool)),
                    (Key::S, Tool::CloneStamp),
                    (Key::V, Tool::Move),
                ];
                for (key, tool) in tool_keys {
                    if input.consume_key(Modifiers::NONE, key) {
                        actions.push(Action::SetTool(tool));
                    }
                }
                let other_keys = [
                    (Key::X, Action::SwapColors),
                    (Key::D, Action::ResetColors),
                    (Key::OpenBracket, Action::BrushSize { grow: false }),
                    (Key::CloseBracket, Action::BrushSize { grow: true }),
                ];
                for (key, action) in other_keys {
                    if input.consume_key(Modifiers::NONE, key) {
                        actions.push(action);
                    }
                }
            }
            actions
        })
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let has_document = self.state.current_document().is_some();
        ui.menu_button("File", |ui| {
            let mut item = |ui: &mut egui::Ui, label: &str, shortcut, enabled, action| {
                let button =
                    egui::Button::new(label).shortcut_text(ui.ctx().format_shortcut(&shortcut));
                if ui.add_enabled(enabled, button).clicked() {
                    actions.push(action);
                }
            };
            item(ui, "New…", SHORTCUT_NEW, true, Action::New);
            item(ui, "Open…", SHORTCUT_OPEN, true, Action::Open);
            item(ui, "Save", SHORTCUT_SAVE, has_document, Action::Save);
            item(
                ui,
                "Save As…",
                SHORTCUT_SAVE_AS,
                has_document,
                Action::SaveAs,
            );
            item(ui, "Export…", SHORTCUT_EXPORT, has_document, Action::Export);
            ui.separator();
            item(ui, "Close", SHORTCUT_CLOSE, has_document, Action::Close);
            item(ui, "Quit", SHORTCUT_QUIT, true, Action::Quit);
        });
        let stack = self.state.current_document().map(|open| open.undo_stack());
        let undo_label = stack.and_then(|s| s.undo_label()).map(str::to_owned);
        let redo_label = stack.and_then(|s| s.redo_label()).map(str::to_owned);
        ui.menu_button("Edit", |ui| {
            let mut item = |ui: &mut egui::Ui, label: &str, shortcut, enabled, action| {
                let button =
                    egui::Button::new(label).shortcut_text(ui.ctx().format_shortcut(&shortcut));
                if ui.add_enabled(enabled, button).clicked() {
                    actions.push(action);
                }
            };
            let undo = undo_label.map_or("Undo".to_owned(), |l| format!("Undo {l}"));
            let redo = redo_label.map_or("Redo".to_owned(), |l| format!("Redo {l}"));
            let can_undo = stack.is_some_and(|s| s.can_undo());
            let can_redo = stack.is_some_and(|s| s.can_redo());
            item(ui, &undo, SHORTCUT_UNDO, can_undo, Action::Undo);
            item(ui, &redo, SHORTCUT_REDO, can_redo, Action::Redo);
            ui.separator();
            if ui
                .add_enabled(has_document, egui::Button::new("Fill Layer…"))
                .clicked()
            {
                actions.push(Action::FillLayer);
            }
            ui.separator();
            let free_transform = egui::Button::new("Free Transform")
                .shortcut_text(ui.ctx().format_shortcut(&SHORTCUT_FREE_TRANSFORM));
            if ui.add_enabled(has_document, free_transform).clicked() {
                actions.push(Action::FreeTransform);
            }
            if ui
                .add_enabled(has_document, egui::Button::new("Flip Horizontal"))
                .clicked()
            {
                actions.push(Action::Flip { horizontal: true });
            }
            if ui
                .add_enabled(has_document, egui::Button::new("Flip Vertical"))
                .clicked()
            {
                actions.push(Action::Flip { horizontal: false });
            }
        });
        let has_selection = self
            .state
            .current_document()
            .is_some_and(|open| open.document.selection().is_some());
        ui.menu_button("Select", |ui| {
            let mut item = |ui: &mut egui::Ui,
                            label: &str,
                            shortcut: Option<KeyboardShortcut>,
                            enabled,
                            action| {
                let mut button = egui::Button::new(label);
                if let Some(shortcut) = shortcut {
                    button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
                }
                if ui.add_enabled(enabled, button).clicked() {
                    actions.push(action);
                }
            };
            item(
                ui,
                "All",
                Some(SHORTCUT_SELECT_ALL),
                has_document,
                Action::SelectAll,
            );
            item(
                ui,
                "None",
                Some(SHORTCUT_DESELECT),
                has_selection,
                Action::Deselect,
            );
            item(
                ui,
                "Invert",
                Some(SHORTCUT_INVERT),
                has_document,
                Action::InvertSelection,
            );
            ui.separator();
            item(
                ui,
                "Grow…",
                None,
                has_selection,
                Action::Radius(RadiusOp::Grow),
            );
            item(
                ui,
                "Shrink…",
                None,
                has_selection,
                Action::Radius(RadiusOp::Shrink),
            );
            item(
                ui,
                "Feather…",
                None,
                has_selection,
                Action::Radius(RadiusOp::Feather),
            );
        });
        if let Some(open) = self.state.current_document() {
            ui.menu_button("Layer", |ui| {
                let shortcut_of = |op: LayerOp| match op {
                    LayerOp::New => Some(SHORTCUT_NEW_LAYER),
                    LayerOp::Duplicate => Some(SHORTCUT_DUPLICATE_LAYER),
                    LayerOp::MoveUp => Some(SHORTCUT_LAYER_UP),
                    LayerOp::MoveDown => Some(SHORTCUT_LAYER_DOWN),
                    LayerOp::MergeDown => Some(SHORTCUT_MERGE_DOWN),
                    _ => None,
                };
                let groups: [&[LayerOp]; 3] = [
                    &[
                        LayerOp::New,
                        LayerOp::Duplicate,
                        LayerOp::Delete,
                        LayerOp::MoveUp,
                        LayerOp::MoveDown,
                        LayerOp::MergeDown,
                        LayerOp::Flatten,
                    ],
                    &[
                        LayerOp::AddMask,
                        LayerOp::AddMaskFromSelection,
                        LayerOp::DeleteMask,
                        LayerOp::ApplyMask,
                        LayerOp::ToggleMask,
                    ],
                    &[LayerOp::EditPixels, LayerOp::EditMask],
                ];
                for (i, group) in groups.iter().enumerate() {
                    if i > 0 {
                        ui.separator();
                    }
                    for &op in *group {
                        let label = if op == LayerOp::ToggleMask
                            && open
                                .document
                                .layer(open.active_layer())
                                .is_some_and(|l| !l.mask_enabled)
                        {
                            "Enable Mask"
                        } else {
                            op.label()
                        };
                        let mut button = egui::Button::new(label);
                        if let Some(shortcut) = shortcut_of(op) {
                            button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
                        }
                        if ui.add_enabled(op.enabled(open), button).clicked() {
                            actions.push(Action::Layer(op));
                        }
                    }
                }
            });
        } else {
            ui.add_enabled(false, egui::Button::new("Layer"));
        }
        let filter_enabled = self.state.current_document().is_some()
            && !self.transform_active()
            && self.filter_dialog.is_none();
        ui.menu_button("Filter", |ui| {
            for (i, category) in filters::Category::ALL.iter().enumerate() {
                if i > 0 {
                    ui.separator();
                }
                ui.weak(category.label());
                for filter in filters::registry()
                    .iter()
                    .filter(|f| f.category() == *category)
                {
                    let label = if filter.params().is_empty() {
                        filter.name().to_owned()
                    } else {
                        format!("{}…", filter.name())
                    };
                    if ui
                        .add_enabled(filter_enabled, egui::Button::new(label))
                        .clicked()
                    {
                        actions.push(Action::Filter(filter.id()));
                    }
                }
            }
        });
        ui.menu_button("View", |ui| {
            let mut item = |ui: &mut egui::Ui, label: &str, shortcut, action| {
                let button =
                    egui::Button::new(label).shortcut_text(ui.ctx().format_shortcut(&shortcut));
                if ui.add_enabled(has_document, button).clicked() {
                    actions.push(Action::View(action));
                }
            };
            item(ui, "Zoom In", SHORTCUT_ZOOM_IN, ViewAction::ZoomIn);
            item(ui, "Zoom Out", SHORTCUT_ZOOM_OUT, ViewAction::ZoomOut);
            item(ui, "Fit to Window", SHORTCUT_FIT, ViewAction::Fit);
            item(
                ui,
                "Actual Size",
                SHORTCUT_ACTUAL_SIZE,
                ViewAction::ActualSize,
            );
            ui.checkbox(&mut self.show_pixel_grid, "Show Pixel Grid");
            ui.separator();
            for panel in Panel::ALL {
                let mut open = self.is_panel_open(panel);
                if ui.checkbox(&mut open, panel.title()).changed() {
                    actions.push(Action::SetPanelOpen(panel, open));
                }
            }
        });
    }

    /// The status bar text.
    pub fn status_text(&self) -> String {
        match self.state.current_document() {
            Some(open) => {
                let doc = &open.document;
                let mut text = format!(
                    "{} — {} × {} px — {} DPI",
                    doc.name,
                    doc.width(),
                    doc.height(),
                    format_dpi(doc.dpi())
                );
                if let Some(canvas) = self.canvases.get(&open.id) {
                    if let Some(view) = canvas.view() {
                        text.push_str(&format!(" — {}", format_zoom(view.zoom)));
                    }
                    if let Some((x, y)) = canvas.hover_pixel() {
                        text.push_str(&format!(" — {x}, {y}"));
                    }
                }
                if open.edit_target() == EditTarget::Mask {
                    text.push_str(" — Editing mask");
                }
                if let Some(message) = &self.status_message {
                    text.push_str(&format!(" — {message}"));
                }
                text
            }
            None => "No document".to_owned(),
        }
    }

    fn run(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::New => self.show_new_dialog(),
            Action::Open => {
                if let Some(path) = open_file_dialog() {
                    self.open_path(&path);
                }
            }
            Action::Export => {
                let Some(open) = self.state.current_document() else {
                    return;
                };
                if let Some(path) = export_file_dialog(&open.document.name) {
                    self.export_to(&path);
                }
            }
            Action::Close => self.close_current(),
            Action::Quit => {
                let _ = ctx;
                self.request_quit();
            }
            Action::Save => {
                if let Some(id) = self.state.current_id() {
                    self.save_document(id, false);
                }
            }
            Action::SaveAs => {
                if let Some(id) = self.state.current_id() {
                    self.save_document(id, true);
                }
            }
            Action::Layer(op) => {
                self.layer_op(op);
            }
            Action::FreeTransform => {
                self.begin_free_transform();
            }
            Action::Flip { horizontal } => {
                self.flip_current(horizontal);
            }
            Action::CommitTransform => {
                self.commit_free_transform();
            }
            Action::CancelTransform => self.cancel_free_transform(),
            Action::Nudge(dx, dy) => {
                self.nudge(dx, dy);
            }
            Action::Filter(id) => {
                self.open_filter(id);
            }
            Action::Video(command) => {
                if let Some(tab) = self.focused_video.and_then(|id| self.videos.get_mut(&id)) {
                    tab.transport(command);
                }
            }
            Action::Undo => {
                self.undo_current();
            }
            Action::Redo => {
                self.redo_current();
            }
            Action::FillLayer => self.open_fill_dialog(),
            Action::View(action) => self.view_action(action),
            Action::SelectAll => {
                self.select_all();
            }
            Action::Deselect => {
                self.deselect();
            }
            Action::InvertSelection => {
                self.invert_selection();
            }
            Action::Radius(op) => self.open_radius_dialog(op),
            Action::SetTool(tool) => self.set_tool(tool),
            Action::SwapColors => self.colors.swap(),
            Action::ResetColors => self.colors.reset(),
            Action::BrushSize { grow } => {
                if let Some(brush) = self.tool_options.brush_for_mut(self.tool) {
                    brush.size = brush.stepped_size(grow);
                }
            }
            Action::SetPanelOpen(panel, open) => self.set_panel_open(panel, open),
        }
    }

    fn show_new_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.new_dialog.take() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        egui::Window::new("New Document")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("new_document_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Width (px)");
                        ui.text_edit_singleline(&mut form.width);
                        ui.end_row();
                        ui.label("Height (px)");
                        ui.text_edit_singleline(&mut form.height);
                        ui.end_row();
                        ui.label("DPI");
                        ui.text_edit_singleline(&mut form.dpi);
                        ui.end_row();
                    });
                if let Some(error) = &form.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.horizontal(|ui| {
                    submit = ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter));
                    cancel =
                        ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape));
                });
            });
        if cancel {
            return;
        }
        if submit {
            self.submit_new_dialog(form);
        } else {
            self.new_dialog = Some(form);
        }
    }

    fn show_fill_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut color) = self.fill_dialog else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        egui::Window::new("Fill Layer")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Color");
                    ui.color_edit_button_srgba_unmultiplied(&mut color);
                    ui.weak(format!(
                        "RGBA {}, {}, {}, {}",
                        color[0], color[1], color[2], color[3]
                    ));
                });
                ui.horizontal(|ui| {
                    submit = ui.button("OK").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        self.set_fill_dialog_color(color);
        if cancel {
            self.cancel_fill_dialog();
        } else if submit {
            self.submit_fill_dialog();
        }
    }

    fn show_radius_dialog_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.radius_dialog.clone() else {
            return;
        };
        let mut submit = false;
        let mut cancel = false;
        egui::Window::new(dialog.op.title())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Radius (px)");
                    ui.text_edit_singleline(&mut dialog.radius);
                });
                if let Some(error) = &dialog.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.horizontal(|ui| {
                    submit = ui.button("OK").clicked() || ui.input(|i| i.key_pressed(Key::Enter));
                    cancel =
                        ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape));
                });
            });
        self.radius_dialog = Some(dialog);
        if cancel {
            self.cancel_radius_dialog();
        } else if submit {
            self.submit_radius_dialog();
        }
    }

    fn show_error_window(&mut self, ctx: &egui::Context) {
        let Some(message) = &self.error else {
            return;
        };
        let mut dismiss = false;
        egui::Window::new("Error")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(message.as_str());
                dismiss = ui.button("OK").clicked();
            });
        if dismiss {
            self.error = None;
        }
    }

    /// Adds a document tab next to existing documents (or into the empty
    /// document area) and focuses it.
    fn add_document_tab(&mut self, id: DocId) {
        if let Some(open) = self.state.document(id) {
            self.canvases.insert(id, CanvasState::new(&open.document));
        }
        self.add_center_tab(Tab::Document(id));
    }

    /// Adds a document or video tab next to the existing ones (or into the
    /// empty center area) and focuses it.
    fn add_center_tab(&mut self, tab: Tab) {
        let leaf = self
            .dock
            .iter_leaves()
            .find(|(_, leaf)| {
                leaf.tabs()
                    .iter()
                    .any(|t| matches!(t, Tab::Document(_) | Tab::Video(_)))
            })
            .or_else(|| {
                self.dock
                    .iter_leaves()
                    .find(|(path, leaf)| path.surface.is_main() && leaf.tabs().is_empty())
            })
            .map(|(path, _)| path);
        match leaf {
            Some(path) => self.append_to_leaf(path, tab),
            None => self.add_beside_main(tab, Side::Right),
        }
    }

    fn append_to_leaf(&mut self, path: egui_dock::NodePath, tab: Tab) {
        if let Ok(leaf) = self.dock.leaf_mut(path) {
            leaf.append_tab(tab);
            let last = leaf.tabs().len() - 1;
            let _ = leaf.set_active_tab(last);
        }
        self.dock.set_focused_node_and_surface(path);
    }

    fn add_beside_main(&mut self, tab: Tab, side: Side) {
        let tree = self.dock.main_surface_mut();
        if tree.is_empty() || tree.root_node().is_some_and(|n| n.is_empty()) {
            self.dock.push_to_first_leaf(tab);
            return;
        }
        match side {
            // `fraction` is the share of the left child: a narrow strip of
            // panels on the left, or documents taking most of the space on the right.
            Side::Left => tree.split_left(NodeIndex::root(), 0.2, vec![tab]),
            Side::Right => tree.split_right(NodeIndex::root(), 0.25, vec![tab]),
        };
        if let Some(path) = self.dock.find_tab(&tab) {
            let _ = self.dock.set_active_tab(path);
            self.dock.set_focused_node_and_surface(path.node_path());
        }
    }

    fn forget_document(&mut self, id: DocId) {
        self.state.close(id);
        self.canvases.remove(&id);
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

/// Initial layout: Tools on the left, documents in the middle, Layers above
/// Asset Library on the right.
pub fn default_layout() -> DockState<Tab> {
    let mut dock = DockState::new(Vec::new());
    let tree = dock.main_surface_mut();
    let [documents, _tools] =
        tree.split_left(NodeIndex::root(), 0.16, vec![Tab::Panel(Panel::Tools)]);
    let [_documents, layers] = tree.split_right(documents, 0.78, vec![Tab::Panel(Panel::Layers)]);
    tree.split_below(
        layers,
        0.5,
        vec![Tab::Panel(Panel::AssetLibrary), Tab::Panel(Panel::FrameSet)],
    );
    dock
}

struct Viewer<'a> {
    state: &'a AppState,
    canvases: &'a mut HashMap<DocId, CanvasState>,
    checker: &'a TextureHandle,
    show_pixel_grid: bool,
    closed: &'a mut Vec<DocId>,
    /// Document tabs whose close button was clicked while they had unsaved changes.
    close_requests: &'a mut Vec<DocId>,
    /// What the user did in the Layers panel this frame.
    layer_actions: &'a mut Vec<LayerAction>,
    rename: &'a mut Option<(usize, String)>,
    /// Document whose transform session is a Free Transform (draws handles).
    free_transform: Option<DocId>,
    tool: Tool,
    tool_options: &'a ToolOptions,
    /// Tool clicked in the Tools panel this frame.
    tool_click: &'a mut Option<Tool>,
    /// Finished selection gestures this frame.
    requests: &'a mut Vec<(DocId, CanvasEvent)>,
    colors: &'a mut Colors,
    videos: &'a mut HashMap<VideoId, VideoTab>,
    closed_videos: &'a mut Vec<VideoId>,
    video_actions: &'a mut Vec<(VideoId, VideoAction)>,
    frame_set_view: &'a mut FrameSetView,
    frame_set_actions: &'a mut Vec<FrameSetAction>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match tab {
            Tab::Panel(panel) => panel.title().into(),
            Tab::Document(id) => self
                .state
                .document(*id)
                .map(|open| {
                    if open.is_dirty() {
                        format!("{} •", open.document.name)
                    } else {
                        open.document.name.clone()
                    }
                })
                .unwrap_or_default()
                .into(),
            Tab::Video(id) => self
                .videos
                .get(id)
                .map(|v| v.info().name.clone())
                .unwrap_or_default()
                .into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match *tab {
            Tab::Panel(Panel::Tools) => tools_panel(ui, self.tool, self.tool_click, self.colors),
            Tab::Panel(Panel::Layers) => {
                let actions = layers_panel(ui, self.state.current_document(), self.rename);
                self.layer_actions.extend(actions);
            }
            Tab::Panel(Panel::AssetLibrary) => {
                ui.weak("The asset library arrives in a later version.");
            }
            Tab::Panel(Panel::FrameSet) => {
                let actions = frame_set_panel(ui, &self.state.frame_set, self.frame_set_view);
                self.frame_set_actions.extend(actions);
            }
            Tab::Document(id) => self.document_view(ui, id),
            Tab::Video(id) => {
                if let Some(video) = self.videos.get_mut(&id) {
                    let actions = video.show(ui);
                    self.video_actions
                        .extend(actions.into_iter().map(|a| (id, a)));
                }
            }
        }
    }

    fn on_close(&mut self, tab: &mut Tab) -> OnCloseResponse {
        if let Tab::Document(id) = tab {
            if self.state.document(*id).is_some_and(|o| o.is_dirty()) {
                self.close_requests.push(*id);
                return OnCloseResponse::Ignore;
            }
            self.closed.push(*id);
        }
        if let Tab::Video(id) = tab {
            self.closed_videos.push(*id);
        }
        OnCloseResponse::Close
    }

    fn scroll_bars(&self, tab: &Tab) -> [bool; 2] {
        // The canvas handles its own scrolling.
        match tab {
            Tab::Document(_) | Tab::Video(_) => [false, false],
            Tab::Panel(_) => [true, true],
        }
    }
}

impl Viewer<'_> {
    fn document_view(&mut self, ui: &mut egui::Ui, id: DocId) {
        let (Some(open), Some(canvas)) = (self.state.document(id), self.canvases.get_mut(&id))
        else {
            return;
        };
        let events = canvas.show(
            ui,
            open,
            self.checker,
            self.show_pixel_grid,
            self.tool,
            self.tool_options,
            open.transform()
                .filter(|_| self.free_transform == Some(id))
                .map(|t| TransformBox {
                    affine: t.affine(),
                    bounds: t.content_bounds(),
                }),
        );
        self.requests.extend(events.into_iter().map(|e| (id, e)));
    }
}

fn tools_panel(ui: &mut egui::Ui, active: Tool, clicked: &mut Option<Tool>, colors: &mut Colors) {
    ui.horizontal_wrapped(|ui| {
        for tool in Tool::ALL {
            let response = ui
                .selectable_label(tool == active, tool.label())
                .on_hover_text(format!("{} ({})", tool.label(), tool.shortcut()));
            if response.clicked() {
                *clicked = Some(tool);
            }
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("FG");
        ui.color_edit_button_srgba_unmultiplied(&mut colors.foreground)
            .on_hover_text("Foreground color");
        ui.label("BG");
        ui.color_edit_button_srgba_unmultiplied(&mut colors.background)
            .on_hover_text("Background color");
        if ui
            .small_button("Swap")
            .on_hover_text("Swap colors (X)")
            .clicked()
        {
            colors.swap();
        }
        if ui
            .small_button("Reset")
            .on_hover_text("Black and white (D)")
            .clicked()
        {
            colors.reset();
        }
    });
}

fn percent_slider(ui: &mut egui::Ui, label: &str, value: &mut f32, min: f32) {
    ui.label(label);
    let mut percent = (*value * 100.0).round();
    if ui
        .add(
            egui::DragValue::new(&mut percent)
                .range(min..=100.0)
                .suffix("%"),
        )
        .changed()
    {
        *value = percent / 100.0;
    }
}

fn tool_options_strip(
    ui: &mut egui::Ui,
    tool: Tool,
    options: &mut ToolOptions,
    transform_active: bool,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        if transform_active {
            ui.label("Free Transform");
            ui.separator();
            ui.label("Interpolation");
            for interp in [Interpolation::Bilinear, Interpolation::Nearest] {
                ui.selectable_value(&mut options.transform_interpolation, interp, interp.label());
            }
            ui.separator();
            if ui.button("Commit").on_hover_text("Enter").clicked() {
                actions.push(Action::CommitTransform);
            }
            if ui.button("Cancel").on_hover_text("Escape").clicked() {
                actions.push(Action::CancelTransform);
            }
            return;
        }
        ui.label(tool.label());
        ui.separator();
        if let Some(brush) = options.brush_for_mut(tool) {
            ui.label("Size");
            ui.add(
                egui::DragValue::new(&mut brush.size)
                    .range(paint::MIN_BRUSH_SIZE..=paint::MAX_BRUSH_SIZE)
                    .speed(0.5)
                    .max_decimals(0),
            );
            percent_slider(ui, "Hardness", &mut brush.hardness, 0.0);
            percent_slider(ui, "Opacity", &mut brush.opacity, 1.0);
            if tool != Tool::CloneStamp {
                ui.checkbox(&mut brush.pixel, "Pixel");
            }
        }
        match tool {
            Tool::CloneStamp => {
                ui.checkbox(&mut options.clone_aligned, "Aligned");
            }
            Tool::PaintBucket => {
                ui.label("Tolerance");
                ui.add(egui::DragValue::new(&mut options.bucket_tolerance).range(0..=255));
                ui.checkbox(&mut options.bucket_contiguous, "Contiguous");
                percent_slider(ui, "Opacity", &mut options.bucket_opacity, 1.0);
            }
            Tool::Gradient => {
                ui.label("Type");
                ui.selectable_value(&mut options.gradient_kind, GradientKind::Linear, "Linear");
                ui.selectable_value(&mut options.gradient_kind, GradientKind::Radial, "Radial");
                ui.separator();
                ui.label("Colors");
                ui.selectable_value(
                    &mut options.gradient_colors,
                    GradientColors::ForegroundToBackground,
                    "FG → BG",
                );
                ui.selectable_value(
                    &mut options.gradient_colors,
                    GradientColors::ForegroundToTransparent,
                    "FG → Transparent",
                );
                percent_slider(ui, "Opacity", &mut options.gradient_opacity, 1.0);
            }
            Tool::Eyedropper => {
                ui.label("Sample");
                ui.selectable_value(&mut options.sample_all_layers, true, "All layers");
                ui.selectable_value(&mut options.sample_all_layers, false, "Active layer");
            }
            _ => {}
        }
        if !tool.is_selection_tool() {
            return;
        }
        ui.label("Mode");
        for mode in CombineMode::ALL {
            ui.selectable_value(&mut options.mode, mode, mode.label());
        }
        if tool.uses_antialias() {
            ui.separator();
            ui.checkbox(&mut options.antialias, "Anti-alias");
        }
        if tool == Tool::MagicWand {
            ui.separator();
            ui.label("Tolerance");
            ui.add(egui::DragValue::new(&mut options.tolerance).range(0..=255));
            ui.checkbox(&mut options.contiguous, "Contiguous");
        }
    });
}

fn format_zoom(zoom: f32) -> String {
    let percent = zoom * 100.0;
    if percent >= 10.0 {
        format!("{percent:.0}%")
    } else {
        format!("{percent:.1}%")
    }
}

fn format_dpi(dpi: f64) -> String {
    if dpi.fract() == 0.0 {
        format!("{dpi:.0}")
    } else {
        format!("{dpi:.2}")
    }
}

/// The color a paint operation uses on `target` (gray on masks).
fn target_color(target: PaintTarget, color: [u8; 4]) -> [u8; 4] {
    match target {
        PaintTarget::Pixels(_) => color,
        PaintTarget::Mask(_) => mask_gray(color),
    }
}

fn save_project_dialog(document_name: &str) -> Option<PathBuf> {
    let stem = Path::new(document_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_owned());
    let mut path = rfd::FileDialog::new()
        .set_file_name(format!("{stem}.{}", project::EXTENSION))
        .add_filter("Game Asset Creator project", &[project::EXTENSION])
        .save_file()?;
    if !project::is_project_path(&path) {
        path.set_extension(project::EXTENSION);
    }
    Some(path)
}

fn open_file_dialog() -> Option<PathBuf> {
    let mut all: Vec<&str> = FileFormat::ALL
        .iter()
        .flat_map(|f| f.extensions().iter().copied())
        .collect();
    all.push(project::EXTENSION);
    all.extend(video::EXTENSIONS);
    let mut dialog = rfd::FileDialog::new()
        .add_filter("Images, videos, and projects", &all)
        .add_filter("Game Asset Creator project", &[project::EXTENSION])
        .add_filter("Videos", &video::EXTENSIONS);
    for format in FileFormat::ALL {
        dialog = dialog.add_filter(format.label(), format.extensions());
    }
    dialog.pick_file()
}

fn export_file_dialog(document_name: &str) -> Option<PathBuf> {
    let stem = Path::new(document_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "export".to_owned());
    let mut dialog = rfd::FileDialog::new().set_file_name(format!("{stem}.png"));
    for format in FileFormat::ALL {
        dialog = dialog.add_filter(format.label(), format.extensions());
    }
    let mut path = dialog.save_file()?;
    if path.extension().is_none() {
        path.set_extension("png");
    }
    Some(path)
}
