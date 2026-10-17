//! Layer system through the app: Layer menu shortcuts, Layers panel actions,
//! masks, Move tool, Free Transform, flips, Save/Open, and unsaved-changes prompts.

use egui::{Key, Modifiers, PointerButton, Pos2};
use game_asset_creator::app_state::EditTarget;
use game_asset_creator::canvas::ViewAction;
use game_asset_creator::document::BlendMode;
use game_asset_creator::layers_panel::{LayerAction, LayerOp};
use game_asset_creator::tools::Tool;
use game_asset_creator::ui::PromptAnswer;
use game_asset_creator::DocId;

mod common;
use common::Harness;

/// A 200×200 document filled opaque white, at 100%.
fn setup() -> (Harness, DocId) {
    let mut h = Harness::new();
    let id = h.new_doc(200, 200);
    h.app.open_fill_dialog();
    h.app.set_fill_dialog_color([255, 255, 255, 255]);
    assert!(h.app.submit_fill_dialog());
    h.app.view_action(ViewAction::ActualSize);
    h.frames(2);
    (h, id)
}

fn names(h: &Harness, id: DocId) -> Vec<String> {
    h.app
        .state
        .document(id)
        .unwrap()
        .document
        .layers()
        .iter()
        .map(|l| l.name.clone())
        .collect()
}

