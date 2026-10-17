//! Paint tools driven through headless egui frames with synthetic input.

use egui::{Key, Modifiers, PointerButton, Pos2, Vec2};
use game_asset_creator::canvas::ViewAction;
use game_asset_creator::paint::{GradientColors, GradientKind};
use game_asset_creator::tools::Tool;
use game_asset_creator::ui::CLONE_SOURCE_HINT;
use game_asset_creator::DocId;

mod common;
use common::Harness;

/// A 300×200 opaque white document at 100%.
fn setup() -> (Harness, DocId) {
    let mut h = Harness::new();
    let id = h.new_doc(300, 200);
    h.app.open_fill_dialog();
    h.app.set_fill_dialog_color([255, 255, 255, 255]);
    assert!(h.app.submit_fill_dialog());
    h.app.view_action(ViewAction::ActualSize);
    h.frames(2);
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

fn at(h: &Harness, id: DocId, x: f32, y: f32) -> Pos2 {
    h.screen_of(id, Pos2::new(x, y))
}

/// Presses at `points[0]`, jumps to each following point in one frame
/// (a very fast mouse), and releases.
fn fast_stroke(h: &mut Harness, id: DocId, points: &[(f32, f32)]) {
    h.pointer = at(h, id, points[0].0, points[0].1);
    h.frame(vec![]);
    let down = h.press(PointerButton::Primary, true);
    h.frame(vec![down]);
    for p in &points[1..] {
        h.pointer = at(h, id, p.0, p.1);
        h.frame(vec![]);
    }
    let up = h.press(PointerButton::Primary, false);
    h.frame(vec![up]);
    h.frame(vec![]);
}

#[test]
fn paint_tool_shortcuts_colors_and_size() {
    let (mut h, _) = setup();
    for (key, tool) in [
        (Key::B, Tool::Brush),
        (Key::E, Tool::Eraser),
        (Key::I, Tool::Eyedropper),
        (Key::G, Tool::Gradient),
        (Key::G, Tool::PaintBucket),
        (Key::G, Tool::Gradient),
        (Key::S, Tool::CloneStamp),
    ] {
        h.tap_key(key);
        assert_eq!(h.app.tool(), tool);
    }
    h.modifiers = Modifiers::COMMAND;
    h.tap_key(Key::B);
    h.modifiers = Modifiers::NONE;
    assert_eq!(h.app.tool(), Tool::CloneStamp, "Cmd+B is not a tool key");

    h.app.colors_mut().foreground = [10, 20, 30, 255];
    h.tap_key(Key::X);
    assert_eq!(h.app.colors().background, [10, 20, 30, 255]);
    assert_eq!(h.app.colors().foreground, [255, 255, 255, 255]);
    h.tap_key(Key::D);
    assert_eq!(h.app.colors().foreground, [0, 0, 0, 255]);
    assert_eq!(h.app.colors().background, [255, 255, 255, 255]);

    h.tap_key(Key::B);
    h.tap_key(Key::CloseBracket);
    assert_eq!(h.app.tool_options().brush.size, 24.0);
    h.tap_key(Key::OpenBracket);
    h.tap_key(Key::OpenBracket);
    assert!(h.app.tool_options().brush.size < 20.0);
    assert_eq!(h.app.tool_options().eraser.size, 20.0, "per-tool values");
}

#[test]
fn fast_brush_stroke_is_continuous_live_and_one_undo_step() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::Brush);
    h.app.tool_options_mut().brush.size = 8.0;
    let steps = undo_len(&h, id);
    let uploads = h.app.canvas(id).unwrap().tiles().uploads();

    h.pointer = at(&h, id, 20.0, 100.0);
    h.frame(vec![]);
    let down = h.press(PointerButton::Primary, true);
    h.frame(vec![down]);
    for p in [(220.0, 60.0), (280.0, 150.0)] {
        h.pointer = at(&h, id, p.0, p.1);
        h.frame(vec![]);
    }
    // Live: painted and tiles refreshed before release, nothing recorded yet.
    assert_eq!(px(&h, id, 20, 100), [0, 0, 0, 255]);
    assert!(h.app.canvas(id).unwrap().tiles().uploads() > uploads);
    assert_eq!(undo_len(&h, id), steps);
    let up = h.press(PointerButton::Primary, false);
    h.frame(vec![up]);
    h.frame(vec![]);

    assert_eq!(undo_len(&h, id), steps + 1);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Brush Stroke"));
    for x in 20..280 {
        let painted = (0..200).any(|y| px(&h, id, x, y)[0] < 128);
        assert!(painted, "gap at column {x}");
    }
    // Anti-aliased: some edge pixels are partially painted.
    let partial = (0..200).filter(|y| {
        let v = px(&h, id, 100, *y)[0];
        v > 0 && v < 255
    });
    assert!(partial.count() >= 1);

    assert!(h.app.undo_current());
    assert!((0..300).all(|x| px(&h, id, x, 100) == [255, 255, 255, 255]));
}

