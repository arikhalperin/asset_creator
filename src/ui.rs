//! The egui front end: menu bar, dockable panels, document tabs, status bar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Key, KeyboardShortcut, Modifiers, TextureHandle};
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

use crate::app_state::{AppState, DocId};
use crate::canvas::CanvasEvent;
use crate::canvas::{self, CanvasState, ViewAction};
use crate::document::DEFAULT_DPI;
use crate::io::FileFormat;
use crate::paint::{self, BucketFill, GradientColors, GradientFill, GradientKind, StrokeKind};
use crate::selection::{self, CombineMode};
use crate::tools::{self, Colors, SelectionRequest, Tool, ToolOptions};
use crate::undo::{FillLayer, SetSelection};

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

const PLACEHOLDER_TOOLS: [&str; 1] = ["Move"];
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
}

impl Panel {
    pub const ALL: [Panel; 3] = [Panel::Tools, Panel::Layers, Panel::AssetLibrary];

    pub fn title(self) -> &'static str {
        match self {
            Panel::Tools => "Tools",
            Panel::Layers => "Layers",
            Panel::AssetLibrary => "Asset Library",
        }
    }
}

/// Anything that can live in a dock tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Panel(Panel),
    Document(DocId),
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
                let o = &self.tool_options;
                let command = BucketFill::new(
                    active,
                    seed,
                    self.colors.foreground,
                    o.bucket_tolerance,
                    o.bucket_contiguous,
                    o.bucket_opacity,
                );
                let _ = self.state.execute(id, Box::new(command));
            }
            CanvasEvent::Gradient { start, end } => {
                let o = &self.tool_options;
                let command = GradientFill::new(
                    active,
                    start,
                    end,
                    o.gradient_kind,
                    o.gradient_colors,
                    self.colors.foreground,
                    self.colors.background,
                    o.gradient_opacity,
                );
                // Drags shorter than a pixel are rejected by the command.
                let _ = self.state.execute(id, Box::new(command));
            }
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
    pub fn open_path(&mut self, path: &Path) -> Option<DocId> {
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

    pub fn close_current(&mut self) {
        if let Some(id) = self.state.current_id() {
            self.close_document(id);
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
                Tab::Panel(_) => None,
            })
            .collect()
    }

    /// Draws one frame of the whole UI.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        if !self.configured_context {
            // Cmd+= / Cmd+- / Cmd+0 zoom the canvas, not the whole UI.
            ui.ctx().options_mut(|o| o.zoom_with_keyboard = false);
            self.configured_context = true;
        }
        let checker = self
            .checker
            .get_or_insert_with(|| canvas::checker_texture(ui.ctx()))
            .clone();
        let mut actions = self.read_shortcuts(ui.ctx());

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
        egui::Panel::top("tool_options").show(ui, |ui| {
            tool_options_strip(ui, self.tool, &mut self.tool_options);
        });
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.label(self.status_text());
        });

        let mut closed = Vec::new();
        let mut layer_click = None;
        let mut tool_click = None;
        let mut requests = Vec::new();
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(0.0))
            .show(ui, |ui| {
                let mut viewer = Viewer {
                    state: &self.state,
                    canvases: &mut self.canvases,
                    checker: &checker,
                    show_pixel_grid: self.show_pixel_grid,
                    closed: &mut closed,
                    layer_click: &mut layer_click,
                    tool: self.tool,
                    tool_options: &self.tool_options,
                    tool_click: &mut tool_click,
                    colors: &mut self.colors,
                    requests: &mut requests,
                };
                DockArea::new(&mut self.dock)
                    .show_leaf_close_all_buttons(false)
                    .show_leaf_collapse_buttons(false)
                    .show_inside(ui, &mut viewer);
            });
        for id in closed {
            self.forget_document(id);
        }
        if let Some((_, Tab::Document(id))) = self.dock.find_active_focused() {
            let id = *id;
            self.state.set_current(id);
        }
        if self.state.current_id() != self.last_current {
            // Switching documents abandons an open polygon, drag, or stroke.
            self.state.cancel_all_strokes();
            self.canvases
                .values_mut()
                .for_each(CanvasState::cancel_gesture);
            self.last_current = self.state.current_id();
        }
        if let (Some(index), Some(id)) = (layer_click, self.state.current_id()) {
            self.state.set_active_layer(id, index);
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
        self.show_error_window(ui.ctx());

        for action in actions {
            self.run(action, ui.ctx());
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
                ]);
            }
            let mut actions: Vec<Action> = bindings
                .into_iter()
                .filter(|(shortcut, _)| input.consume_shortcut(shortcut))
                .map(|(_, action)| action)
                .collect();
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
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
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
        let tab = Tab::Document(id);
        let leaf = self
            .dock
            .iter_leaves()
            .find(|(_, leaf)| leaf.tabs().iter().any(|t| matches!(t, Tab::Document(_))))
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
    tree.split_below(layers, 0.5, vec![Tab::Panel(Panel::AssetLibrary)]);
    dock
}