fn active(h: &Harness, id: DocId) -> usize {
    h.app.state.document(id).unwrap().active_layer()
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

fn undo_len(h: &Harness, id: DocId) -> usize {
    h.app.state.document(id).unwrap().undo_stack().undo_len()
}

fn px(h: &Harness, id: DocId, layer: usize, x: u32, y: u32) -> [u8; 4] {
    h.app
        .state
        .document(id)
        .unwrap()
        .document
        .layer(layer)
        .unwrap()
        .pixels
        .get_pixel(x, y)
        .0
}

fn at(h: &Harness, id: DocId, x: f32, y: f32) -> Pos2 {
    h.screen_of(id, Pos2::new(x, y))
}

#[test]
fn layer_shortcuts_and_menu_operations() {
    let (mut h, id) = setup();
    h.shortcut(Modifiers::COMMAND | Modifiers::SHIFT, Key::N);
    assert_eq!(names(&h, id), ["Background", "Layer 1"]);
    assert_eq!(active(&h, id), 1);
    assert_eq!(undo_label(&h, id).as_deref(), Some("New Layer"));
    h.shortcut(Modifiers::COMMAND, Key::J);
    assert_eq!(names(&h, id), ["Background", "Layer 1", "Layer 1 copy"]);
    h.shortcut(Modifiers::COMMAND, Key::OpenBracket);
    assert_eq!(names(&h, id), ["Background", "Layer 1 copy", "Layer 1"]);
    assert_eq!(active(&h, id), 1);
    h.shortcut(Modifiers::COMMAND, Key::CloseBracket);
    assert_eq!(active(&h, id), 2);
    h.shortcut(Modifiers::COMMAND, Key::E);
    assert_eq!(names(&h, id), ["Background", "Layer 1"]);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Merge Down"));
    assert!(h.app.layer_op(LayerOp::Flatten));
    assert_eq!(names(&h, id), ["Background"]);
    assert!(
        !h.app.layer_op(LayerOp::Delete),
        "the only layer cannot be deleted"
    );
    assert!(!h.app.layer_op(LayerOp::MergeDown));
    // Undo walks everything back.
    for _ in 0..5 {
        assert!(h.app.undo_current());
    }
    assert_eq!(names(&h, id), ["Background", "Layer 1"]);
}

#[test]
fn panel_actions_visibility_rename_blend_opacity() {
    let (mut h, id) = setup();
    h.app.layer_op(LayerOp::New);
    let steps = undo_len(&h, id);
    h.app.layer_action(LayerAction::ToggleVisible(1));
    assert!(
        !h.app
            .state
            .document(id)
            .unwrap()
            .document
            .layer(1)
            .unwrap()
            .visible
    );
    assert_eq!(undo_label(&h, id).as_deref(), Some("Hide Layer"));
    h.app.layer_action(LayerAction::StartRename(1));
    h.app
        .layer_action(LayerAction::Rename(1, "  Shading ".into()));
    assert_eq!(names(&h, id)[1], "Shading");
    h.app.layer_action(LayerAction::StartRename(1));
    h.app.layer_action(LayerAction::Rename(1, "   ".into()));
    assert_eq!(names(&h, id)[1], "Shading", "empty names rejected");
    h.app.layer_action(LayerAction::Blend(BlendMode::Overlay));
    assert_eq!(
        h.app
            .state
            .document(id)
            .unwrap()
            .document
            .layer(1)
            .unwrap()
            .blend_mode,
        BlendMode::Overlay
    );
    assert_eq!(undo_len(&h, id), steps + 3);

    // An opacity drag previews live and records one step.
    for v in [0.8, 0.5, 0.2] {
        h.app.layer_action(LayerAction::OpacityPreview(v));
        h.frame(vec![]);
    }
    assert_eq!(undo_len(&h, id), steps + 3);
    h.app.layer_action(LayerAction::OpacityCommit);
    assert_eq!(undo_len(&h, id), steps + 4);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Layer Opacity"));
    assert_eq!(
        h.app
            .state
            .document(id)
            .unwrap()
            .document
            .layer(1)
            .unwrap()
            .opacity(),
        0.2
    );
    h.app.undo_current();
    assert_eq!(
        h.app
            .state
            .document(id)
            .unwrap()
            .document
            .layer(1)
            .unwrap()
            .opacity(),
        1.0
    );

    h.app.layer_action(LayerAction::Select(0));
    assert_eq!(active(&h, id), 0);
    // The panel renders with all of this.
    h.frame(vec![]);
}

#[test]
fn mask_editing_with_paint_tools() {
    let (mut h, id) = setup();
    assert!(h.app.layer_op(LayerOp::AddMask));
    let open = h.app.state.document(id).unwrap();
    assert_eq!(
        open.edit_target(),
        EditTarget::Mask,
        "adding a mask targets it"
    );
    assert!(h.app.status_text().contains("Editing mask"));
    let pixels = open.document.layer(0).unwrap().pixels.clone();

    // Brush black on the mask hides; pixels stay untouched.
    h.app.set_tool(Tool::Brush);
    h.app.colors_mut().foreground = [200, 30, 30, 255];
    h.pointer = at(&h, id, 50.0, 50.0);
    h.frame(vec![]);
    let down = h.press(PointerButton::Primary, true);
    h.frame(vec![down]);
    h.pointer = at(&h, id, 51.0, 50.0);
    h.frame(vec![]);
    let up = h.press(PointerButton::Primary, false);
    h.frame(vec![up]);
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
    let gray = layer.mask().unwrap().get_pixel(50, 50)[0];
    assert_eq!(
        gray,
        (0.299f32 * 200.0 + 0.587 * 30.0 + 0.114 * 30.0).round() as u8
    );
    assert_eq!(layer.mask().unwrap().get_pixel(150, 150)[0], 255);
    assert!(
        h.app
            .state
            .document(id)
            .unwrap()
            .document
            .composite()
            .get_pixel(50, 50)[3]
            < 255
    );

    // The eraser reveals on a mask.
    h.app.set_tool(Tool::Eraser);
    h.pointer = at(&h, id, 50.0, 50.0);
    h.frame(vec![]);
    let down = h.press(PointerButton::Primary, true);
    h.frame(vec![down]);
    let up = h.press(PointerButton::Primary, false);
    h.frame(vec![up]);
    assert_eq!(
        h.app
            .state
            .document(id)
            .unwrap()
            .document
            .layer(0)
            .unwrap()
            .mask()
            .unwrap()
            .get_pixel(50, 50)[0],
        255
    );

    // Back to pixels; toggling and deleting the mask.
    assert!(h.app.layer_op(LayerOp::EditPixels));
    assert_eq!(
        h.app.state.document(id).unwrap().edit_target(),
        EditTarget::Pixels
    );
    assert!(h.app.layer_op(LayerOp::ToggleMask));
    assert!(
        !h.app
            .state
            .document(id)
            .unwrap()
            .document
            .layer(0)
            .unwrap()
            .mask_enabled
    );
    assert!(h.app.layer_op(LayerOp::DeleteMask));
    assert!(h
        .app
        .state
        .document(id)
        .unwrap()
        .document
        .layer(0)
        .unwrap()
        .mask()
        .is_none());
}

#[test]
fn move_tool_drag_and_nudges() {
    let (mut h, id) = setup();
    h.app.layer_op(LayerOp::New);
    // A red dot on the new layer at (40, 40).
    h.app.set_tool(Tool::Brush);
    h.app.colors_mut().foreground = [255, 0, 0, 255];
    h.app.tool_options_mut().brush.size = 1.0;
    h.app.tool_options_mut().brush.pixel = true;
    h.click_at(at(&h, id, 40.5, 40.5));
    assert_eq!(px(&h, id, 1, 40, 40), [255, 0, 0, 255]);

    h.tap_key(Key::V);
    assert_eq!(h.app.tool(), Tool::Move);
    let steps = undo_len(&h, id);
    h.drag(at(&h, id, 100.0, 100.0), at(&h, id, 130.0, 90.0), 6);
    assert_eq!(undo_len(&h, id), steps + 1);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Move"));
    assert_eq!(px(&h, id, 1, 70, 30), [255, 0, 0, 255]);
    assert_eq!(px(&h, id, 1, 40, 40)[3], 0);
    assert_eq!(
        px(&h, id, 0, 40, 40),
        [255, 255, 255, 255],
        "other layers untouched"
    );

    h.tap_key(Key::ArrowRight);
    assert_eq!(px(&h, id, 1, 71, 30), [255, 0, 0, 255]);
    h.modifiers = Modifiers::SHIFT;
    h.tap_key(Key::ArrowDown);
    h.modifiers = Modifiers::NONE;
    assert_eq!(px(&h, id, 1, 71, 40), [255, 0, 0, 255]);
    assert_eq!(undo_len(&h, id), steps + 3);
}

#[test]
fn free_transform_handles_enter_and_escape() {
    let (mut h, id) = setup();
    let original = h.app.state.document(id).unwrap().document.clone();
    h.shortcut(Modifiers::COMMAND, Key::T);
    assert!(h.app.transform_active());
    // The box covers the opaque layer (whole canvas); drag the bottom-right
    // handle to the middle: scale 0.5 anchored at the top-left.
    h.drag(at(&h, id, 200.0, 200.0), at(&h, id, 100.0, 100.0), 8);
    assert_eq!(px(&h, id, 0, 50, 50), [255, 255, 255, 255]);
    assert_eq!(px(&h, id, 0, 150, 150)[3], 0, "outside the scaled box");
    assert_eq!(undo_len(&h, id), original_steps(&h, id));
    h.tap_key(Key::Enter);
    assert!(!h.app.transform_active());
    assert_eq!(undo_label(&h, id).as_deref(), Some("Free Transform"));
    assert!(h.app.undo_current());
    assert_eq!(h.app.state.document(id).unwrap().document, original);

    // Escape cancels a move inside the box.
    h.shortcut(Modifiers::COMMAND, Key::T);
    h.drag(at(&h, id, 100.0, 100.0), at(&h, id, 160.0, 100.0), 6);
    assert_eq!(px(&h, id, 0, 30, 100)[3], 0);
    h.tap_key(Key::Escape);
    assert!(!h.app.transform_active());
    assert_eq!(h.app.state.document(id).unwrap().document, original);

    // Switching tools cancels too.
    h.shortcut(Modifiers::COMMAND, Key::T);
    h.drag(at(&h, id, 100.0, 100.0), at(&h, id, 160.0, 100.0), 6);
    h.tap_key(Key::B);
    assert!(!h.app.transform_active());
    assert_eq!(h.app.state.document(id).unwrap().document, original);
}

fn original_steps(h: &Harness, id: DocId) -> usize {
    // Fill Layer is the only recorded step before transforming.
    let _ = (h, id);
    1
}

#[test]
fn flip_through_the_app() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::Brush);
    h.app.tool_options_mut().brush.size = 1.0;
    h.app.tool_options_mut().brush.pixel = true;
    h.click_at(at(&h, id, 10.5, 20.5));
    assert!(h.app.flip_current(true));
    assert_eq!(px(&h, id, 0, 189, 20), [0, 0, 0, 255]);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Flip Horizontal"));
    assert!(h.app.flip_current(false));
    assert_eq!(px(&h, id, 0, 189, 179), [0, 0, 0, 255]);
}

