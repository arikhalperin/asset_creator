//! Filter menu and preview dialog through the app.

use egui::{Key, Modifiers, Pos2};
use game_asset_creator::app_state::EditTarget;
use game_asset_creator::canvas::ViewAction;
use game_asset_creator::filters::ParamValue;
use game_asset_creator::layers_panel::LayerOp;
use game_asset_creator::tools::Tool;
use game_asset_creator::DocId;

mod common;
use common::Harness;

/// A 120×80 document: left half red, right half blue, at 100%.
fn setup() -> (Harness, DocId) {
    let mut h = Harness::new();
    let id = h.new_doc(120, 80);
    h.app.open_fill_dialog();
    h.app.set_fill_dialog_color([0, 0, 255, 255]);
    assert!(h.app.submit_fill_dialog());
    h.app.set_tool(Tool::RectSelect);
    h.app.view_action(ViewAction::ActualSize);
    h.frames(2);
    let a = h.screen_of(id, Pos2::new(0.0, 0.0));
    let b = h.screen_of(id, Pos2::new(60.0, 80.0));
    h.drag(a, b, 6);
    h.app.open_fill_dialog();
    h.app.set_fill_dialog_color([255, 0, 0, 255]);
    assert!(h.app.submit_fill_dialog());
    assert!(h.app.deselect());
    (h, id)
}

fn px(h: &Harness, id: DocId, x: u32, y: u32) -> [u8; 4] {
    h.app
        .state
        .document(id)
        .unwrap()
        .document
        .layer(0)
        .unwrap()
        .pixels
        .get_pixel(x, y)
        .0
}

fn undo_len(h: &Harness, id: DocId) -> usize {
    h.app.state.document(id).unwrap().undo_stack().undo_len()
}

fn undo_label(h: &Harness, id: DocId) -> Option<String> {
    h.app
        .state
        .document(id)
        .unwrap()
        .undo_stack()
        .undo_label()
        .map(str::to_owned)
}

fn has_text(output: &egui::FullOutput, needle: &str) -> bool {
    output.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(t) => t.galley.text().contains(needle),
        _ => false,
    })
}

#[test]
fn dialog_previews_live_ok_records_one_step() {
    let (mut h, id) = setup();
    let original = h.app.state.document(id).unwrap().document.clone();
    let steps = undo_len(&h, id);
    assert!(h.app.open_filter("gaussian_blur"));
    h.frame(vec![]); // egui sizes new windows invisibly on their first frame
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Gaussian Blur") && has_text(&out, "Preview"));
    // The canvas shows the blur at the red/blue edge right away.
    let edge = px(&h, id, 60, 40);
    assert!(edge[0] > 0 && edge[2] > 0, "{edge:?}");
    assert_eq!(undo_len(&h, id), steps, "previews are not recorded");

    h.app.set_filter_param("radius", ParamValue::Float(8.0));
    let original_pixel = original.layer(0).unwrap().pixels.get_pixel(54, 40).0;
    assert!(
        px(&h, id, 54, 40)[2] > original_pixel[2],
        "a larger radius reaches further"
    );
    h.app.set_filter_preview(false);
    assert_eq!(
        h.app.state.document(id).unwrap().document,
        original,
        "Preview off shows the original"
    );
    h.app.set_filter_preview(true);
    h.app.reset_filter();
    assert_eq!(h.app.filter_dialog().unwrap().params.float("radius"), 2.0);

    assert!(h.app.filter_ok());
    assert!(h.app.filter_dialog().is_none());
    assert_eq!(undo_len(&h, id), steps + 1);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Gaussian Blur"));
    assert!(h.app.undo_current());
    assert_eq!(h.app.state.document(id).unwrap().document, original);
}

#[test]
fn cancel_restores_exactly() {
    let (mut h, id) = setup();
    let original = h.app.state.document(id).unwrap().document.clone();
    assert!(h.app.open_filter("hue_saturation"));
    h.app.set_filter_param("hue", ParamValue::Int(90));
    assert_ne!(h.app.state.document(id).unwrap().document, original);
    h.app.filter_cancel();
    assert_eq!(h.app.state.document(id).unwrap().document, original);
    assert!(h.app.filter_dialog().is_none());
    assert!(!h.app.state.document(id).unwrap().is_filtering());
}

#[test]
fn parameterless_filters_apply_immediately() {
    let (mut h, id) = setup();
    let steps = undo_len(&h, id);
    assert!(h.app.open_filter("invert"));
    assert!(h.app.filter_dialog().is_none());
    assert_eq!(px(&h, id, 10, 10), [0, 255, 255, 255]);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Invert"));
    assert!(h.app.open_filter("desaturate"));
    assert!(h.app.open_filter("sharpen"));
    assert_eq!(undo_len(&h, id), steps + 3);
}

#[test]
fn grouped_parameters_per_channel_levels() {
    let (mut h, id) = setup();
    assert!(h.app.open_filter("levels"));
    h.app.set_filter_param("channel", ParamValue::Choice(1));
    h.frame(vec![]); // the Red group renders
    h.app
        .set_filter_param("red.out_white", ParamValue::Int(100));
    assert_eq!(px(&h, id, 10, 10), [100, 0, 0, 255], "only red changes");
    assert_eq!(px(&h, id, 100, 10), [0, 0, 255, 255]);
    assert!(h.app.filter_ok());
    assert_eq!(undo_label(&h, id).as_deref(), Some("Levels"));
}

