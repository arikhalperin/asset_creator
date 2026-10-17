//! Selection tools, Select menu, and marching ants driven through headless
//! egui frames with synthetic input.

use egui::{Key, Modifiers, PointerButton, Pos2, Vec2};
use game_asset_creator::canvas::ViewAction;
use game_asset_creator::selection::{self, CombineMode};
use game_asset_creator::tools::Tool;
use game_asset_creator::ui::RadiusOp;
use game_asset_creator::{DocId, PixelRect};

mod common;
use common::Harness;

/// A 200×200 document shown at 100% (one image pixel per point).
fn setup() -> (Harness, DocId) {
    let mut h = Harness::new();
    let id = h.new_doc(200, 200);
    h.app.view_action(ViewAction::ActualSize);
    h.frames(2);
    (h, id)
}

fn sel_bounds(h: &Harness, id: DocId) -> Option<PixelRect> {
    h.app
        .state
        .document(id)
        .unwrap()
        .document
        .selection()
        .and_then(selection::bounds)
}

fn mask_at(h: &Harness, id: DocId, x: u32, y: u32) -> u8 {
    h.app
        .state
        .document(id)
        .unwrap()
        .document
        .selection()
        .map_or(0, |m| m.get_pixel(x, y)[0])
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

fn drag_image(h: &mut Harness, id: DocId, from: (f32, f32), to: (f32, f32)) {
    let a = h.screen_of(id, Pos2::new(from.0, from.1));
    let b = h.screen_of(id, Pos2::new(to.0, to.1));
    h.drag(a, b, 6);
}

fn click_image(h: &mut Harness, id: DocId, at: (f32, f32)) {
    let p = h.screen_of(id, Pos2::new(at.0, at.1));
    h.click_at(p);
}

#[test]
fn tools_panel_default_and_single_key_shortcuts() {
    let mut h = Harness::new();
    h.new_doc(50, 50);
    assert_eq!(h.app.tool(), Tool::RectSelect);
    h.tap_key(Key::M);
    assert_eq!(h.app.tool(), Tool::EllipseSelect);
    h.tap_key(Key::M);
    assert_eq!(h.app.tool(), Tool::RectSelect);
    h.tap_key(Key::L);
    assert_eq!(h.app.tool(), Tool::Lasso);
    h.tap_key(Key::L);
    assert_eq!(h.app.tool(), Tool::PolygonLasso);
    h.tap_key(Key::W);
    assert_eq!(h.app.tool(), Tool::MagicWand);

    // Cmd/Ctrl held: not a tool shortcut.
    h.modifiers = Modifiers::COMMAND;
    h.tap_key(Key::M);
    h.modifiers = Modifiers::NONE;
    assert_eq!(h.app.tool(), Tool::MagicWand);

    // While typing in a text field, letters are text.
    h.app.show_new_dialog();
    h.frame(vec![]);
    for _ in 0..20 {
        if h.ctx.text_edit_focused() {
            break;
        }
        h.tap_key(Key::Tab);
    }
    assert!(h.ctx.text_edit_focused());
    h.tap_key(Key::M);
    assert_eq!(h.app.tool(), Tool::MagicWand);
}

#[test]
fn rectangle_drag_selects_and_click_clears() {
    let (mut h, id) = setup();
    let uploads = h.app.canvas(id).unwrap().tiles().uploads();
    drag_image(&mut h, id, (10.0, 10.0), (50.0, 30.0));
    assert_eq!(sel_bounds(&h, id), Some(PixelRect::new(10, 10, 40, 20)));
    assert_eq!(undo_label(&h, id).as_deref(), Some("Rectangle Select"));
    assert_eq!(mask_at(&h, id, 30, 20), 255);
    assert_eq!(
        h.app.canvas(id).unwrap().tiles().uploads(),
        uploads,
        "selection re-uploads no tiles"
    );

    // A click without dragging clears the selection (Replace mode).
    click_image(&mut h, id, (100.0, 100.0));
    assert_eq!(sel_bounds(&h, id), None);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Deselect"));

    // Undo/redo restore exact masks.
    assert!(h.app.undo_current());
    assert_eq!(sel_bounds(&h, id), Some(PixelRect::new(10, 10, 40, 20)));
    assert!(h.app.undo_current());
    assert_eq!(sel_bounds(&h, id), None);
    assert!(h.app.redo_current());
    assert_eq!(sel_bounds(&h, id), Some(PixelRect::new(10, 10, 40, 20)));
}

#[test]
fn modifiers_choose_combine_mode() {
    let (mut h, id) = setup();
    drag_image(&mut h, id, (10.0, 10.0), (50.0, 50.0));

    h.modifiers = Modifiers::SHIFT;
    drag_image(&mut h, id, (40.0, 40.0), (80.0, 80.0));
    h.modifiers = Modifiers::NONE;
    assert_eq!(
        sel_bounds(&h, id),
        Some(PixelRect::new(10, 10, 70, 70)),
        "Shift adds"
    );
    assert_eq!(mask_at(&h, id, 70, 20), 0);

    h.modifiers = Modifiers::ALT;
    drag_image(&mut h, id, (10.0, 10.0), (30.0, 30.0));
    h.modifiers = Modifiers::NONE;
    assert_eq!(mask_at(&h, id, 20, 20), 0, "Option/Alt subtracts");
    assert_eq!(mask_at(&h, id, 45, 45), 255);

    h.modifiers = Modifiers::SHIFT | Modifiers::ALT;
    drag_image(&mut h, id, (40.0, 40.0), (60.0, 60.0));
    h.modifiers = Modifiers::NONE;
    assert_eq!(
        sel_bounds(&h, id),
        Some(PixelRect::new(40, 40, 20, 20)),
        "Shift+Option intersects"
    );

    // The tool-options mode applies when no modifier is held.
    h.app.tool_options_mut().mode = CombineMode::Add;
    drag_image(&mut h, id, (100.0, 100.0), (110.0, 110.0));
    assert_eq!(mask_at(&h, id, 50, 50), 255);
    assert_eq!(mask_at(&h, id, 105, 105), 255);
}

#[test]
fn ellipse_drag_with_and_without_antialias() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::EllipseSelect);
    drag_image(&mut h, id, (20.0, 20.0), (80.0, 60.0));
    assert_eq!(undo_label(&h, id).as_deref(), Some("Ellipse Select"));
    assert_eq!(mask_at(&h, id, 50, 40), 255);
    assert_eq!(mask_at(&h, id, 21, 21), 0);
    let mask = h
        .app
        .state
        .document(id)
        .unwrap()
        .document
        .selection()
        .unwrap()
        .clone();
    assert!(
        mask.iter().any(|v| *v > 0 && *v < 255),
        "soft edge by default"
    );

    h.app.tool_options_mut().antialias = false;
    drag_image(&mut h, id, (20.0, 20.0), (80.0, 60.0));
    let mask = h
        .app
        .state
        .document(id)
        .unwrap()
        .document
        .selection()
        .unwrap()
        .clone();
    assert!(
        mask.iter().all(|v| *v == 0 || *v == 255),
        "hard edge when off"
    );
}