fn has_text(output: &egui::FullOutput, needle: &str) -> bool {
    output.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(t) => t.galley.text().contains(needle),
        _ => false,
    })
}

#[test]
fn save_open_and_dirty_marker() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sprite.gac");
    let (mut h, id) = setup();
    h.app.layer_op(LayerOp::New);
    h.app.layer_action(LayerAction::Blend(BlendMode::Multiply));
    assert!(h.app.state.document(id).unwrap().is_dirty());
    assert!(has_text(&h.frame(vec![]), "Untitled-1 •"));

    assert!(h.app.save_to(&path));
    let open = h.app.state.document(id).unwrap();
    assert!(!open.is_dirty());
    assert_eq!(open.document.name, "sprite.gac");
    let out = h.frame(vec![]);
    assert!(has_text(&out, "sprite.gac") && !has_text(&out, "sprite.gac •"));

    let saved = h.app.state.document(id).unwrap().document.clone();
    let reopened = h.app.open_path(&path).unwrap();
    let doc = &h.app.state.document(reopened).unwrap().document;
    assert_eq!(doc.layers(), saved.layers());
    assert_eq!(doc.active_layer(), saved.active_layer());

    // Undo back to the saved state clears the marker again.
    h.app.state.set_current(id);
    h.app.layer_action(LayerAction::ToggleVisible(1));
    assert!(h.app.state.document(id).unwrap().is_dirty());
    h.app.undo_current();
    assert!(!h.app.state.document(id).unwrap().is_dirty());

    // A corrupt project shows an error and opens nothing.
    let bad = dir.path().join("broken.gac");
    std::fs::write(&bad, b"garbage").unwrap();
    let tabs = h.app.document_tabs().len();
    assert!(h.app.open_path(&bad).is_none());
    assert!(h.app.error_message().is_some());
    assert_eq!(h.app.document_tabs().len(), tabs);
}