#[test]
fn filters_respect_selection_and_mask_target() {
    let (mut h, id) = setup();
    // Select the right quarter only.
    let a = h.screen_of(id, Pos2::new(90.0, 0.0));
    let b = h.screen_of(id, Pos2::new(120.0, 80.0));
    h.drag(a, b, 6);
    assert!(h.app.open_filter("invert"));
    assert_eq!(
        px(&h, id, 100, 10),
        [255, 255, 0, 255],
        "inside the selection"
    );
    assert_eq!(px(&h, id, 70, 10), [0, 0, 255, 255], "outside unchanged");
    assert!(h.app.deselect());

    // On a mask: the mask changes, the pixels do not.
    assert!(h.app.layer_op(LayerOp::AddMask));
    assert_eq!(
        h.app.state.document(id).unwrap().edit_target(),
        EditTarget::Mask
    );
    let pixels = h
        .app
        .state
        .document(id)
        .unwrap()
        .document
        .layer(0)
        .unwrap()
        .pixels
        .clone();
    assert!(h.app.open_filter("invert"));
    let layer = h
        .app
        .state
        .document(id)
        .unwrap()
        .document
        .layer(0)
        .unwrap()
        .clone();
    assert_eq!(layer.pixels, pixels);
    assert!(
        layer.mask().unwrap().iter().all(|v| *v == 0),
        "white mask inverted to black"
    );
}

#[test]
fn menu_disabled_without_document_or_during_transform_and_undo_closes_dialog() {
    let mut h = Harness::new();
    assert!(!h.app.open_filter("gaussian_blur"), "no document");
    let (mut h, id) = setup();
    h.shortcut(Modifiers::COMMAND, Key::T);
    assert!(!h.app.open_filter("gaussian_blur"), "transform in progress");
    h.tap_key(Key::Escape);
    assert!(h.app.open_filter("posterize"));
    assert!(!h.app.open_filter("invert"), "one dialog at a time");
    let before = undo_len(&h, id);
    // Undo while the dialog is open cancels the preview and closes the dialog.
    h.app.undo_current();
    h.frame(vec![]);
    assert!(h.app.filter_dialog().is_none());
    assert_eq!(undo_len(&h, id), before - 1);
    // The Filter menu renders with every filter.
    let out = h.frame(vec![]);
    assert!(has_text(&out, "Filter"));
}

#[test]
fn curve_editor_drag_brightens_midtones() {
    let mut h = Harness::new();
    let id = h.new_doc(40, 40);
    h.app.open_fill_dialog();
    h.app.set_fill_dialog_color([128, 128, 128, 255]);
    assert!(h.app.submit_fill_dialog());
    assert!(h.app.open_filter("curves"));
    h.frame(vec![]);
    let out = h.frame(vec![]);
    // Find the 220×220 curve graph among the painted shapes.
    let graph = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Rect(r)
                if (r.rect.width() - 220.0).abs() < 0.5
                    && (r.rect.height() - 220.0).abs() < 0.5 =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .expect("curve graph drawn");
    // Drag from the middle of the diagonal (128,128) up to (128,200).
    let from = Pos2::new(
        graph.left() + 128.0 / 255.0 * 220.0,
        graph.bottom() - 128.0 / 255.0 * 220.0,
    );
    let to = Pos2::new(from.x, graph.bottom() - 200.0 / 255.0 * 220.0);
    h.drag(from, to, 6);
    let v = px(&h, id, 20, 20);
    assert!(v[0] > 180 && v[0] == v[1] && v[1] == v[2], "{v:?}");
    assert!(h.app.filter_ok());
    assert_eq!(undo_label(&h, id).as_deref(), Some("Curves"));
}

#[test]
fn save_and_export_wait_for_the_filter_preview() {
    let (mut h, id) = setup();
    let dir = std::env::temp_dir().join(format!("gac_filter_save_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(h.app.open_filter("hue_saturation"));
    h.app.set_filter_param("hue", ParamValue::Int(90));
    assert!(h.app.state.save(id, &dir.join("a.gac")).is_err());
    assert!(
        !dir.join("a.gac").exists(),
        "no project with the unconfirmed preview"
    );
    assert!(!h.app.export_to(&dir.join("a.png")));
    assert!(
        !dir.join("a.png").exists(),
        "no export with the unconfirmed preview"
    );
    assert!(h.app.filter_dialog().is_some(), "the dialog stays open");
    assert!(h.app.filter_ok());
    assert!(h.app.state.save(id, &dir.join("a.gac")).is_ok());
    assert!(h.app.export_to(&dir.join("a.png")));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn filtering_a_mask_writes_luminance() {
    let mut h = Harness::new();
    let id = h.new_doc(20, 20);
    assert!(h.app.layer_op(LayerOp::AddMask));
    // Color Balance turns the white mask colored; the mask keeps its luminance.
    assert!(h.app.open_filter("color_balance"));
    h.app
        .set_filter_param("highlights.cyan_red", ParamValue::Int(-100));
    assert!(h.app.filter_ok());
    let white = image::RgbaImage::from_pixel(20, 20, image::Rgba([255, 255, 255, 255]));
    let filter = game_asset_creator::filters::find("color_balance").unwrap();
    let mut params = game_asset_creator::filters::Params::defaults(&filter.params());
    params.set("highlights.cyan_red", ParamValue::Int(-100));
    let colored = filter.apply(&white, &params).get_pixel(5, 5).0;
    assert_ne!(
        colored[0], colored[1],
        "the filter result is colored: {colored:?}"
    );
    let expected = game_asset_creator::undo::luminance_u8(image::Rgba(colored));
    let mask = h
        .app
        .state
        .document(id)
        .unwrap()
        .document
        .layer(0)
        .unwrap()
        .mask()
        .unwrap()
        .clone();
    assert_eq!(mask.get_pixel(5, 5)[0], expected);
    assert_ne!(expected, colored[0], "not just the red channel");
}
