//! The Asset Library panel: search, category filters, thumbnail grid,
//! editing, categories, and the Export dialog.

use std::collections::{BTreeSet, HashMap};

use egui::{Color32, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};

use crate::asset_library::{Asset, AssetLibrary, CategorySource, OTHER};
use crate::library::pack::{ExportFormat, Layout, SheetOptions};

/// What the user did in the panel (or its windows) this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum LibraryAction {
    Open(u64),
    Rename(u64, String),
    SetCategory(Vec<u64>, String),
    SetTags(u64, Vec<String>),
    Remove(Vec<u64>),
    AddCategory(String),
    RenameCategory(String, String),
    RemoveCategory(String),
    AutoTag(Vec<u64>),
    Reveal(u64),
    /// Export… was confirmed in the dialog.
    Export(Vec<u64>, ExportFormat),
    DownloadTagger,
}

/// Choices in the Export dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Pngs,
    Sheet,
    Strips,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportDialog {
    pub ids: Vec<u64>,
    pub kind: ExportKind,
    pub grid: bool,
    /// 0 = automatic.
    pub columns: u32,
    pub padding: u32,
    pub power_of_two: bool,
}

impl ExportDialog {
    pub fn new(ids: Vec<u64>) -> Self {
        Self {
            ids,
            kind: ExportKind::Sheet,
            grid: false,
            columns: 0,
            padding: 2,
            power_of_two: false,
        }
    }

    pub fn format(&self) -> ExportFormat {
        match self.kind {
            ExportKind::Pngs => ExportFormat::Pngs,
            ExportKind::Strips => ExportFormat::Strips {
                padding: self.padding,
            },
            ExportKind::Sheet => ExportFormat::Sheet(SheetOptions {
                layout: if self.grid {
                    Layout::Grid {
                        columns: (self.columns > 0).then_some(self.columns),
                    }
                } else {
                    Layout::MaxRects
                },
                padding: self.padding,
                power_of_two: self.power_of_two,
                max_size: 4096,
            }),
        }
    }
}

/// Panel state kept between frames.
pub struct LibraryView {
    pub search: String,
    /// Thumbnail size in points.
    pub thumb: f32,
    /// Category chips that are on (empty = All).
    pub chips: BTreeSet<String>,
    pub selected: BTreeSet<u64>,
    textures: HashMap<(u64, usize), TextureHandle>,
    pub rename: Option<(u64, String)>,
    pub tags: Option<(u64, String)>,
    pub confirm_remove: Option<Vec<u64>>,
    pub categories_open: bool,
    pub new_category: String,
    pub category_rename: Option<(String, String)>,
    pub export: Option<ExportDialog>,
}

impl Default for LibraryView {
    fn default() -> Self {
        Self {
            search: String::new(),
            thumb: 64.0,
            chips: BTreeSet::new(),
            selected: BTreeSet::new(),
            textures: HashMap::new(),
            rename: None,
            tags: None,
            confirm_remove: None,
            categories_open: false,
            new_category: String::new(),
            category_rename: None,
            export: None,
        }
    }
}

impl LibraryView {
    /// Assets shown under the current search and chips, in order added.
    pub fn visible<'a>(&self, library: &'a AssetLibrary) -> Vec<&'a Asset> {
        library
            .assets()
            .iter()
            .filter(|a| self.chips.is_empty() || self.chips.contains(&a.category))
            .filter(|a| a.matches(&self.search))
            .collect()
    }
}

/// The tagging status shown in the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagStatus {
    pub text: String,
    pub can_download: bool,
}

