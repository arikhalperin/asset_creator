//! Extraction UI: the Remove Background and Auto-Detect Objects dialogs, the
//! Extract Object session, canvas overlays, and the Asset Library panel.

use std::collections::{BTreeSet, HashMap};

use egui::{Color32, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use image::{GrayImage, RgbaImage};

use crate::app_state::DocId;
use crate::asset_library::AssetLibrary;
use crate::canvas::ViewState;
use crate::document::PixelRect;
use crate::extract::detect::{self, DetectParams, Proposal};
use crate::extract::keying::{self, KeyParams, RemoveBackground};
use crate::extract::segment::{Engine, Prompt};
use crate::filters::Params;

/// Tint of the Extract Object mask overlay.
const MASK_TINT: Color32 = Color32::from_rgba_premultiplied(40, 90, 200, 110);

// ---------------------------------------------------------------------------
// Remove Background

/// The open Remove Background dialog.
pub struct RemoveBackgroundDialog {
    pub doc: DocId,
    pub params: Params,
    pub preview: bool,
    /// The automatically chosen color (for Auto / Reset).
    pub auto_color: [u8; 3],
    /// Waiting for a canvas click to pick the color.
    pub picking: bool,
    /// The layer before keying (colors are picked from it).
    pub original: RgbaImage,
}

/// What the user did in a dialog this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogOutcome {
    Open { changed: bool },
    Ok,
    Cancel,
}

impl RemoveBackgroundDialog {
    pub fn new(doc: DocId, layer: &RgbaImage) -> Self {
        let key = KeyParams::auto(layer);
        Self {
            doc,
            params: RemoveBackground::params_for(&key),
            preview: true,
            auto_color: key.color,
            picking: false,
            original: layer.clone(),
        }
    }

    pub fn key(&self) -> KeyParams {
        RemoveBackground::key_params(&self.params)
    }

    pub fn set_key(&mut self, key: KeyParams) {
        self.params = RemoveBackground::params_for(&key);
    }

    /// Restores defaults with the automatic color.
    pub fn reset(&mut self) {
        self.set_key(KeyParams {
            color: self.auto_color,
            tolerance: KeyParams::DEFAULT_TOLERANCE,
            softness: KeyParams::DEFAULT_SOFTNESS,
            contiguous: true,
        });
    }

    pub fn show(&mut self, ctx: &egui::Context) -> DialogOutcome {
        let before = (self.params.clone(), self.preview);
        let mut key = self.key();
        let mut outcome = None;
        egui::Window::new("Remove Background")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-24.0, 72.0))
            .show(ctx, |ui| {
                egui::Grid::new("remove_background")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Background");
                        ui.horizontal(|ui| {
                            let mut rgb = key.color;
                            ui.color_edit_button_srgb(&mut rgb);
                            key.color = rgb;
                            if ui.button("Auto").clicked() {
                                key.color = self.auto_color;
                            }
                            let pick = if self.picking {
                                "Click the canvas…"
                            } else {
                                "Pick"
                            };
                            if ui.selectable_label(self.picking, pick).clicked() {
                                self.picking = !self.picking;
                            }
                        });
                        ui.end_row();
                        let mut tolerance = (key.tolerance * 100.0).round() as u32;
                        ui.label("Tolerance");
                        ui.add(egui::Slider::new(&mut tolerance, 0..=100).suffix(" %"));
                        key.tolerance = tolerance as f32 / 100.0;
                        ui.end_row();
                        let mut softness = (key.softness * 100.0).round() as u32;
                        ui.label("Edge Softness");
                        ui.add(egui::Slider::new(&mut softness, 0..=100).suffix(" %"));
                        key.softness = softness as f32 / 100.0;
                        ui.end_row();
                        ui.label("Contiguous");
                        ui.checkbox(&mut key.contiguous, "");
                        ui.end_row();
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.preview, "Preview");
                    if ui.button("Reset").clicked() {
                        key = KeyParams {
                            color: self.auto_color,
                            tolerance: KeyParams::DEFAULT_TOLERANCE,
                            softness: KeyParams::DEFAULT_SOFTNESS,
                            contiguous: true,
                        };
                    }
                    if ui.button("Cancel").clicked() {
                        outcome = Some(DialogOutcome::Cancel);
                    }
                    if ui.button("OK").clicked() {
                        outcome = Some(DialogOutcome::Ok);
                    }
                });
            });
        self.set_key(key);
        outcome.unwrap_or(DialogOutcome::Open {
            changed: (self.params.clone(), self.preview) != before,
        })
    }
}