#[test]
fn close_and_quit_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let (mut h, a) = setup(); // dirty (filled)
    h.app.close_current();
    assert_eq!(
        h.app.unsaved_prompt().map(|p| p.queue.clone()),
        Some(vec![a])
    );
    h.frame(vec![]); // the prompt renders
    h.app.answer_prompt(PromptAnswer::Cancel);
    assert!(h.app.unsaved_prompt().is_none());
    assert!(h.app.document_tabs().contains(&a));
    h.app.close_current();
    h.app.answer_prompt(PromptAnswer::DontSave);
    assert!(!h.app.document_tabs().contains(&a));

    // Quit asks for each dirty document; Save writes a document with a project path.
    let b = h.new_doc(10, 10);
    let c = h.new_doc(10, 10);
    let clean = h.new_doc(10, 10);
    for id in [b, c] {
        h.app.state.set_current(id);
        h.app.layer_op(LayerOp::New);
    }
    let path = dir.path().join("b.gac");
    h.app.state.set_current(b);
    assert!(h.app.save_to(&path));
    h.app.layer_op(LayerOp::New); // dirty again, but has a .gac path
    h.app.request_quit();
    assert_eq!(h.app.unsaved_prompt().unwrap().queue, vec![b, c]);
    assert!(h.app.unsaved_prompt().unwrap().quitting);
    h.app.answer_prompt(PromptAnswer::Save);
    assert!(!h.app.document_tabs().contains(&b));
    assert_eq!(
        game_asset_creator::project::load_project(&path)
            .unwrap()
            .layer_count(),
        3
    );
    assert!(!h.app.wants_quit());
    h.app.answer_prompt(PromptAnswer::Cancel);
    assert!(!h.app.wants_quit(), "cancel aborts quitting");
    assert!(h.app.document_tabs().contains(&c) && h.app.document_tabs().contains(&clean));

    h.app.request_quit();
    assert_eq!(h.app.unsaved_prompt().unwrap().queue, vec![c]);
    h.app.answer_prompt(PromptAnswer::DontSave);
    assert!(h.app.wants_quit());
}

#[test]
fn window_close_button_asks_first() {
    let (mut h, _) = setup();
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, common::SCREEN)),
        ..Default::default()
    };
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    let app = &mut h.app;
    let mut out = h.ctx.run_ui(input, |ui| app.show(ui));
    out.textures_delta.clear();
    assert!(h.app.unsaved_prompt().is_some_and(|p| p.quitting));
    assert!(!h.app.wants_quit());
    let cancelled = out
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .is_some_and(|v| v.commands.contains(&egui::ViewportCommand::CancelClose));
    assert!(cancelled);
}

#[test]
fn interpolation_option_applies_to_open_transform() {
    let (mut h, id) = setup();
    assert_eq!(
        h.app.tool_options().transform_interpolation,
        game_asset_creator::transform::Interpolation::Bilinear
    );
    h.shortcut(Modifiers::COMMAND, Key::T);
    h.app.tool_options_mut().transform_interpolation =
        game_asset_creator::transform::Interpolation::Nearest;
    h.frame(vec![]);
    let session = h
        .app
        .state
        .document(id)
        .unwrap()
        .transform()
        .unwrap()
        .interpolation;
    assert_eq!(
        session,
        game_asset_creator::transform::Interpolation::Nearest
    );
    h.tap_key(Key::Escape);
}

#[test]
fn cmd_q_goes_through_the_unsaved_prompt() {
    let (mut h, id) = setup();
    assert!(
        h.ctx.options(|o| o.quit_shortcuts.is_empty()),
        "egui's own Cmd+Q close is disabled"
    );
    h.shortcut(Modifiers::COMMAND, Key::Q);
    assert_eq!(
        h.app.unsaved_prompt().map(|p| p.queue.clone()),
        Some(vec![id])
    );
    assert!(!h.app.wants_quit());
}
