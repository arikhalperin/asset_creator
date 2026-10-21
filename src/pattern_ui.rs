//! Extract → Find Repeating Pattern…: the detected tile, manual crop on the
//! canvas, a live 3×3 tiled preview, Make Seamless, and Add to Library.

use egui::{Color32, Pos2, Rect, Stroke, TextureHandle, Vec2};
use image::RgbaImage;

use crate::app_state::DocId;
use crate::canvas::ViewState;
use crate::document::PixelRect;
use crate::pattern::{self, Detection};

/// Largest side of the 3×3 preview on screen (points).
const PREVIEW_SIZE: f32 = 240.0;
/// Distance (physical screen pixels) within which an edge or corner is grabbed.
const GRAB: f32 = 8.0;

/// What the user did in the pattern window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternOutcome {
    Open,
    /// Re-run detection on the current selection or layer.
    Detect,
    Add,
    Close,
}

/// Which part of the tile a manual-crop drag holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Grip {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

impl Grip {
    fn is_move(self) -> bool {
        !(self.left || self.right || self.top || self.bottom)
    }
}

#[derive(Debug, Clone, Copy)]
enum Drag {
    /// Moving or resizing: the grip, the press point, the tile at the press.
    Edit(Grip, (f32, f32), PixelRect),
    /// Drawing a new tile from this corner.
    New((f32, f32)),
}

/// The open pattern window on one document.
pub struct PatternDialog {
    pub doc: DocId,
    /// The active layer when the window opened (or Detect was last run).
    pub layer: RgbaImage,
    /// The analyzed region (selection bounds or the whole layer).
    pub region: PixelRect,
    pub detection: Detection,
    /// The tile in layer coordinates.
    pub tile: PixelRect,
    /// Manual crop: canvas drags move, resize, or redraw the tile.
    pub manual: bool,
    /// Make Seamless blend width, percent (5–50).
    pub blend: u32,
    /// The current tile made seamless (cleared when the tile changes).
    pub seamless: Option<RgbaImage>,
    drag: Option<Drag>,
    texture: Option<(PixelRect, bool, TextureHandle)>,
}

impl PatternDialog {
    /// Analyzes `region` of `layer`.
    pub fn new(doc: DocId, layer: RgbaImage, region: PixelRect) -> Self {
        let mut dialog = Self {
            doc,
            layer,
            region,
            detection: Detection {
                found: false,
                period: (region.width, region.height),
                confidence: 0.0,
                tile: PixelRect::new(0, 0, region.width, region.height),
            },
            tile: region,
            manual: false,
            blend: (pattern::DEFAULT_BLEND * 100.0).round() as u32,
            seamless: None,
            drag: None,
            texture: None,
        };
        dialog.detect();
        dialog
    }

    /// Runs detection on the region and proposes its tile.
    pub fn detect(&mut self) {
        let r = self.region;
        let crop = image::imageops::crop_imm(&self.layer, r.x, r.y, r.width, r.height).to_image();
        self.detection = pattern::detect(&crop);
        let t = self.detection.tile;
        self.set_tile(PixelRect::new(r.x + t.x, r.y + t.y, t.width, t.height));
    }

    /// Sets the tile (clamped to the layer, at least 1 × 1); a changed tile
    /// drops the seamless version.
    pub fn set_tile(&mut self, tile: PixelRect) {
        let (w, h) = self.layer.dimensions();
        let x = tile.x.min(w.saturating_sub(1));
        let y = tile.y.min(h.saturating_sub(1));
        let clamped = PixelRect::new(
            x,
            y,
            tile.width.clamp(1, w - x),
            tile.height.clamp(1, h - y),
        );
        if clamped != self.tile {
            self.tile = clamped;
            self.seamless = None;
        }
    }

    /// The tile's pixels as cut from the layer.
    pub fn raw_tile(&self) -> RgbaImage {
        let t = self.tile;
        image::imageops::crop_imm(&self.layer, t.x, t.y, t.width, t.height).to_image()
    }

    /// The tile that Add to Library saves (the seamless version if made).
    pub fn tile_image(&self) -> RgbaImage {
        self.seamless.clone().unwrap_or_else(|| self.raw_tile())
    }

    /// The 3×3 tiled preview image.
    pub fn preview(&self) -> RgbaImage {
        pattern::repeat(&self.tile_image(), 3, 3)
    }