// ---------------------------------------------------------------------------
// Auto-Detect Objects

/// The open Auto-Detect Objects dialog.
pub struct AutoDetectDialog {
    pub doc: DocId,
    /// The pixels assets are cut from (the layer, keyed when it had no
    /// transparency).
    pub source: RgbaImage,
    pub params: DetectParams,
    pub proposals: Vec<Proposal>,
    pub checked: Vec<bool>,
}

/// What the user did in the Auto-Detect dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectOutcome {
    Open,
    Add,
    Cancel,
}

impl AutoDetectDialog {
    /// Detects objects in `layer`, keying it first when it has no transparency.
    pub fn new(doc: DocId, layer: &RgbaImage) -> Self {
        let source = if layer.pixels().any(|p| p[3] < 255) {
            layer.clone()
        } else {
            keying::key(layer, &KeyParams::auto(layer))
        };
        let mut dialog = Self {
            doc,
            source,
            params: DetectParams::default(),
            proposals: Vec::new(),
            checked: Vec::new(),
        };
        dialog.detect();
        dialog
    }

    /// Re-runs detection with the current parameters (all checked).
    pub fn detect(&mut self) {
        self.proposals = detect::detect(&self.source, &self.params);
        self.checked = vec![true; self.proposals.len()];
    }