#[test]
fn shift_click_paints_a_straight_line() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::Brush);
    h.app.tool_options_mut().brush.size = 4.0;
    fast_stroke(&mut h, id, &[(20.0, 20.0), (21.0, 20.0)]);
    h.modifiers = Modifiers::SHIFT;
    h.click_at(at(&h, id, 200.0, 120.0));
    h.modifiers = Modifiers::NONE;
    assert_eq!(undo_label(&h, id).as_deref(), Some("Brush Stroke"));
    // Points on the line from (21,20) to (200,120).
    for t in [0.25f32, 0.5, 0.75] {
        let x = 21.0 + (200.0 - 21.0) * t;
        let y = 20.0 + (120.0 - 20.0) * t;
        assert_eq!(px(&h, id, x as u32, y as u32)[0], 0, "on line at t={t}");
    }
    assert_eq!(px(&h, id, 200, 20), [255, 255, 255, 255]);
}

#[test]
fn eraser_makes_pixels_transparent() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::Eraser);
    fast_stroke(&mut h, id, &[(50.0, 50.0), (150.0, 50.0)]);
    assert_eq!(px(&h, id, 100, 50)[3], 0);
    assert_eq!(px(&h, id, 100, 150)[3], 255);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Eraser Stroke"));
}

#[test]
fn eyedropper_sets_foreground_and_background_without_undo() {
    let (mut h, id) = setup();
    // Paint a red area with the bucket? Use a brush stroke with a red color.
    h.app.set_tool(Tool::Brush);
    h.app.colors_mut().foreground = [200, 10, 10, 255];
    h.app.tool_options_mut().brush.size = 30.0;
    fast_stroke(&mut h, id, &[(60.0, 60.0), (61.0, 60.0)]);
    h.app.colors_mut().reset();
    let steps = undo_len(&h, id);

    h.app.set_tool(Tool::Eyedropper);
    h.click_at(at(&h, id, 60.0, 60.0));
    assert_eq!(h.app.colors().foreground, [200, 10, 10, 255]);
    h.modifiers = Modifiers::ALT;
    h.click_at(at(&h, id, 250.0, 150.0));
    h.modifiers = Modifiers::NONE;
    assert_eq!(h.app.colors().background, [255, 255, 255, 255]);
    h.app.colors_mut().background = [1, 1, 1, 1];
    h.modifiers = Modifiers::ALT;
    h.click_at(at(&h, id, 60.0, 60.0));
    h.modifiers = Modifiers::NONE;
    assert_eq!(h.app.colors().background, [200, 10, 10, 255]);
    assert_eq!(undo_len(&h, id), steps, "picking is not an edit");

    // Active-layer sampling of a single-layer document matches the composite.
    h.app.tool_options_mut().sample_all_layers = false;
    h.app.colors_mut().reset();
    h.click_at(at(&h, id, 60.0, 60.0));
    assert_eq!(h.app.colors().foreground, [200, 10, 10, 255]);
}

#[test]
fn paint_bucket_fills_region() {
    let (mut h, id) = setup();
    // Draw a black vertical wall splitting the image.
    h.app.set_tool(Tool::Brush);
    h.app.tool_options_mut().brush.size = 6.0;
    h.app.tool_options_mut().brush.pixel = true;
    fast_stroke(&mut h, id, &[(150.0, -5.0), (150.0, 210.0)]);

    h.app.set_tool(Tool::PaintBucket);
    h.app.colors_mut().foreground = [0, 128, 255, 255];
    h.click_at(at(&h, id, 50.0, 50.0));
    assert_eq!(undo_label(&h, id).as_deref(), Some("Paint Bucket"));
    assert_eq!(px(&h, id, 10, 10), [0, 128, 255, 255]);
    assert_eq!(
        px(&h, id, 250, 10),
        [255, 255, 255, 255],
        "wall stops the fill"
    );
    assert_eq!(px(&h, id, 150, 100), [0, 0, 0, 255]);
}