    pub fn make_seamless(&mut self) {
        self.seamless = Some(pattern::make_seamless(
            &self.raw_tile(),
            self.blend as f32 / 100.0,
        ));
    }

    pub fn revert(&mut self) {
        self.seamless = None;
    }

    /// Extraction parameters recorded with a saved tile.
    pub fn params(&self) -> serde_json::Value {
        let t = self.tile;
        serde_json::json!({
            "period": [self.detection.period.0, self.detection.period.1],
            "found": self.detection.found,
            "confidence": self.detection.confidence,
            "region": [self.region.x, self.region.y, self.region.width, self.region.height],
            "tile": [t.x, t.y, t.width, t.height],
            "seamless": self.seamless.is_some(),
            "blend": self.blend,
        })
    }

    // --- Manual crop --------------------------------------------------------

    /// A canvas press at `point` (image coordinates); `grab` is the edge
    /// tolerance in image pixels.
    pub fn drag_begin(&mut self, point: (f32, f32), grab: f32) {
        let t = self.tile;
        // Small tiles keep a middle area for moving: edges grab at most a
        // quarter of the tile on each axis.
        let gx = grab.min(t.width as f32 / 4.0);
        let gy = grab.min(t.height as f32 / 4.0);
        let (l, r) = (t.x as f32, t.right() as f32);
        let (top, b) = (t.y as f32, t.bottom() as f32);
        let (x, y) = point;
        let inside_x = x >= l - gx && x <= r + gx;
        let inside_y = y >= top - gy && y <= b + gy;
        if !(inside_x && inside_y) {
            self.drag = Some(Drag::New(point));
            return;
        }
        let near = |a: f32, b: f32, g: f32| (a - b).abs() <= g;
        let mut grip = Grip {
            left: near(x, l, gx),
            right: near(x, r, gx),
            top: near(y, top, gy),
            bottom: near(y, b, gy),
        };
        // A tiny tile: prefer the nearer edge.
        if grip.left && grip.right {
            grip.left = (x - l).abs() <= (x - r).abs();
            grip.right = !grip.left;
        }
        if grip.top && grip.bottom {
            grip.top = (y - top).abs() <= (y - b).abs();
            grip.bottom = !grip.top;
        }
        self.drag = Some(Drag::Edit(grip, point, t));
    }

    pub fn drag_to(&mut self, point: (f32, f32)) {
        let (w, h) = (self.layer.width() as f32, self.layer.height() as f32);
        let clamp = |p: (f32, f32)| (p.0.clamp(0.0, w), p.1.clamp(0.0, h));
        let rect_of = |a: (f32, f32), b: (f32, f32)| {
            let (x0, x1) = (a.0.min(b.0).round(), a.0.max(b.0).round());
            let (y0, y1) = (a.1.min(b.1).round(), a.1.max(b.1).round());
            PixelRect::new(
                x0 as u32,
                y0 as u32,
                ((x1 - x0) as u32).max(1),
                ((y1 - y0) as u32).max(1),
            )
        };
        match self.drag {
            Some(Drag::New(start)) => self.set_tile(rect_of(clamp(start), clamp(point))),
            Some(Drag::Edit(grip, start, t)) => {
                let dx = (point.0 - start.0).round();
                let dy = (point.1 - start.1).round();
                if grip.is_move() {
                    let x = (t.x as f32 + dx).clamp(0.0, w - t.width as f32);
                    let y = (t.y as f32 + dy).clamp(0.0, h - t.height as f32);
                    self.set_tile(PixelRect::new(x as u32, y as u32, t.width, t.height));
                } else {
                    let mut l = t.x as f32;
                    let mut r = t.right() as f32;
                    let mut top = t.y as f32;
                    let mut b = t.bottom() as f32;
                    if grip.left {
                        l = (l + dx).clamp(0.0, r - 1.0);
                    }
                    if grip.right {
                        r = (r + dx).clamp(l + 1.0, w);
                    }
                    if grip.top {
                        top = (top + dy).clamp(0.0, b - 1.0);
                    }
                    if grip.bottom {
                        b = (b + dy).clamp(top + 1.0, h);
                    }
                    self.set_tile(rect_of((l, top), (r, b)));
                }
            }
            None => {}
        }
    }

