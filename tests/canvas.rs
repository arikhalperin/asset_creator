//! Canvas and undo behaviour driven through headless egui frames with
//! synthetic input, the same way the running app receives it.

use egui::{Event, Key, Modifiers, MouseWheelUnit, PointerButton, Vec2};
use game_asset_creator::canvas::{ViewAction, MAX_ZOOM, ZOOM_STEP};
use game_asset_creator::{Command, CommandError, Document, PixelRect};

mod common;
use common::{Harness, SCREEN};

/// Changes one pixel of layer 0 and reports a 1×1 dirty rect.
struct SetPixel {
    x: u32,
    y: u32,
    value: [u8; 4],
    old: Option<[u8; 4]>,
}

impl Command for SetPixel {
    fn label(&self) -> &str {
        "Set Pixel"
    }
    fn apply(&mut self, doc: &mut Document) -> Result<(), CommandError> {
        let px = doc
            .layer_mut(0)
            .unwrap()
            .pixels
            .get_pixel_mut(self.x, self.y);
        self.old = Some(px.0);
        px.0 = self.value;
        Ok(())
    }
    fn revert(&mut self, doc: &mut Document) {
        doc.layer_mut(0)
            .unwrap()
            .pixels
            .get_pixel_mut(self.x, self.y)
            .0 = self.old.unwrap();
    }
    fn dirty_rect(&self, _doc: &Document) -> PixelRect {
        PixelRect::new(self.x, self.y, 1, 1)
    }
    fn memory_cost(&self) -> usize {
        4
    }
}

#[test]
fn small_document_opens_at_100_percent_large_at_fit() {
    let mut h = Harness::new();
    let small = h.new_doc(64, 32);
    assert_eq!(h.view(small).zoom, 1.0);
    let large = h.new_doc(4000, 4000);
    let view = h.view(large);
    assert!(view.zoom < 0.25, "fit zoom {}", view.zoom);
    // Whole image visible: its rect fits inside the screen.
    let rect = view.image_rect([4000, 4000], 1.0);
    assert!(rect.width() <= SCREEN.x && rect.height() <= SCREEN.y);
    // First display uploads every tile exactly once (16 × 16 tiles).
    assert_eq!(h.uploads(large), 256);
}

#[test]
fn pinch_zoom_keeps_point_under_cursor_and_does_not_reupload() {
    let mut h = Harness::new();
    let id = h.new_doc(4000, 4000);
    let uploads = h.uploads(id);
    let canvas_min = h.app.canvas(id).unwrap().rect().unwrap().min;
    let anchor = h.pointer - canvas_min.to_vec2();
    let before = h.view(id);

    h.frame(vec![Event::Zoom(1.5)]);
    let after = h.view(id);
    assert!((after.zoom - before.zoom * 1.5).abs() < 1e-4);
    let img_before = before.view_to_image(anchor, 1.0);
    let img_after = after.view_to_image(anchor, 1.0);
    assert!(
        (img_before - img_after).length() < 1e-2,
        "{img_before:?} vs {img_after:?}"
    );
    assert_eq!(h.uploads(id), uploads, "zooming must not re-upload tiles");
}

#[test]
fn command_wheel_zooms_continuously() {
    let mut h = Harness::new();
    let id = h.new_doc(500, 400);
    let zoom = h.view(id).zoom;
    h.modifiers = Modifiers::COMMAND;
    let wheel = Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: Vec2::new(0.0, 7.0),
        modifiers: Modifiers::COMMAND,
        phase: egui::TouchPhase::Move,
    };
    h.frame(vec![wheel]);
    h.modifiers = Modifiers::NONE;
    let zoomed = h.view(id).zoom;
    assert!(zoomed > zoom, "{zoomed} > {zoom}");
    // Not a fixed step: a small wheel delta gives a small change.
    assert!(zoomed < zoom * ZOOM_STEP);
}

#[test]
fn zoom_is_clamped() {
    let mut h = Harness::new();
    let id = h.new_doc(100, 100);
    for _ in 0..30 {
        h.frame(vec![Event::Zoom(4.0)]);
    }
    assert_eq!(h.view(id).zoom, MAX_ZOOM);
    for _ in 0..40 {
        h.frame(vec![Event::Zoom(0.25)]);
    }
    assert_eq!(h.view(id).zoom, 0.01);
}