struct Viewer<'a> {
    state: &'a AppState,
    canvases: &'a mut HashMap<DocId, CanvasState>,
    checker: &'a TextureHandle,
    show_pixel_grid: bool,
    closed: &'a mut Vec<DocId>,
    /// Layer index clicked in the Layers panel this frame.
    layer_click: &'a mut Option<usize>,
    tool: Tool,
    tool_options: &'a ToolOptions,
    /// Tool clicked in the Tools panel this frame.
    tool_click: &'a mut Option<Tool>,
    /// Finished selection gestures this frame.
    requests: &'a mut Vec<(DocId, CanvasEvent)>,
    colors: &'a mut Colors,
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
                .map(|open| open.document.name.clone())
                .unwrap_or_default()
                .into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match *tab {
            Tab::Panel(Panel::Tools) => tools_panel(ui, self.tool, self.tool_click, self.colors),
            Tab::Panel(Panel::Layers) => layers_panel(ui, self.state, self.layer_click),
            Tab::Panel(Panel::AssetLibrary) => {
                ui.weak("The asset library arrives in a later version.");
            }
            Tab::Document(id) => self.document_view(ui, id),
        }
    }

    fn on_close(&mut self, tab: &mut Tab) -> OnCloseResponse {
        if let Tab::Document(id) = tab {
            self.closed.push(*id);
        }
        OnCloseResponse::Close
    }

    fn scroll_bars(&self, tab: &Tab) -> [bool; 2] {
        // The canvas handles its own scrolling.
        match tab {
            Tab::Document(_) => [false, false],
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
        for name in PLACEHOLDER_TOOLS {
            ui.add_enabled(false, egui::Button::new(name))
                .on_disabled_hover_text("Coming soon");
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
            .small_button("⇄")
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

fn tool_options_strip(ui: &mut egui::Ui, tool: Tool, options: &mut ToolOptions) {
    ui.horizontal(|ui| {
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

fn layers_panel(ui: &mut egui::Ui, state: &AppState, clicked: &mut Option<usize>) {
    if state.current_document().is_none() {
        ui.weak("No document");
        return;
    }
    for row in state.current_layer_rows() {
        ui.horizontal(|ui| {
            ui.label(if row.visible { "👁" } else { "–" })
                .on_hover_text(if row.visible { "Visible" } else { "Hidden" });
            if ui.selectable_label(row.active, row.name).clicked() {
                *clicked = Some(row.index);
            }
        });
    }
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

fn open_file_dialog() -> Option<PathBuf> {
    let all: Vec<&str> = FileFormat::ALL
        .iter()
        .flat_map(|f| f.extensions().iter().copied())
        .collect();
    let mut dialog = rfd::FileDialog::new().add_filter("Images", &all);
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