    pub fn drag_end(&mut self) {
        self.drag = None;
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// What the preview on screen shows: the tile, whether seamless, and the
    /// preview texture's size (set when the window is drawn).
    pub fn shown_preview(&self) -> Option<(PixelRect, bool, [usize; 2])> {
        self.texture
            .as_ref()
            .map(|(t, s, tex)| (*t, *s, tex.size()))
    }

    // --- Window -------------------------------------------------------------

    fn preview_texture(&mut self, ctx: &egui::Context) -> TextureHandle {
        let key = (self.tile, self.seamless.is_some());
        if let Some((tile, seamless, texture)) = &self.texture {
            if (*tile, *seamless) == key {
                return texture.clone();
            }
        }
        let img = self.preview();
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [img.width() as usize, img.height() as usize],
            img.as_raw(),
        );
        let texture = ctx.load_texture("pattern-preview", color, egui::TextureOptions::NEAREST);
        self.texture = Some((key.0, key.1, texture.clone()));
        texture
    }

    pub fn show(&mut self, ctx: &egui::Context) -> PatternOutcome {
        let mut outcome = PatternOutcome::Open;
        let mut tile = self.tile;
        let mut blend = self.blend;
        let texture = self.preview_texture(ctx);
        let (lw, lh) = self.layer.dimensions();
        egui::Window::new("Find Repeating Pattern")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-24.0, 72.0))
            .show(ctx, |ui| {
                let d = &self.detection;
                if d.found {
                    ui.label(format!(
                        "Period {} × {} px   (confidence {:.0} %)",
                        d.period.0,
                        d.period.1,
                        d.confidence * 100.0
                    ));
                } else {
                    ui.label("No repeating pattern found");
                }
                ui.weak(format!(
                    "Region {} × {} at {}, {}",
                    self.region.width, self.region.height, self.region.x, self.region.y
                ));
                ui.separator();
                egui::Grid::new("pattern_tile")
                    .num_columns(4)
                    .show(ui, |ui| {
                        ui.label("X");
                        ui.add(egui::DragValue::new(&mut tile.x).range(0..=lw - 1));
                        ui.label("Y");
                        ui.add(egui::DragValue::new(&mut tile.y).range(0..=lh - 1));
                        ui.end_row();
                        ui.label("W");
                        ui.add(egui::DragValue::new(&mut tile.width).range(1..=lw));
                        ui.label("H");
                        ui.add(egui::DragValue::new(&mut tile.height).range(1..=lh));
                        ui.end_row();
                    });
                ui.checkbox(&mut self.manual, "Manual crop (drag on the canvas)");
                ui.separator();
                let size = texture.size_vec2();
                let scale = (PREVIEW_SIZE / size.x.max(size.y)).min(4.0);
                ui.add(egui::Image::new((texture.id(), size * scale)));
                ui.weak("3 × 3 tiled preview");
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Blend width");
                    ui.add(egui::Slider::new(&mut blend, 5..=50).suffix(" %"));
                });
                ui.horizontal(|ui| {
                    if ui.button("Make Seamless").clicked() {
                        self.blend = blend;
                        self.make_seamless();
                    }
                    if ui
                        .add_enabled(self.seamless.is_some(), egui::Button::new("Revert"))
                        .clicked()
                    {
                        self.revert();
                    }
                    if self.seamless.is_some() {
                        ui.weak("seamless");
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Close").clicked() {
                        outcome = PatternOutcome::Close;
                    }
                    if ui.button("Detect").clicked() {
                        outcome = PatternOutcome::Detect;
                    }
                    if ui.button("Add to Library").clicked() {
                        outcome = PatternOutcome::Add;
                    }
                });
            });
        self.blend = blend;
        self.set_tile(tile);
        outcome
    }
}

/// Edge tolerance in image pixels at `zoom` (physical pixels per image pixel).
pub fn grab_tolerance(zoom: f32) -> f32 {
    GRAB / zoom.max(1e-3)
}