#[test]
fn lasso_drag_selects_enclosed_area() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::Lasso);
    let pts = [(20.0, 20.0), (120.0, 20.0), (20.0, 120.0)];
    let screen: Vec<Pos2> = pts
        .iter()
        .map(|p| h.screen_of(id, Pos2::new(p.0, p.1)))
        .collect();
    h.pointer = screen[0];
    h.frame(vec![]);
    let down = h.press(PointerButton::Primary, true);
    h.frame(vec![down]);
    for target in [screen[1], screen[2]] {
        let start = h.pointer;
        for i in 1..=10 {
            h.pointer = start + (target - start) * (i as f32 / 10.0);
            h.frame(vec![]);
        }
    }
    let up = h.press(PointerButton::Primary, false);
    h.frame(vec![up]);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Lasso Select"));
    assert_eq!(mask_at(&h, id, 40, 40), 255);
    assert_eq!(mask_at(&h, id, 100, 100), 0);
}

#[test]
fn polygon_lasso_clicks_enter_backspace_escape() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::PolygonLasso);
    for p in [(10.0, 10.0), (100.0, 10.0), (100.0, 100.0), (150.0, 150.0)] {
        click_image(&mut h, id, p);
    }
    assert_eq!(h.app.canvas(id).unwrap().polygon_points(), Some(4));
    h.tap_key(Key::Backspace);
    assert_eq!(h.app.canvas(id).unwrap().polygon_points(), Some(3));
    assert_eq!(sel_bounds(&h, id), None, "not applied before closing");
    h.tap_key(Key::Enter);
    assert_eq!(h.app.canvas(id).unwrap().polygon_points(), None);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Polygon Select"));
    assert_eq!(mask_at(&h, id, 80, 30), 255);
    assert_eq!(mask_at(&h, id, 20, 80), 0);

    // Escape cancels.
    click_image(&mut h, id, (150.0, 10.0));
    click_image(&mut h, id, (190.0, 10.0));
    h.tap_key(Key::Escape);
    assert_eq!(h.app.canvas(id).unwrap().polygon_points(), None);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Polygon Select"));

    // Enter with fewer than three vertices cancels without selecting.
    click_image(&mut h, id, (150.0, 10.0));
    click_image(&mut h, id, (190.0, 10.0));
    h.tap_key(Key::Enter);
    assert_eq!(h.app.canvas(id).unwrap().polygon_points(), None);
    assert_eq!(mask_at(&h, id, 80, 30), 255, "selection unchanged");
}