#[test]
fn gradient_drag_paints_linear_and_radial() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::Gradient);
    h.app.colors_mut().foreground = [0, 0, 0, 255];
    h.app.colors_mut().background = [255, 0, 0, 255];
    h.drag(at(&h, id, 0.5, 100.0), at(&h, id, 299.5, 100.0), 8);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Gradient"));
    assert_eq!(px(&h, id, 0, 50), [0, 0, 0, 255]);
    assert_eq!(px(&h, id, 299, 50), [255, 0, 0, 255]);
    let mid = px(&h, id, 150, 50);
    assert!((mid[0] as i32 - 128).abs() <= 3 && mid[1] == 0, "{mid:?}");

    h.app.tool_options_mut().gradient_kind = GradientKind::Radial;
    h.app.tool_options_mut().gradient_colors = GradientColors::ForegroundToTransparent;
    h.drag(at(&h, id, 150.0, 100.0), at(&h, id, 250.0, 100.0), 8);
    assert!(px(&h, id, 150, 100)[0] <= 3, "foreground at the center");
    // Far corner (beyond the radius) is unchanged by the transparent end.
    assert_eq!(px(&h, id, 299, 0), [255, 0, 0, 255]);
}

#[test]
fn clone_stamp_needs_source_and_copies_aligned() {
    let (mut h, id) = setup();
    // A black square at (20..40, 20..40).
    h.app.set_tool(Tool::Brush);
    h.app.tool_options_mut().brush.pixel = true;
    h.app.tool_options_mut().brush.size = 20.0;
    fast_stroke(&mut h, id, &[(30.0, 30.0), (30.5, 30.0)]);
    let steps = undo_len(&h, id);

    h.app.set_tool(Tool::CloneStamp);
    h.app.tool_options_mut().clone.size = 6.0;
    fast_stroke(&mut h, id, &[(200.0, 30.0), (201.0, 30.0)]);
    assert_eq!(h.app.status_message(), Some(CLONE_SOURCE_HINT));
    assert_eq!(undo_len(&h, id), steps, "no source: nothing painted");
    assert!(h.app.status_text().contains(CLONE_SOURCE_HINT));

    h.modifiers = Modifiers::ALT;
    h.click_at(at(&h, id, 30.0, 30.0));
    h.modifiers = Modifiers::NONE;
    assert_eq!(h.app.canvas(id).unwrap().clone_source(), Some((30.0, 30.0)));
    fast_stroke(&mut h, id, &[(200.0, 30.0), (201.0, 30.0)]);
    assert_eq!(undo_label(&h, id).as_deref(), Some("Clone Stamp"));
    assert_eq!(
        px(&h, id, 200, 30),
        [0, 0, 0, 255],
        "copied from the square"
    );
    assert_eq!(
        h.app.canvas(id).unwrap().clone_offset(),
        Some((-170.0, 0.0))
    );

    // Aligned: a second stroke keeps the offset (samples 170 px to the left).
    fast_stroke(&mut h, id, &[(260.0, 150.0), (261.0, 150.0)]);
    assert_eq!(
        px(&h, id, 260, 150),
        [255, 255, 255, 255],
        "source (90,150) is white"
    );

    // Not aligned: each stroke restarts at the source.
    h.app.tool_options_mut().clone_aligned = false;
    fast_stroke(&mut h, id, &[(260.0, 150.0), (261.0, 150.0)]);
    assert_eq!(px(&h, id, 260, 150), [0, 0, 0, 255]);
}

#[test]
fn paint_tools_respect_the_selection() {
    let (mut h, id) = setup();
    // Select the left half.
    h.app.set_tool(Tool::RectSelect);
    h.drag(at(&h, id, 0.0, 0.0), at(&h, id, 150.0, 200.0), 6);
    h.app.set_tool(Tool::Brush);
    h.app.tool_options_mut().brush.size = 10.0;
    fast_stroke(&mut h, id, &[(20.0, 100.0), (280.0, 100.0)]);
    assert_eq!(px(&h, id, 100, 100), [0, 0, 0, 255]);
    assert_eq!(px(&h, id, 200, 100), [255, 255, 255, 255]);

    h.app.set_tool(Tool::PaintBucket);
    h.app.colors_mut().foreground = [0, 255, 0, 255];
    h.click_at(at(&h, id, 200.0, 20.0));
    assert_eq!(
        px(&h, id, 200, 20),
        [255, 255, 255, 255],
        "click outside selection"
    );
    h.click_at(at(&h, id, 50.0, 20.0));
    assert_eq!(px(&h, id, 50, 20), [0, 255, 0, 255]);
    assert_eq!(px(&h, id, 250, 20), [255, 255, 255, 255]);

    h.app.set_tool(Tool::Gradient);
    h.drag(at(&h, id, 0.5, 180.0), at(&h, id, 299.5, 180.0), 6);
    assert_eq!(px(&h, id, 290, 180), [255, 255, 255, 255]);
    assert_ne!(px(&h, id, 10, 180), [255, 255, 255, 255]);
}

