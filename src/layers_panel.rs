//! The Layers panel: rows with visibility, rename, and mask chips, the
//! active layer's blend mode and opacity, and the stack buttons. It only
//! reports what the user did; the app applies it.

use egui::RichText;

use crate::app_state::{EditTarget, OpenDocument};
use crate::document::BlendMode;
use crate::layers;

/// Layer menu and panel operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerOp {
    New,
    Duplicate,
    Delete,
    MoveUp,
    MoveDown,
    MergeDown,
    Flatten,
    AddMask,
    AddMaskFromSelection,
    DeleteMask,
    ApplyMask,
    ToggleMask,
    EditPixels,
    EditMask,
}

impl LayerOp {
    pub fn label(self) -> &'static str {
        match self {
            Self::New => "New Layer",
            Self::Duplicate => "Duplicate Layer",
            Self::Delete => "Delete Layer",
            Self::MoveUp => "Move Layer Up",
            Self::MoveDown => "Move Layer Down",
            Self::MergeDown => "Merge Down",
            Self::Flatten => "Flatten Image",
            Self::AddMask => "Add Layer Mask",
            Self::AddMaskFromSelection => "Add Mask from Selection",
            Self::DeleteMask => "Delete Mask",
            Self::ApplyMask => "Apply Mask",
            Self::ToggleMask => "Disable Mask",
            Self::EditPixels => "Edit Layer Pixels",
            Self::EditMask => "Edit Layer Mask",
        }
    }

    /// Whether the operation applies to the document right now.
    pub fn enabled(self, open: &OpenDocument) -> bool {
        let doc = &open.document;
        let layer = doc.layer(doc.active_layer());
        let has_mask = layer.is_some_and(|l| l.mask().is_some());
        match self {
            Self::New | Self::Flatten => true,
            Self::Duplicate => layer.is_some(),
            Self::Delete => layers::delete_layer(doc).is_some(),
            Self::MoveUp => layers::move_layer(doc, true).is_some(),
            Self::MoveDown => layers::move_layer(doc, false).is_some(),
            Self::MergeDown => layers::merge_down(doc).is_some(),
            Self::AddMask => !has_mask,
            Self::AddMaskFromSelection => !has_mask && doc.selection().is_some(),
            Self::DeleteMask | Self::ApplyMask | Self::ToggleMask => has_mask,
            Self::EditPixels => open.edit_target() == EditTarget::Mask,
            Self::EditMask => has_mask && open.edit_target() == EditTarget::Pixels,
        }
    }
}

/// Something the user did in the Layers panel.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerAction {
    Select(usize),
    ToggleVisible(usize),
    StartRename(usize),
    Rename(usize, String),
    CancelRename,
    EditMask(usize),
    Blend(BlendMode),
    /// Live opacity while dragging (0–1).
    OpacityPreview(f32),
    /// The opacity drag (or edit) finished.
    OpacityCommit,
    Op(LayerOp),
}

/// Draws the panel; `rename` holds the row being renamed and its text.
pub fn layers_panel(
    ui: &mut egui::Ui,
    open: Option<&OpenDocument>,
    rename: &mut Option<(usize, String)>,
) -> Vec<LayerAction> {
    let mut actions = Vec::new();
    let Some(open) = open else {
        ui.weak("No document");
        return actions;
    };
    let doc = &open.document;
    let active = doc.active_layer();
    if let Some(layer) = doc.layer(active) {
        ui.horizontal(|ui| {
            let mut mode = layer.blend_mode;
            egui::ComboBox::from_id_salt("blend_mode")
                .selected_text(mode.label())
                .show_ui(ui, |ui| {
                    for m in BlendMode::ALL {
                        ui.selectable_value(&mut mode, m, m.label());
                    }
                });
            if mode != layer.blend_mode {
                actions.push(LayerAction::Blend(mode));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Opacity");
            let mut percent = (layer.opacity() * 100.0).round();
            let response = ui.add(egui::Slider::new(&mut percent, 0.0..=100.0).suffix("%"));
            if response.changed() {
                actions.push(LayerAction::OpacityPreview(percent / 100.0));
            }
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                actions.push(LayerAction::OpacityCommit);
            }
        });
        ui.separator();
    }

    for (index, layer) in doc.layers().iter().enumerate().rev() {
        ui.horizontal(|ui| {
            let eye = if layer.visible { "👁" } else { "–" };
            if ui
                .small_button(eye)
                .on_hover_text(if layer.visible {
                    "Hide layer"
                } else {
                    "Show layer"
                })
                .clicked()
            {
                actions.push(LayerAction::ToggleVisible(index));
            }
            let is_active = index == active;
            match rename {
                Some((row, text)) if *row == index => {
                    let response = ui.text_edit_singleline(text);
                    response.request_focus();
                    let (enter, escape) = ui.input(|i| {
                        (
                            i.key_pressed(egui::Key::Enter),
                            i.key_pressed(egui::Key::Escape),
                        )
                    });
                    if enter {
                        actions.push(LayerAction::Rename(index, text.clone()));
                    } else if escape {
                        actions.push(LayerAction::CancelRename);
                    }
                }
                _ => {
                    let pixels_target = is_active && open.edit_target() == EditTarget::Pixels;
                    let name = ui.selectable_label(
                        pixels_target || (is_active && layer.mask().is_none()),
                        &layer.name,
                    );
                    if name.double_clicked() {
                        actions.push(LayerAction::StartRename(index));
                    } else if name.clicked() {
                        actions.push(LayerAction::Select(index));
                    }
                }
            }
            if layer.mask().is_some() {
                let mut text = RichText::new("Mask");
                if !layer.mask_enabled {
                    text = text.strikethrough();
                }
                let selected = is_active && open.edit_target() == EditTarget::Mask;
                if ui
                    .selectable_label(selected, text)
                    .on_hover_text("Edit the layer mask")
                    .clicked()
                {
                    actions.push(LayerAction::EditMask(index));
                }
            }
            if is_active {
                ui.weak("◀");
            }
        });
    }

    ui.separator();
    ui.horizontal_wrapped(|ui| {
        for (op, text) in [
            (LayerOp::New, "New"),
            (LayerOp::Duplicate, "Duplicate"),
            (LayerOp::Delete, "Delete"),
            (LayerOp::MoveUp, "Up"),
            (LayerOp::MoveDown, "Down"),
            (LayerOp::MergeDown, "Merge Down"),
            (LayerOp::Flatten, "Flatten"),
        ] {
            if ui
                .add_enabled(op.enabled(open), egui::Button::new(text))
                .on_hover_text(op.label())
                .clicked()
            {
                actions.push(LayerAction::Op(op));
            }
        }
    });
    actions
}