/// Outlines the analyzed region and the tile on the canvas (with handles in
/// manual crop mode).
pub fn draw_overlay(ui: &egui::Ui, canvas_rect: Rect, view: &ViewState, dialog: &PatternDialog) {
    let ppp = ui.ctx().pixels_per_point();
    let to_screen = |x: u32, y: u32| {
        view.image_to_view(Pos2::new(x as f32, y as f32), ppp) + canvas_rect.min.to_vec2()
    };
    let rect_of =
        |r: PixelRect| Rect::from_min_max(to_screen(r.x, r.y), to_screen(r.right(), r.bottom()));
    let painter = ui.painter_at(canvas_rect);
    painter.rect_stroke(
        rect_of(dialog.region),
        0.0,
        Stroke::new(1.0, Color32::from_white_alpha(140)),
        egui::StrokeKind::Outside,
    );
    let tile = rect_of(dialog.tile);
    let color = Color32::from_rgb(255, 200, 0);
    painter.rect_stroke(
        tile,
        0.0,
        Stroke::new(2.0, color),
        egui::StrokeKind::Outside,
    );
    if dialog.manual {
        for c in [
            tile.left_top(),
            tile.right_top(),
            tile.left_bottom(),
            tile.right_bottom(),
            tile.center_top(),
            tile.center_bottom(),
            tile.left_center(),
            tile.right_center(),
        ] {
            painter.rect_filled(Rect::from_center_size(c, Vec2::splat(7.0)), 0.0, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn layer() -> RgbaImage {
        RgbaImage::from_fn(200, 120, |x, y| {
            let row = (y / 10) % 2;
            let xx = (x + row * 12) % 24;
            if y % 10 < 2 || xx < 2 {
                Rgba([210, 205, 200, 255])
            } else {
                Rgba([140 + (xx * 3) as u8, 60 + (y % 10) as u8 * 4, 50, 255])
            }
        })
    }

    #[test]
    fn manual_crop_moves_resizes_and_redraws() {
        let mut d = PatternDialog::new(DocId(1), layer(), PixelRect::new(0, 0, 200, 120));
        assert_eq!(d.detection.period, (24, 20));
        d.set_tile(PixelRect::new(50, 40, 30, 20));
        // Move from inside.
        d.drag_begin((60.0, 50.0), 2.0);
        d.drag_to((70.0, 45.0));
        d.drag_end();
        assert_eq!(d.tile, PixelRect::new(60, 35, 30, 20));
        // Resize the right edge.
        d.drag_begin((90.0, 45.0), 2.0);
        d.drag_to((100.0, 45.0));
        d.drag_end();
        assert_eq!(d.tile, PixelRect::new(60, 35, 40, 20));
        // Bottom-left corner.
        d.drag_begin((60.0, 55.0), 2.0);
        d.drag_to((55.0, 65.0));
        d.drag_end();
        assert_eq!(d.tile, PixelRect::new(55, 35, 45, 30));
        // Outside: a new tile, clamped to the layer.
        d.drag_begin((150.0, 100.0), 2.0);
        d.drag_to((260.0, 80.0));
        d.drag_end();
        assert_eq!(d.tile, PixelRect::new(150, 80, 50, 20));
        // A 16 px tile with a large grab tolerance still moves from inside.
        d.drag_end();
        d.set_tile(PixelRect::new(20, 20, 16, 16));
        d.drag_begin((28.0, 28.0), 8.0);
        d.drag_to((30.0, 31.0));
        d.drag_end();
        assert_eq!(d.tile, PixelRect::new(22, 23, 16, 16));
        d.set_tile(PixelRect::new(150, 80, 50, 20));
        // Moving never leaves the layer.
        d.drag_begin((170.0, 90.0), 2.0);
        d.drag_to((400.0, 400.0));
        assert_eq!(d.tile, PixelRect::new(150, 100, 50, 20));
    }

    #[test]
    fn seamless_version_is_dropped_when_the_tile_changes() {
        let mut d = PatternDialog::new(DocId(1), layer(), PixelRect::new(0, 0, 200, 120));
        d.set_tile(PixelRect::new(3, 3, 50, 37));
        d.make_seamless();
        assert!(d.seamless.is_some());
        assert!(pattern::seam_ratio(&d.tile_image()) <= pattern::SEAMLESS_RATIO);
        assert_eq!(d.preview().dimensions(), (150, 111));
        d.set_tile(PixelRect::new(4, 3, 50, 37));
        assert!(d.seamless.is_none());
        d.set_tile(PixelRect::new(190, 3, 50, 500));
        assert_eq!(d.tile, PixelRect::new(190, 3, 10, 117));
    }
}