#[test]
fn switching_tool_mid_stroke_discards_it() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::Brush);
    let steps = undo_len(&h, id);
    h.pointer = at(&h, id, 50.0, 50.0);
    h.frame(vec![]);
    let down = h.press(PointerButton::Primary, true);
    h.frame(vec![down]);
    h.pointer = at(&h, id, 150.0, 50.0);
    h.frame(vec![]);
    assert_eq!(px(&h, id, 100, 50), [0, 0, 0, 255]);
    h.tap_key(Key::E);
    assert_eq!(px(&h, id, 100, 50), [255, 255, 255, 255], "restored");
    let up = h.press(PointerButton::Primary, false);
    h.frame(vec![up]);
    assert_eq!(undo_len(&h, id), steps);
}

#[test]
fn brush_cursor_circle_and_options_strip_render() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::Brush);
    h.pointer = at(&h, id, 100.0, 100.0);
    let out = h.frame(vec![]);
    assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::None);
    let circles = out
        .shapes
        .iter()
        .filter(|s| matches!(&s.shape, egui::Shape::Circle(c) if (c.radius - 10.0).abs() < 0.01))
        .count();
    assert_eq!(circles, 1, "black outline of a 20 px brush at 100%");
    for tool in Tool::ALL {
        h.app.set_tool(tool);
        h.frame(vec![]);
    }
    h.app.set_tool(Tool::PaintBucket);
    let out = h.frame(vec![]);
    assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::Crosshair);
}

#[test]
fn space_drag_pans_instead_of_painting() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::Brush);
    let steps = undo_len(&h, id);
    let start = h.app.canvas(id).unwrap().view().unwrap().offset;
    let space = h.key(Key::Space, true);
    h.frame(vec![space]);
    let from = h.pointer;
    h.drag(from, from + Vec2::new(60.0, 0.0), 5);
    let up = h.key(Key::Space, false);
    h.frame(vec![up]);
    assert!((h.app.canvas(id).unwrap().view().unwrap().offset - start).x > 40.0);
    assert_eq!(undo_len(&h, id), steps);
}

#[test]
fn strokes_refresh_only_the_tiles_they_touch() {
    let mut h = Harness::new();
    let id = h.new_doc(1024, 1024); // 4×4 tiles
    h.app.view_action(ViewAction::Fit);
    h.frames(2);
    h.app.set_tool(Tool::Brush);
    h.app.tool_options_mut().brush.size = 10.0;
    let uploads = h.app.canvas(id).unwrap().tiles().uploads();
    // A short stroke well inside tile (1, 1): x, y in 256..512.
    fast_stroke(&mut h, id, &[(300.0, 300.0), (400.0, 380.0)]);
    let refreshed = h.app.canvas(id).unwrap().tiles().uploads() - uploads;
    assert!(refreshed >= 1, "the touched tile refreshed");
    // Live updates may refresh the same tile several times, but never another one:
    // undo then shows exactly one more upload for the single touched tile.
    let before_undo = h.app.canvas(id).unwrap().tiles().uploads();
    assert!(h.app.undo_current());
    h.frame(vec![]);
    assert_eq!(h.app.canvas(id).unwrap().tiles().uploads() - before_undo, 1);
}

#[test]
fn eraser_and_clone_respect_selection_through_the_ui() {
    let (mut h, id) = setup();
    h.app.set_tool(Tool::RectSelect);
    h.drag(at(&h, id, 0.0, 0.0), at(&h, id, 150.0, 200.0), 6);

    h.app.set_tool(Tool::Eraser);
    fast_stroke(&mut h, id, &[(20.0, 50.0), (280.0, 50.0)]);
    assert_eq!(px(&h, id, 100, 50)[3], 0);
    assert_eq!(px(&h, id, 200, 50), [255, 255, 255, 255]);

    // A black dot inside the selection becomes the clone source.
    h.app.set_tool(Tool::Brush);
    h.app.tool_options_mut().brush.size = 20.0;
    fast_stroke(&mut h, id, &[(40.0, 150.0), (41.0, 150.0)]);
    h.app.set_tool(Tool::CloneStamp);
    h.modifiers = Modifiers::ALT;
    h.click_at(at(&h, id, 40.0, 150.0));
    h.modifiers = Modifiers::NONE;
    // Clone it to x=100 (selected) and x=220 (unselected).
    fast_stroke(&mut h, id, &[(100.0, 150.0), (101.0, 150.0)]);
    assert_eq!(px(&h, id, 100, 150), [0, 0, 0, 255]);
    h.app.tool_options_mut().clone_aligned = false;
    fast_stroke(&mut h, id, &[(220.0, 150.0), (221.0, 150.0)]);
    assert_eq!(
        px(&h, id, 220, 150),
        [255, 255, 255, 255],
        "outside selection"
    );
}