#[test]
fn polygon_closes_on_first_vertex_and_on_double_click() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::PolygonLasso);
    h.app.tool_options_mut().mode = CombineMode::Replace;
    for p in [(150.0, 150.0), (190.0, 150.0), (190.0, 190.0)] {
        click_image(&mut h, id, p);
    }
    // Within 6 points of the first vertex.
    click_image(&mut h, id, (153.0, 152.0));
    assert_eq!(h.app.canvas(id).unwrap().polygon_points(), None);
    assert_eq!(mask_at(&h, id, 185, 160), 255);

    // Double-click closes.
    for p in [(10.0, 10.0), (60.0, 10.0)] {
        click_image(&mut h, id, p);
    }
    click_image(&mut h, id, (60.0, 60.0));
    h.click(); // second click at the same spot → double-click
    assert_eq!(h.app.canvas(id).unwrap().polygon_points(), None);
    assert_eq!(mask_at(&h, id, 50, 20), 255);
    assert_eq!(mask_at(&h, id, 185, 160), 0, "replaced");
}

#[test]
fn clicking_first_vertex_with_too_few_points_cancels() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::PolygonLasso);
    click_image(&mut h, id, (10.0, 10.0));
    click_image(&mut h, id, (80.0, 10.0));
    assert_eq!(h.app.canvas(id).unwrap().polygon_points(), Some(2));
    // Back on the first vertex (pause past the double-click window first).
    h.time = Some(100.0);
    click_image(&mut h, id, (12.0, 11.0));
    assert_eq!(
        h.app.canvas(id).unwrap().polygon_points(),
        None,
        "cancelled"
    );
    assert_eq!(sel_bounds(&h, id), None, "nothing selected");
    assert_eq!(undo_label(&h, id), None);
}

#[test]
fn switching_tools_cancels_polygon() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::PolygonLasso);
    click_image(&mut h, id, (10.0, 10.0));
    click_image(&mut h, id, (50.0, 10.0));
    assert!(h.app.canvas(id).unwrap().has_gesture());
    h.tap_key(Key::M);
    assert!(!h.app.canvas(id).unwrap().has_gesture());
}

