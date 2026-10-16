//! The egui front end: menu bar, dockable panels, document tabs, status bar.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Key, KeyboardShortcut, Modifiers, TextureHandle, TextureOptions};
use egui_dock::tab_viewer::OnCloseResponse;
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

use crate::app_state::{AppState, DocId};
use crate::document::DEFAULT_DPI;
use crate::io::FileFormat;

pub const APP_TITLE: &str = "Game Asset Creator";

/// Largest width/height accepted by File → New.
pub const MAX_NEW_DOCUMENT_SIDE: u32 = 16384;

const SHORTCUT_NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const SHORTCUT_OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SHORTCUT_EXPORT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::E);
const SHORTCUT_CLOSE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
const SHORTCUT_QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);

const PLACEHOLDER_TOOLS: [&str; 6] = ["Move", "Select", "Brush", "Eraser", "Fill", "Picker"];

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
    SetPanelOpen(Panel, bool),
}

/// The application: state, dock layout, cached textures, and dialogs.
pub struct EditorApp {
    pub state: AppState,
    dock: DockState<Tab>,
    textures: HashMap<DocId, (u64, TextureHandle)>,
    new_dialog: Option<NewDocumentForm>,
    error: Option<String>,
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
            textures: HashMap::new(),
            new_dialog: None,
            error: None,
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
        let mut actions = self.read_shortcuts(ui.ctx());

        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| self.menu_bar(ui, &mut actions));
        });
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.label(self.status_text());
        });

        let mut closed = Vec::new();
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(0.0))
            .show(ui, |ui| {
                let mut viewer = Viewer {
                    state: &self.state,
                    textures: &mut self.textures,
                    closed: &mut closed,
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

        self.show_new_dialog_window(ui.ctx());
        self.show_error_window(ui.ctx());

        for action in actions {
            self.run(action, ui.ctx());
        }
    }

    fn read_shortcuts(&self, ctx: &egui::Context) -> Vec<Action> {
        ctx.input_mut(|input| {
            // Most specific first: Cmd+Shift+E before any Cmd+E binding.
            [
                (SHORTCUT_EXPORT, Action::Export),
                (SHORTCUT_NEW, Action::New),
                (SHORTCUT_OPEN, Action::Open),
                (SHORTCUT_CLOSE, Action::Close),
                (SHORTCUT_QUIT, Action::Quit),
            ]
            .into_iter()
            .filter(|(shortcut, _)| input.consume_shortcut(shortcut))
            .map(|(_, action)| action)
            .collect()
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
        ui.menu_button("View", |ui| {
            for panel in Panel::ALL {
                let mut open = self.is_panel_open(panel);
                if ui.checkbox(&mut open, panel.title()).changed() {
                    actions.push(Action::SetPanelOpen(panel, open));
                }
            }
        });
    }

    fn status_text(&self) -> String {
        match self.state.current_document() {
            Some(open) => {
                let doc = &open.document;
                format!(
                    "{} — {} × {} px — {} DPI",
                    doc.name,
                    doc.width(),
                    doc.height(),
                    format_dpi(doc.dpi())
                )
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
        self.textures.remove(&id);
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
    textures: &'a mut HashMap<DocId, (u64, TextureHandle)>,
    closed: &'a mut Vec<DocId>,
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
            Tab::Panel(Panel::Tools) => tools_panel(ui),
            Tab::Panel(Panel::Layers) => layers_panel(ui, self.state),
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
        // Document views manage their own scroll area.
        match tab {
            Tab::Document(_) => [false, false],
            Tab::Panel(_) => [true, true],
        }
    }
}

impl Viewer<'_> {
    fn document_view(&mut self, ui: &mut egui::Ui, id: DocId) {
        let Some(open) = self.state.document(id) else {
            return;
        };
        let stale = self
            .textures
            .get(&id)
            .is_none_or(|(revision, _)| *revision != open.revision);
        if stale {
            let composite = open.document.composite();
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [composite.width() as usize, composite.height() as usize],
                composite.as_raw(),
            );
            let texture =
                ui.ctx()
                    .load_texture(format!("document-{}", id.0), image, TextureOptions::NEAREST);
            self.textures.insert(id, (open.revision, texture));
        }
        let texture = &self.textures[&id].1;
        // 100%: one image pixel per physical screen pixel.
        let size = texture.size_vec2() / ui.ctx().pixels_per_point();
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            ui.image((texture.id(), size));
        });
    }
}

fn tools_panel(ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        for name in PLACEHOLDER_TOOLS {
            ui.add_enabled(false, egui::Button::new(name))
                .on_disabled_hover_text("Coming soon");
        }
    });
}

fn layers_panel(ui: &mut egui::Ui, state: &AppState) {
    if state.current_document().is_none() {
        ui.weak("No document");
        return;
    }
    for row in state.current_layer_rows() {
        ui.horizontal(|ui| {
            ui.label(if row.visible { "👁" } else { "–" })
                .on_hover_text(if row.visible { "Visible" } else { "Hidden" });
            ui.label(row.name);
        });
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