#[test]
fn scroll_pans_without_reupload() {
    let mut h = Harness::new();
    let id = h.new_doc(4000, 4000);
    let uploads = h.uploads(id);
    let offset = h.view(id).offset;
    h.frame(vec![Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: Vec2::new(0.0, -120.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    }]);
    h.frames(30); // smooth scrolling settles over several frames
    let moved = h.view(id).offset - offset;
    assert!(moved.y < -100.0, "{moved:?}");
    assert_eq!(moved.x, 0.0);
    assert_eq!(h.uploads(id), uploads);
}

#[test]
fn space_drag_pans() {
    let mut h = Harness::new();
    let id = h.new_doc(4000, 4000);
    let uploads = h.uploads(id);
    let start = h.view(id).offset;

    let space = h.key(Key::Space, true);
    h.frame(vec![space]);
    let press = Event::PointerButton {
        pos: h.pointer,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    };
    h.frame(vec![press]);
    for _ in 0..5 {
        h.pointer += Vec2::new(20.0, 10.0);
        h.frame(vec![]);
    }
    let release = Event::PointerButton {
        pos: h.pointer,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    };
    let space_up = h.key(Key::Space, false);
    h.frame(vec![release, space_up]);

    let moved = h.view(id).offset - start;
    assert!(moved.x > 50.0 && moved.y > 25.0, "{moved:?}");
    assert_eq!(h.uploads(id), uploads);
}

#[test]
fn plain_drag_without_space_does_not_pan() {
    let mut h = Harness::new();
    let id = h.new_doc(4000, 4000);
    let start = h.view(id).offset;
    let press = Event::PointerButton {
        pos: h.pointer,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    };
    h.frame(vec![press]);
    for _ in 0..5 {
        h.pointer += Vec2::new(20.0, 0.0);
        h.frame(vec![]);
    }
    assert_eq!(h.view(id).offset, start);
}

#[test]
fn middle_drag_pans() {
    let mut h = Harness::new();
    let id = h.new_doc(4000, 4000);
    let start = h.view(id).offset;
    let press = Event::PointerButton {
        pos: h.pointer,
        button: PointerButton::Middle,
        pressed: true,
        modifiers: Modifiers::NONE,
    };
    h.frame(vec![press]);
    for _ in 0..4 {
        h.pointer += Vec2::new(-15.0, 0.0);
        h.frame(vec![]);
    }
    assert!((h.view(id).offset - start).x < -30.0);
}

#[test]
fn view_shortcuts_fit_actual_and_step() {
    let mut h = Harness::new();
    let id = h.new_doc(4000, 4000);
    let fit = h.view(id).zoom;

    h.shortcut(Modifiers::COMMAND, Key::Num1);
    assert_eq!(h.view(id).zoom, 1.0);

    h.shortcut(Modifiers::COMMAND, Key::Equals);
    assert!((h.view(id).zoom - ZOOM_STEP).abs() < 1e-5);
    h.shortcut(Modifiers::COMMAND, Key::Minus);
    h.shortcut(Modifiers::COMMAND, Key::Minus);
    assert!((h.view(id).zoom - 1.0 / ZOOM_STEP).abs() < 1e-5);

    h.shortcut(Modifiers::COMMAND, Key::Num0);
    assert!((h.view(id).zoom - fit).abs() < 1e-5);
    // The whole UI did not zoom.
    assert_eq!(h.ctx.zoom_factor(), 1.0);
}

#[test]
fn status_bar_shows_zoom_and_pixel_under_pointer() {
    let mut h = Harness::new();
    let id = h.new_doc(4000, 4000);
    h.app.view_action(ViewAction::ActualSize);
    h.frames(2);
    let text = h.app.status_text();
    assert!(text.contains("100%"), "{text}");
    let (x, y) = h
        .app
        .canvas(id)
        .unwrap()
        .hover_pixel()
        .expect("pointer over image");
    assert!(text.ends_with(&format!("{x}, {y}")), "{text}");
    // Moving one point right at 100% (ppp 1) moves one pixel.
    h.pointer.x += 1.0;
    h.frame(vec![]);
    assert_eq!(h.app.canvas(id).unwrap().hover_pixel(), Some((x + 1, y)));
}