#[test]
fn magic_wand_selects_matching_region_of_active_layer() {
    let (mut h, id) = setup();
    // Paint the left half red through a selection + fill.
    drag_image(&mut h, id, (0.0, 0.0), (100.0, 200.0));
    h.app.open_fill_dialog();
    h.app.set_fill_dialog_color([220, 0, 0, 255]);
    assert!(h.app.submit_fill_dialog());
    assert!(h.app.deselect());

    h.app.set_tool(Tool::MagicWand);
    click_image(&mut h, id, (50.0, 50.0));
    assert_eq!(undo_label(&h, id).as_deref(), Some("Magic Wand"));
    assert_eq!(sel_bounds(&h, id), Some(PixelRect::new(0, 0, 100, 200)));
    click_image(&mut h, id, (150.0, 50.0));
    assert_eq!(sel_bounds(&h, id), Some(PixelRect::new(100, 0, 100, 200)));
    // Clicking outside the image does nothing.
    let before = undo_label(&h, id);
    h.click_at(h.screen_of(id, Pos2::new(-30.0, 50.0)));
    assert_eq!(undo_label(&h, id), before);
}

#[test]
fn select_menu_shortcuts() {
    let (mut h, id) = setup();
    h.shortcut(Modifiers::COMMAND, Key::A);
    assert_eq!(sel_bounds(&h, id), Some(PixelRect::new(0, 0, 200, 200)));
    assert_eq!(undo_label(&h, id).as_deref(), Some("Select All"));
    h.shortcut(Modifiers::COMMAND | Modifiers::SHIFT, Key::I);
    assert_eq!(
        sel_bounds(&h, id),
        None,
        "inverting everything selects nothing"
    );
    assert_eq!(undo_label(&h, id).as_deref(), Some("Invert Selection"));
    h.shortcut(Modifiers::COMMAND | Modifiers::SHIFT, Key::I);
    assert_eq!(
        sel_bounds(&h, id),
        Some(PixelRect::new(0, 0, 200, 200)),
        "invert of none selects all"
    );
    h.shortcut(Modifiers::COMMAND, Key::D);
    assert_eq!(sel_bounds(&h, id), None);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Deselect"));
}

#[test]
fn grow_shrink_feather_dialogs() {
    let (mut h, id) = setup();
    h.app.open_radius_dialog(RadiusOp::Grow);
    assert!(h.app.radius_dialog().is_none(), "needs a selection");

    drag_image(&mut h, id, (50.0, 50.0), (100.0, 100.0));
    h.app.open_radius_dialog(RadiusOp::Grow);
    assert_eq!(h.app.radius_dialog().unwrap().radius, "2");
    h.frame(vec![]); // dialog renders
    h.app.set_radius_text("abc");
    assert!(!h.app.submit_radius_dialog());
    assert!(h.app.radius_dialog().unwrap().error.is_some());
    h.app.set_radius_text("101");
    assert!(!h.app.submit_radius_dialog());
    h.app.set_radius_text("4");
    assert!(h.app.submit_radius_dialog());
    assert!(h.app.radius_dialog().is_none());
    assert_eq!(undo_label(&h, id).as_deref(), Some("Grow Selection"));
    assert_eq!(sel_bounds(&h, id), Some(PixelRect::new(46, 46, 58, 58)));

    h.app.open_radius_dialog(RadiusOp::Shrink);
    h.app.set_radius_text("4");
    assert!(h.app.submit_radius_dialog());
    assert_eq!(sel_bounds(&h, id), Some(PixelRect::new(50, 50, 50, 50)));

    h.app.open_radius_dialog(RadiusOp::Feather);
    assert_eq!(h.app.radius_dialog().unwrap().radius, "5");
    assert!(h.app.submit_radius_dialog());
    assert_eq!(undo_label(&h, id).as_deref(), Some("Feather Selection"));
    let soft: Vec<u8> = (40..60).map(|x| mask_at(&h, id, x, 75)).collect();
    assert!(soft.iter().any(|v| *v > 0 && *v < 255), "{soft:?}");
    assert_eq!(mask_at(&h, id, 75, 75), 255);

    // Cancel does nothing.
    h.app.open_radius_dialog(RadiusOp::Grow);
    h.app.cancel_radius_dialog();
    assert_eq!(undo_label(&h, id).as_deref(), Some("Feather Selection"));
}