    pub fn set_params(&mut self, params: DetectParams) {
        if params != self.params {
            self.params = params;
            self.detect();
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> DetectOutcome {
        let mut outcome = DetectOutcome::Open;
        let mut params = self.params;
        egui::Window::new("Auto-Detect Objects")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-24.0, 72.0))
            .show(ctx, |ui| {
                egui::Grid::new("auto_detect")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Minimum Size");
                        ui.add(
                            egui::DragValue::new(&mut params.min_size)
                                .range(1..=10_000)
                                .suffix(" px²"),
                        );
                        ui.end_row();
                        ui.label("Merge Distance");
                        ui.add(
                            egui::DragValue::new(&mut params.merge_distance)
                                .range(0..=64)
                                .suffix(" px"),
                        );
                        ui.end_row();
                    });
                ui.separator();
                let checked = self.checked.iter().filter(|c| **c).count();
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "{} objects found, {checked} selected",
                        self.proposals.len()
                    ));
                    if ui.small_button("All").clicked() {
                        self.checked.iter_mut().for_each(|c| *c = true);
                    }
                    if ui.small_button("None").clicked() {
                        self.checked.iter_mut().for_each(|c| *c = false);
                    }
                });
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for (i, p) in self.proposals.iter().enumerate() {
                            ui.checkbox(
                                &mut self.checked[i],
                                format!(
                                    "{}   {} × {}   at {}, {}",
                                    i + 1,
                                    p.rect.width,
                                    p.rect.height,
                                    p.rect.x,
                                    p.rect.y
                                ),
                            );
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        outcome = DetectOutcome::Cancel;
                    }
                    if ui
                        .add_enabled(checked > 0, egui::Button::new("Add to Library"))
                        .clicked()
                    {
                        outcome = DetectOutcome::Add;
                    }
                });
            });
        self.set_params(params);
        outcome
    }

    /// The checked proposals as tight crops.
    pub fn crops(&self) -> Vec<(RgbaImage, PixelRect)> {
        self.proposals
            .iter()
            .zip(&self.checked)
            .filter(|(_, c)| **c)
            .filter_map(|(p, _)| crate::extract::tight_crop_region(&self.source, &p.mask, p.rect))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Extract Object

/// The Extract Object tool's prompt and result on one document.
pub struct ExtractSession {
    pub doc: DocId,
    pub prompt: Prompt,
    /// Id of the newest segmentation request.
    pub request: u64,
    /// The newest mask: image-sized, and which engine made it.
    pub mask: Option<(GrayImage, Engine)>,
    pub note: Option<String>,
    pub busy: bool,
    texture: Option<TextureHandle>,
}

impl ExtractSession {
    pub fn new(doc: DocId) -> Self {
        Self {
            doc,
            prompt: Prompt::default(),
            request: 0,
            mask: None,
            note: None,
            busy: false,
            texture: None,
        }
    }

    pub fn set_mask(&mut self, mask: GrayImage, engine: Engine, note: Option<String>) {
        self.mask = Some((mask, engine));
        self.note = note;
        self.busy = false;
        self.texture = None;
    }

    fn mask_texture(&mut self, ctx: &egui::Context) -> Option<&TextureHandle> {
        if self.texture.is_none() {
            let (mask, _) = self.mask.as_ref()?;
            let tint = MASK_TINT.to_array();
            let pixels: Vec<Color32> = mask
                .pixels()
                .map(|p| {
                    if p[0] >= 128 {
                        Color32::from_rgba_premultiplied(tint[0], tint[1], tint[2], tint[3])
                    } else {
                        Color32::TRANSPARENT
                    }
                })
                .collect();
            let image =
                egui::ColorImage::new([mask.width() as usize, mask.height() as usize], pixels);
            self.texture = Some(ctx.load_texture("extract-mask", image, TextureOptions::NEAREST));
        }
        self.texture.as_ref()
    }
}

/// Draws extraction overlays over a document canvas: the Extract Object mask
/// and points, and Auto-Detect proposal boxes.
pub fn draw_overlays(
    ui: &egui::Ui,
    canvas_rect: Rect,
    view: &ViewState,
    image_size: [u32; 2],
    session: Option<&mut ExtractSession>,
    detect: Option<&AutoDetectDialog>,
) {
    let ppp = ui.ctx().pixels_per_point();
    let to_screen = |p: Pos2| view.image_to_view(p, ppp) + canvas_rect.min.to_vec2();
    let painter = ui.painter_at(canvas_rect);
    if let Some(session) = session {
        let image_rect = Rect::from_min_max(
            to_screen(Pos2::ZERO),
            to_screen(Pos2::new(image_size[0] as f32, image_size[1] as f32)),
        );
        if let Some(texture) = session.mask_texture(ui.ctx()) {
            painter.image(
                texture.id(),
                image_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        if let Some((a, b)) = session.prompt.rect {
            let r = Rect::from_two_pos(
                to_screen(Pos2::new(a.0, a.1)),
                to_screen(Pos2::new(b.0, b.1)),
            );
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(1.5, Color32::WHITE),
                egui::StrokeKind::Middle,
            );
        }
        for &(x, y, include) in &session.prompt.points {
            let c = to_screen(Pos2::new(x, y));
            let color = if include {
                Color32::from_rgb(40, 200, 80)
            } else {
                Color32::from_rgb(220, 50, 50)
            };
            painter.circle_filled(c, 5.0, color);
            painter.circle_stroke(c, 5.0, Stroke::new(1.0, Color32::WHITE));
        }
    }
    if let Some(detect) = detect {
        for (i, (p, checked)) in detect.proposals.iter().zip(&detect.checked).enumerate() {
            let r = Rect::from_min_max(
                to_screen(Pos2::new(p.rect.x as f32, p.rect.y as f32)),
                to_screen(Pos2::new(p.rect.right() as f32, p.rect.bottom() as f32)),
            );
            let color = if *checked {
                Color32::from_rgb(255, 200, 0)
            } else {
                Color32::from_gray(140)
            };
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(if *checked { 2.0 } else { 1.0 }, color),
                egui::StrokeKind::Outside,
            );
            painter.text(
                r.left_top() + Vec2::new(2.0, 1.0),
                egui::Align2::LEFT_TOP,
                format!("{}", i + 1),
                egui::FontId::proportional(12.0),
                color,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Asset Library panel

/// Selection and texture cache of the Asset Library panel.
#[derive(Default)]
pub struct AssetLibraryView {
    pub selected: BTreeSet<u64>,
    textures: HashMap<u64, TextureHandle>,
}

/// What the user did in the Asset Library panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetAction {
    SavePng(u64),
    SaveAll,
    Open(u64),
    Remove(Vec<u64>),
}

const THUMB: f32 = 48.0;

pub fn asset_library_panel(
    ui: &mut egui::Ui,
    library: &AssetLibrary,
    view: &mut AssetLibraryView,
) -> Vec<AssetAction> {
    let mut actions = Vec::new();
    view.textures.retain(|id, _| library.get(*id).is_some());
    view.selected.retain(|id| library.get(*id).is_some());
    let single = (view.selected.len() == 1).then(|| *view.selected.first().expect("one selected"));
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "{} asset{}",
            library.len(),
            if library.len() == 1 { "" } else { "s" }
        ));
        if ui
            .add_enabled(single.is_some(), egui::Button::new("Save PNG…"))
            .clicked()
        {
            actions.push(AssetAction::SavePng(single.expect("enabled with one")));
        }
        if ui
            .add_enabled(!library.is_empty(), egui::Button::new("Save All…"))
            .clicked()
        {
            actions.push(AssetAction::SaveAll);
        }
        if ui
            .add_enabled(single.is_some(), egui::Button::new("Open as Document"))
            .clicked()
        {
            actions.push(AssetAction::Open(single.expect("enabled with one")));
        }
        if ui
            .add_enabled(!view.selected.is_empty(), egui::Button::new("Remove"))
            .clicked()
        {
            actions.push(AssetAction::Remove(view.selected.iter().copied().collect()));
        }
    });
    if library.is_empty() {
        ui.weak("Extracted assets appear here (Extract menu, or the Extract Object tool).");
        return actions;
    }
    ui.separator();
    let modifiers = ui.input(|i| i.modifiers);
    let ids: Vec<u64> = library.assets().iter().map(|a| a.id).collect();
    for asset in library.assets() {
        let texture = view.textures.entry(asset.id).or_insert_with(|| {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [asset.image.width() as usize, asset.image.height() as usize],
                asset.image.as_raw(),
            );
            ui.ctx()
                .load_texture(format!("asset-{}", asset.id), image, TextureOptions::LINEAR)
        });
        let selected = view.selected.contains(&asset.id);
        let response = ui
            .horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(THUMB), Sense::hover());
                let (w, h) = (asset.image.width() as f32, asset.image.height() as f32);
                let scale = (THUMB / w).min(THUMB / h).min(4.0);
                ui.painter()
                    .rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
                ui.painter().image(
                    texture.id(),
                    Rect::from_center_size(rect.center(), Vec2::new(w, h) * scale),
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
                ui.vertical(|ui| {
                    ui.label(&asset.name);
                    ui.small(format!(
                        "{} × {}",
                        asset.image.width(),
                        asset.image.height()
                    ));
                });
            })
            .response;
        let response = ui.interact(
            response.rect,
            ui.id().with(("asset", asset.id)),
            Sense::click(),
        );
        if selected {
            ui.painter().rect_stroke(
                response.rect.expand(1.0),
                2.0,
                Stroke::new(2.0, ui.visuals().selection.stroke.color),
                egui::StrokeKind::Inside,
            );
        }
        if response.double_clicked() {
            actions.push(AssetAction::Open(asset.id));
        } else if response.clicked() {
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
                view.selected.clear();
                view.selected.insert(asset.id);
            }
        }
    }
    actions
}