pub fn library_panel(
    ui: &mut egui::Ui,
    library: &AssetLibrary,
    view: &mut LibraryView,
    status: &TagStatus,
) -> Vec<LibraryAction> {
    let mut actions = Vec::new();
    view.textures
        .retain(|(id, _), _| library.get(*id).is_some());
    view.selected.retain(|id| library.get(*id).is_some());
    view.chips.retain(|c| library.categories().contains(c));

    // Rows wrap at the panel's width as measured here (a wide control must
    // not widen the layout for the rows after it).
    let width = ui.available_width();
    let row = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut egui::Ui)| {
        ui.allocate_ui(Vec2::new(width, 0.0), |ui| {
            ui.set_max_width(width);
            ui.horizontal_wrapped(|ui| add(ui));
        });
    };
    ui.spacing_mut().slider_width = 60.0;

    // Toolbar.
    row(ui, &mut |ui| {
        ui.label(format!(
            "{} asset{}",
            library.len(),
            if library.len() == 1 { "" } else { "s" }
        ));
        ui.add(
            egui::TextEdit::singleline(&mut view.search)
                .hint_text("Search")
                .desired_width(90.0),
        );
        ui.add(egui::Slider::new(&mut view.thumb, 40.0..=128.0).show_value(false))
            .on_hover_text("Thumbnail size");
        let sel: Vec<u64> = view.selected.iter().copied().collect();
        let shown: Vec<u64> = view.visible(library).iter().map(|a| a.id).collect();
        let target = if sel.is_empty() { shown } else { sel.clone() };
        if ui
            .add_enabled(!target.is_empty(), egui::Button::new("Export…"))
            .clicked()
        {
            view.export = Some(ExportDialog::new(target));
        }
        if ui
            .add_enabled(!sel.is_empty(), egui::Button::new("Auto-Tag"))
            .clicked()
        {
            actions.push(LibraryAction::AutoTag(sel.clone()));
        }
        if ui.button("Categories…").clicked() {
            view.categories_open = true;
        }
    });
    // Status and notices wrap to the panel's width (a long line must not
    // widen the panel's layout).
    ui.add(egui::Label::new(egui::RichText::new(&status.text).weak()).wrap());
    if status.can_download && ui.small_button("Download tagging model…").clicked() {
        actions.push(LibraryAction::DownloadTagger);
    }
    for notice in library.notices() {
        ui.add(
            egui::Label::new(egui::RichText::new(notice).color(ui.visuals().warn_fg_color)).wrap(),
        );
    }

    // Category chips with counts.
    row(ui, &mut |ui| {
        if ui
            .selectable_label(view.chips.is_empty(), format!("All {}", library.len()))
            .clicked()
        {
            view.chips.clear();
        }
        for c in library.categories() {
            let n = library.assets().iter().filter(|a| &a.category == c).count();
            let on = view.chips.contains(c);
            if ui.selectable_label(on, format!("{c} {n}")).clicked() {
                if on {
                    view.chips.remove(c);
                } else {
                    view.chips.insert(c.clone());
                }
            }
        }
    });

    // Selection actions.
    let sel: Vec<u64> = view.selected.iter().copied().collect();
    let single = (sel.len() == 1).then(|| sel[0]);
    row(ui, &mut |ui| {
        if ui
            .add_enabled(single.is_some(), egui::Button::new("Rename"))
            .clicked()
        {
            let id = single.expect("enabled with one");
            view.rename = Some((
                id,
                library.get(id).map(|a| a.name.clone()).unwrap_or_default(),
            ));
        }
        ui.add_enabled_ui(!sel.is_empty(), |ui| {
            ui.menu_button("Category", |ui| {
                for c in library.categories() {
                    if ui.button(c).clicked() {
                        actions.push(LibraryAction::SetCategory(sel.clone(), c.clone()));
                        ui.close();
                    }
                }
            });
        });
        if ui
            .add_enabled(single.is_some(), egui::Button::new("Tags…"))
            .clicked()
        {
            let id = single.expect("enabled with one");
            view.tags = Some((
                id,
                library
                    .get(id)
                    .map(|a| a.tags.join(", "))
                    .unwrap_or_default(),
            ));
        }
        if ui
            .add_enabled(single.is_some(), egui::Button::new("Open as Document"))
            .clicked()
        {
            actions.push(LibraryAction::Open(single.expect("enabled with one")));
        }
        if ui
            .add_enabled(single.is_some(), egui::Button::new("Reveal in Finder"))
            .clicked()
        {
            actions.push(LibraryAction::Reveal(single.expect("enabled with one")));
        }
        if ui
            .add_enabled(!sel.is_empty(), egui::Button::new("Remove…"))
            .clicked()
        {
            view.confirm_remove = Some(sel.clone());
        }
    });
    ui.separator();

    if library.is_empty() {
        ui.weak("Extracted assets appear here (Extract menu, Extract Object tool, or a video's Extract Assets).");
        return actions;
    }

    // Thumbnail grid.
    let visible = view.visible(library);
    let ids: Vec<u64> = visible.iter().map(|a| a.id).collect();
    let modifiers = ui.input(|i| i.modifiers);
    let size = view.thumb;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                for asset in &visible {
                    let frames = asset.all_frames();
                    let texture = view
                        .textures
                        .entry((asset.id, 0))
                        .or_insert_with(|| {
                            let image = frames[0];
                            let c = egui::ColorImage::from_rgba_unmultiplied(
                                [image.width() as usize, image.height() as usize],
                                image.as_raw(),
                            );
                            ui.ctx().load_texture(
                                format!("asset-{}", asset.id),
                                c,
                                TextureOptions::NEAREST,
                            )
                        })
                        .clone();
                    let selected = view.selected.contains(&asset.id);
                    let cell = ui
                        .vertical(|ui| {
                            ui.set_width(size.max(72.0));
                            let (rect, _) = ui.allocate_exact_size(
                                Vec2::new(size.max(72.0), size),
                                Sense::hover(),
                            );
                            ui.painter()
                                .rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
                            let (w, h) = (asset.image.width() as f32, asset.image.height() as f32);
                            let s = (rect.width() / w).min(rect.height() / h).min(8.0);
                            ui.painter().image(
                                texture.id(),
                                Rect::from_center_size(rect.center(), Vec2::new(w, h) * s),
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                Color32::WHITE,
                            );
                            if asset.is_animation() {
                                // Filmstrip mark.
                                let band = Rect::from_min_size(
                                    rect.left_bottom() - Vec2::new(0.0, 6.0),
                                    Vec2::new(rect.width(), 6.0),
                                );
                                ui.painter()
                                    .rect_filled(band, 0.0, Color32::from_black_alpha(160));
                                for k in 0..((rect.width() / 8.0) as usize) {
                                    let p =
                                        band.left_center() + Vec2::new(4.0 + k as f32 * 8.0, 0.0);
                                    ui.painter().rect_filled(
                                        Rect::from_center_size(p, Vec2::splat(3.0)),
                                        0.0,
                                        Color32::WHITE,
                                    );
                                }
                            }
                            ui.label(&asset.name);
                            let category = match asset.category_source {
                                CategorySource::Auto => format!(
                                    "{} (auto {:.0} %)",
                                    asset.category,
                                    asset.confidence.unwrap_or(0.0) * 100.0
                                ),
                                _ => asset.category.clone(),
                            };
                            if asset.is_animation() {
                                ui.small(format!(
                                    "{} frames, {} × {}",
                                    frames.len(),
                                    asset.image.width(),
                                    asset.image.height()
                                ));
                            } else {
                                ui.small(format!(
                                    "{} × {}",
                                    asset.image.width(),
                                    asset.image.height()
                                ));
                            }
                            ui.small(category);
                        })
                        .response;
                    let click =
                        ui.interact(cell.rect, ui.id().with(("asset", asset.id)), Sense::click());
                    if selected {
                        ui.painter().rect_stroke(
                            cell.rect.expand(2.0),
                            3.0,
                            Stroke::new(2.0, ui.visuals().selection.stroke.color),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if click.double_clicked() {
                        actions.push(LibraryAction::Open(asset.id));
                    } else if click.clicked() {
                        if modifiers.command {
                            if !view.selected.remove(&asset.id) {
                                view.selected.insert(asset.id);
                            }
                        } else if modifiers.shift && !view.selected.is_empty() {
                            let anchor = ids
                                .iter()
                                .position(|id| view.selected.contains(id))
                                .unwrap_or(0);
                            let here = ids.iter().position(|id| *id == asset.id).unwrap_or(0);
                            view.selected
                                .extend(&ids[anchor.min(here)..=anchor.max(here)]);
                        } else {
                            view.selected = [asset.id].into();
                        }
                    }
                }
            });
        });
    actions
}