#[test]
fn fill_layer_dialog_fills_active_layer_and_undo_redo_via_shortcuts() {
    let mut h = Harness::new();
    let id = h.new_doc(300, 300);
    let uploads = h.uploads(id);
    let original = h.app.state.document(id).unwrap().document.clone();

    h.app.open_fill_dialog();
    assert_eq!(h.app.fill_dialog_color(), Some([0, 0, 0, 255]));
    h.frame(vec![]); // dialog renders
    h.app.set_fill_dialog_color([200, 10, 20, 255]);
    assert!(h.app.submit_fill_dialog());
    assert_eq!(h.app.fill_dialog_color(), None);
    h.frame(vec![]);
    let doc = &h.app.state.document(id).unwrap().document;
    assert!(doc.composite().pixels().all(|p| p.0 == [200, 10, 20, 255]));
    // 300×300 → 2×2 tiles re-uploaded.
    assert_eq!(h.uploads(id), uploads + 4);
    assert_eq!(
        h.app.state.document(id).unwrap().undo_stack().undo_label(),
        Some("Fill Layer")
    );

    h.shortcut(Modifiers::COMMAND, Key::Z);
    assert_eq!(h.app.state.document(id).unwrap().document, original);
    assert_eq!(h.uploads(id), uploads + 8);

    h.shortcut(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert!(h
        .app
        .state
        .document(id)
        .unwrap()
        .document
        .composite()
        .pixels()
        .all(|p| p.0 == [200, 10, 20, 255]));
    h.shortcut(Modifiers::COMMAND, Key::Z);
    h.shortcut(Modifiers::COMMAND, Key::Y);
    assert!(h.app.state.document(id).unwrap().undo_stack().can_undo());
    assert!(!h.app.state.document(id).unwrap().undo_stack().can_redo());

    // The dialog remembers the last color; Cancel changes nothing.
    h.app.open_fill_dialog();
    assert_eq!(h.app.fill_dialog_color(), Some([200, 10, 20, 255]));
    h.app.cancel_fill_dialog();
    assert_eq!(h.app.state.document(id).unwrap().undo_stack().undo_len(), 1);
}

#[test]
fn fill_targets_selected_active_layer() {
    let mut h = Harness::new();
    let id = h.new_doc(4, 4);
    h.app
        .state
        .execute(
            id,
            Box::new(SetPixel {
                x: 0,
                y: 0,
                value: [1; 4],
                old: None,
            }),
        )
        .unwrap();
    // Single-layer document: active layer 0.
    assert_eq!(h.app.state.document(id).unwrap().active_layer(), 0);
    h.app.open_fill_dialog();
    h.app.set_fill_dialog_color([9, 9, 9, 9]);
    h.app.submit_fill_dialog();
    let doc = &h.app.state.document(id).unwrap().document;
    assert!(doc.layer(0).unwrap().pixels.pixels().all(|p| p.0 == [9; 4]));
}

#[test]
fn edits_reupload_only_dirty_tiles() {
    let mut h = Harness::new();
    let id = h.new_doc(4000, 4000);
    let uploads = h.uploads(id);
    h.app
        .state
        .execute(
            id,
            Box::new(SetPixel {
                x: 1000,
                y: 3000,
                value: [255; 4],
                old: None,
            }),
        )
        .unwrap();
    h.frame(vec![]);
    assert_eq!(h.uploads(id), uploads + 1);
    assert!(h.app.undo_current());
    h.frame(vec![]);
    assert_eq!(h.uploads(id), uploads + 2);
    // A pixel on a tile corner touches only its own tile.
    h.app
        .state
        .execute(
            id,
            Box::new(SetPixel {
                x: 255,
                y: 255,
                value: [255; 4],
                old: None,
            }),
        )
        .unwrap();
    h.frame(vec![]);
    assert_eq!(h.uploads(id), uploads + 3);
}

#[test]
fn undo_shortcut_ignored_while_typing_but_works_after_space_focus() {
    let mut h = Harness::new();
    let id = h.new_doc(8, 8);
    h.app.open_fill_dialog();
    h.app.submit_fill_dialog();

    // Space over the canvas takes focus; shortcuts must still work.
    let space = h.key(Key::Space, true);
    let space_up = h.key(Key::Space, false);
    h.frame(vec![space, space_up]);
    h.shortcut(Modifiers::COMMAND, Key::Z);
    assert!(!h.app.state.document(id).unwrap().undo_stack().can_undo());
    h.shortcut(Modifiers::COMMAND, Key::Y);
    assert!(h.app.state.document(id).unwrap().undo_stack().can_undo());

    // With a text field focused, Cmd+Z belongs to the text field.
    h.app.show_new_dialog();
    h.frame(vec![]);
    for _ in 0..20 {
        if h.ctx.text_edit_focused() {
            break;
        }
        let tab = h.key(Key::Tab, true);
        h.frame(vec![tab]);
    }
    assert!(h.ctx.text_edit_focused(), "could not focus a text field");
    h.shortcut(Modifiers::COMMAND, Key::Z);
    assert!(h.app.state.document(id).unwrap().undo_stack().can_undo());
}

fn count_line_segments(output: &egui::FullOutput) -> usize {
    output
        .shapes
        .iter()
        .filter(|s| matches!(s.shape, egui::Shape::LineSegment { .. }))
        .count()
}

#[test]
fn pixel_grid_only_above_8x_and_toggleable() {
    let mut h = Harness::new();
    let id = h.new_doc(64, 64);
    h.app.view_action(ViewAction::ActualSize);
    h.frames(2);
    let baseline = count_line_segments(&h.frame(vec![]));

    // Exactly 8× (three Zoom In steps from 100% = 2.83×; use pinch to 8×).
    h.frame(vec![Event::Zoom(8.0)]);
    assert!((h.view(id).zoom - 8.0).abs() < 1e-4);
    let at_8 = count_line_segments(&h.frame(vec![]));
    assert_eq!(at_8, baseline, "no grid at exactly 8×");

    h.frame(vec![Event::Zoom(1.5)]);
    let above = count_line_segments(&h.frame(vec![]));
    assert!(
        above > baseline + 20,
        "grid lines drawn: {above} vs {baseline}"
    );

    h.app.set_show_pixel_grid(false);
    let off = count_line_segments(&h.frame(vec![]));
    assert_eq!(off, baseline);
}

#[test]
fn closing_document_drops_canvas_and_history() {
    let mut h = Harness::new();
    let id = h.new_doc(16, 16);
    h.app.open_fill_dialog();
    h.app.submit_fill_dialog();
    h.app.close_document(id);
    h.frame(vec![]);
    assert!(h.app.canvas(id).is_none());
    assert!(h.app.state.document(id).is_none());
}

/// Release-mode benchmark: run with
/// `cargo test --release --test canvas -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_pan_zoom_4000() {
    let mut h = Harness::new();
    let id = h.new_doc(4000, 4000);
    let uploads = h.uploads(id);
    let frames = 120;
    let start = std::time::Instant::now();
    for i in 0..frames {
        let event = if i % 2 == 0 {
            Event::Zoom(if i % 4 == 0 { 1.03 } else { 0.98 })
        } else {
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: Vec2::new(3.0, -2.0),
                modifiers: Modifiers::NONE,
                phase: egui::TouchPhase::Move,
            }
        };
        let output = h.frame(vec![event]);
        // Include tessellation, as the real app does every frame.
        let _ = h.ctx.tessellate(output.shapes, output.pixels_per_point);
    }
    let per_frame = start.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    println!("pan/zoom frame on 4000x4000: {per_frame:.3} ms");
    assert_eq!(h.uploads(id), uploads);
    if !cfg!(debug_assertions) {
        assert!(per_frame < 8.0, "{per_frame:.3} ms per frame");
    }
}

fn count_meshes(output: &egui::FullOutput) -> usize {
    output
        .shapes
        .iter()
        .filter(|s| matches!(s.shape, egui::Shape::Mesh(_)))
        .count()
}

#[test]
fn minified_view_draws_from_mip_pyramid() {
    let mut h = Harness::new();
    let id = h.new_doc(4000, 4000);
    let zoom = h.view(id).zoom;
    assert!((0.125..0.25).contains(&zoom), "fit zoom {zoom}");
    // Level 2 (1000×1000) has 4×4 tiles; full resolution would need 256.
    let meshes = count_meshes(&h.frame(vec![]));
    assert!(meshes <= 16 + 4, "{meshes} meshes drawn at fit");
    // At 100% only the visible full-resolution tiles are drawn.
    h.app.view_action(ViewAction::ActualSize);
    h.frame(vec![]);
    let meshes = count_meshes(&h.frame(vec![]));
    assert!(meshes < 40, "{meshes} meshes drawn at 100%");
}