fn dash_positions(output: &egui::FullOutput) -> Vec<[Pos2; 2]> {
    output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::LineSegment { points, stroke } if stroke.color == egui::Color32::BLACK => {
                Some(*points)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn marching_ants_animate_and_request_repaint() {
    let (mut h, id) = setup();
    drag_image(&mut h, id, (20.0, 20.0), (120.0, 90.0));
    h.time = Some(10.0);
    let first = h.frame(vec![]);
    let a = dash_positions(&first);
    assert!(a.len() > 20, "dashes drawn: {}", a.len());
    let repaint = first
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map(|v| v.repaint_delay)
        .unwrap();
    assert!(
        repaint <= std::time::Duration::from_millis(100),
        "{repaint:?}"
    );

    h.time = Some(10.1);
    let b = dash_positions(&h.frame(vec![]));
    assert_ne!(a, b, "dash pattern moves over time");
    // Two full periods later (0.5 s) the pattern repeats.
    h.time = Some(10.5);
    let c = dash_positions(&h.frame(vec![]));
    assert_eq!(a.len(), c.len());
    for (p, q) in a.iter().zip(&c) {
        assert!(p[0].distance(q[0]) < 0.01 && p[1].distance(q[1]) < 0.01);
    }

    // No selection: no ants.
    h.app.deselect();
    assert!(dash_positions(&h.frame(vec![])).is_empty());
}

#[test]
fn navigation_still_works_with_selection_tools() {
    let (mut h, id) = setup();
    let start = h.app.canvas(id).unwrap().view().unwrap().offset;
    // Space+drag pans without selecting.
    let space = h.key(Key::Space, true);
    h.frame(vec![space]);
    let from = h.pointer;
    h.drag(from, from + Vec2::new(60.0, 30.0), 5);
    let space_up = h.key(Key::Space, false);
    h.frame(vec![space_up]);
    let moved = h.app.canvas(id).unwrap().view().unwrap().offset - start;
    assert!(moved.x > 40.0, "{moved:?}");
    assert_eq!(sel_bounds(&h, id), None);

    // Middle drag pans without selecting.
    let down = h.press(PointerButton::Middle, true);
    h.frame(vec![down]);
    for _ in 0..4 {
        h.pointer += Vec2::new(-10.0, 0.0);
        h.frame(vec![]);
    }
    let up = h.press(PointerButton::Middle, false);
    h.frame(vec![up]);
    assert_eq!(sel_bounds(&h, id), None);

    // Pinch still zooms.
    let zoom = h.app.canvas(id).unwrap().view().unwrap().zoom;
    h.frame(vec![egui::Event::Zoom(1.5)]);
    assert!(h.app.canvas(id).unwrap().view().unwrap().zoom > zoom);
}

#[test]
fn crosshair_cursor_over_image() {
    let (mut h, id) = setup();
    h.pointer = h.screen_of(id, Pos2::new(100.0, 100.0));
    let out = h.frame(vec![]);
    assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::Crosshair);
}

#[test]
fn fill_respects_selection_through_the_ui() {
    let (mut h, id) = setup();
    let uploads = h.app.canvas(id).unwrap().tiles().uploads();
    drag_image(&mut h, id, (10.0, 10.0), (30.0, 30.0));
    h.app.open_fill_dialog();
    h.app.set_fill_dialog_color([0, 0, 255, 255]);
    assert!(h.app.submit_fill_dialog());
    h.frame(vec![]);
    let doc = &h.app.state.document(id).unwrap().document;
    let px = |x, y| doc.layer(0).unwrap().pixels.get_pixel(x, y).0;
    assert_eq!(px(20, 20), [0, 0, 255, 255]);
    assert_eq!(px(5, 5), [0, 0, 0, 0]);
    assert_eq!(px(31, 20), [0, 0, 0, 0]);
    // Only the one tile containing the selection was refreshed.
    assert_eq!(h.app.canvas(id).unwrap().tiles().uploads(), uploads + 1);
}