/// The panel's windows (rename, tags, remove confirmation, categories,
/// export); returns their actions.
pub fn library_windows(
    ctx: &egui::Context,
    library: &AssetLibrary,
    view: &mut LibraryView,
) -> Vec<LibraryAction> {
    let mut actions = Vec::new();
    let small_window = |title: &str| {
        egui::Window::new(title.to_owned())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
    };
    if let Some((id, mut text)) = view.rename.take() {
        let mut keep = true;
        small_window("Rename Asset").show(ctx, |ui| {
            let r = ui.text_edit_singleline(&mut text);
            r.request_focus();
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
                if ui.button("Rename").clicked()
                    || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    actions.push(LibraryAction::Rename(id, text.clone()));
                    keep = false;
                }
            });
        });
        if keep {
            view.rename = Some((id, text));
        }
    }
    if let Some((id, mut text)) = view.tags.take() {
        let mut keep = true;
        small_window("Tags").show(ctx, |ui| {
            ui.label("Comma-separated tags");
            ui.text_edit_singleline(&mut text);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
                if ui.button("OK").clicked() {
                    actions.push(LibraryAction::SetTags(
                        id,
                        text.split(',').map(|t| t.to_owned()).collect(),
                    ));
                    keep = false;
                }
            });
        });
        if keep {
            view.tags = Some((id, text));
        }
    }
    if let Some(ids) = view.confirm_remove.take() {
        let mut keep = true;
        small_window("Remove Assets").show(ctx, |ui| {
            ui.label(format!(
                "Remove {} asset{} from the library? Their files are deleted.",
                ids.len(),
                if ids.len() == 1 { "" } else { "s" }
            ));
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
                if ui.button("Remove").clicked() {
                    actions.push(LibraryAction::Remove(ids.clone()));
                    keep = false;
                }
            });
        });
        if keep {
            view.confirm_remove = Some(ids);
        }
    }
    if view.categories_open {
        let mut open = true;
        egui::Window::new("Categories")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                for c in library.categories() {
                    ui.horizontal(|ui| match &mut view.category_rename {
                        Some((from, to)) if from == c => {
                            ui.text_edit_singleline(to);
                            if ui.small_button("OK").clicked() {
                                actions
                                    .push(LibraryAction::RenameCategory(from.clone(), to.clone()));
                                view.category_rename = None;
                            }
                        }
                        _ => {
                            ui.label(c);
                            if c != OTHER {
                                if ui.small_button("Rename").clicked() {
                                    view.category_rename = Some((c.clone(), c.clone()));
                                }
                                if ui.small_button("Remove").clicked() {
                                    actions.push(LibraryAction::RemoveCategory(c.clone()));
                                }
                            }
                        }
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut view.new_category);
                    if ui.button("Add").clicked() && !view.new_category.trim().is_empty() {
                        actions.push(LibraryAction::AddCategory(std::mem::take(
                            &mut view.new_category,
                        )));
                    }
                });
            });
        view.categories_open = open;
    }
    if let Some(mut dialog) = view.export.take() {
        let mut keep = true;
        small_window("Export Assets").show(ctx, |ui| {
            ui.label(format!(
                "{} asset{}",
                dialog.ids.len(),
                if dialog.ids.len() == 1 { "" } else { "s" }
            ));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut dialog.kind, ExportKind::Pngs, "PNG files");
                ui.selectable_value(&mut dialog.kind, ExportKind::Sheet, "Sprite sheet");
                ui.selectable_value(&mut dialog.kind, ExportKind::Strips, "Animation strips");
            });
            if dialog.kind == ExportKind::Sheet {
                ui.horizontal(|ui| {
                    ui.label("Layout");
                    ui.selectable_value(&mut dialog.grid, false, "Max-Rects");
                    ui.selectable_value(&mut dialog.grid, true, "Grid");
                });
                if dialog.grid {
                    ui.horizontal(|ui| {
                        ui.label("Columns (0 = auto)");
                        ui.add(egui::DragValue::new(&mut dialog.columns).range(0..=256));
                    });
                }
                ui.checkbox(&mut dialog.power_of_two, "Power-of-two size");
            }
            if dialog.kind != ExportKind::Pngs {
                ui.horizontal(|ui| {
                    ui.label("Padding");
                    ui.add(
                        egui::DragValue::new(&mut dialog.padding)
                            .range(0..=16)
                            .suffix(" px"),
                    );
                });
            }
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
                if ui.button("Export…").clicked() {
                    actions.push(LibraryAction::Export(dialog.ids.clone(), dialog.format()));
                    keep = false;
                }
            });
        });
        if keep {
            view.export = Some(dialog);
        }
    }
    actions
}
