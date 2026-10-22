//! Parts Layout ▸ Enhance Lines…: the window's own drawn areas, a live
//! preview of them, and Apply on a worker (see [`crate::enhance`]).

use std::sync::mpsc;
use std::sync::Arc;

use egui::{TextureHandle, TextureOptions};
use image::{GrayImage, RgbaImage};

use super::{fill_polygon, JobKind, PartsAction, PartsTab, RecolorEvent, RecolorJob};
use crate::enhance::{self, Settings};
use crate::pdf::mm_to_px;

/// Largest enhanced region (pixels) a preview works on; bigger areas are
/// previewed at a reduced scale.
const PREVIEW_MAX_PX: f32 = 1_000_000.0;
/// Smallest per-page preview budget (pixels), however many pages have areas.
const PREVIEW_MIN_PX: f32 = 250_000.0;

/// The Enhance Lines… window's state.
#[derive(Debug, Clone, PartialEq)]
pub struct EnhanceDialog {
    pub settings: Settings,
    /// Drags on pages draw areas (rectangle; Shift: lasso).
    pub area_mode: bool,
}

/// One page's enhanced region: where it is (page pixels), the scale it was
/// enhanced at, the result, and which of its pixels are inside the areas.
struct Region {
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    image: RgbaImage,
    allowed: GrayImage,
}

/// The page rectangle (x0, y0, w, h) an enhancement of `polys` reads:
/// their bounding box plus the settings' margin.
fn region_rect(
    page: &RgbaImage,
    polys: &[&Vec<(f32, f32)>],
    settings: &Settings,
) -> Option<(u32, u32, u32, u32)> {
    let margin = settings.margin_px(enhance::px_per_mm(crate::pdf::DPI)) as f32;
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in polys.iter().flat_map(|p| p.iter()) {
        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
    }
    let clamp = |v: f32, max: u32| v.clamp(0.0, max as f32) as u32;
    let (x0, y0) = (
        clamp(x0 - margin, page.width()),
        clamp(y0 - margin, page.height()),
    );
    let (x1, y1) = (
        clamp(x1 + margin + 1.0, page.width()),
        clamp(y1 + margin + 1.0, page.height()),
    );
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some((x0, y0, x1 - x0, y1 - y0))
}

/// Enhances the part of `page` inside `polys` (page pixels) at `scale` (≤ 1).
fn enhance_region(
    page: &RgbaImage,
    polys: &[&Vec<(f32, f32)>],
    settings: &Settings,
    scale: f32,
) -> Option<Region> {
    let ppm = enhance::px_per_mm(crate::pdf::DPI);
    let (x0, y0, w, h) = region_rect(page, polys, settings)?;
    let view = image::imageops::crop_imm(page, x0, y0, w, h);
    let scale = scale.min(1.0);
    let crop = if scale < 1.0 {
        // Shrunk straight from the page (no full-size copy).
        let (sw, sh) = (
            ((w as f32 * scale).round() as u32).max(1),
            ((h as f32 * scale).round() as u32).max(1),
        );
        image::imageops::thumbnail(&*view, sw, sh)
    } else {
        view.to_image()
    };
    let mut allowed = GrayImage::new(crop.width(), crop.height());
    for poly in polys {
        let local: Vec<(f32, f32)> = poly
            .iter()
            .map(|&(x, y)| ((x - x0 as f32) * scale, (y - y0 as f32) * scale))
            .collect();
        fill_polygon(&mut allowed, &local);
    }
    let image = enhance::enhance(&crop, settings, ppm * scale, Some(&allowed));
    Some(Region {
        x0,
        y0,
        w,
        h,
        image,
        allowed,
    })
}

impl PartsTab {
    /// Enhance Lines…: opens the window (drawing areas) and closes Recolor….
    pub fn open_enhance(&mut self) {
        if self.pages.is_empty() {
            return;
        }
        self.recolor = None;
        self.recolor_textures.clear();
        self.enhance = Some(EnhanceDialog {
            settings: Settings::default(),
            area_mode: true,
        });
        self.enhance_textures.clear();
    }

    /// Marks the enhance preview as changed.
    pub fn enhance_changed(&mut self) {
        self.enhance_textures.clear();
        self.enhance_applied = false;
    }

    /// Whether the page view shows the enhancement preview (window open,
    /// areas drawn, something to do, and not just applied).
    pub fn enhance_previewing(&self) -> bool {
        self.enhance
            .as_ref()
            .is_some_and(|d| !d.settings.is_identity())
            && !self.enhance_areas.is_empty()
            && !self.enhance_applied
    }

    fn enhance_polys(&self, page: usize) -> Vec<&Vec<(f32, f32)>> {
        self.enhance_areas
            .iter()
            .filter(|(p, _)| *p == page)
            .map(|(_, a)| a)
            .collect()
    }

    /// Adds a drawn Enhance Lines area (page pixels); tiny ones (under 2 mm
    /// across) are ignored.
    pub fn add_enhance_area(&mut self, page: usize, poly: Vec<(f32, f32)>) -> bool {
        if page >= self.pages.len() || poly.len() < 3 {
            return false;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in &poly {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        if (x1 - x0).min(y1 - y0) < mm_to_px(2.0) {
            return false;
        }
        self.enhance_areas.push((page, poly));
        self.enhance_changed();
        true
    }

    /// Clear Areas (Enhance Lines).
    pub fn clear_enhance_areas(&mut self) {
        self.enhance_areas.clear();
        self.enhance_changed();
    }

    /// The page preview with its areas enhanced, while the window is open.
    pub(super) fn enhance_texture(
        &mut self,
        ctx: &egui::Context,
        i: usize,
    ) -> Option<TextureHandle> {
        let settings = self.enhance.as_ref()?.settings;
        if !self.enhance_previewing() {
            return None;
        }
        if self.enhance_polys(i).is_empty() {
            return None;
        }
        if self.enhance_textures.len() < self.pages.len() {
            self.enhance_textures.resize(self.pages.len(), None);
        }
        if self.enhance_textures[i].is_none() {
            let c = self.enhanced_preview(i, &settings)?;
            self.enhance_textures[i] =
                Some(ctx.load_texture(format!("pdf-enhance-{i}"), c, TextureOptions::LINEAR));
        }
        self.enhance_textures[i].clone()
    }

    /// Page `i`'s preview image with its areas enhanced (full resolution, or
    /// reduced for big areas, then scaled to the preview).
    fn enhanced_preview(&self, i: usize, settings: &Settings) -> Option<egui::ColorImage> {
        let polys = self.enhance_polys(i);
        let page = &self.pages[i].image;
        let small = self.page_previews.get(i)?;
        // Big areas are enhanced once, at a reduced scale.
        let (_, _, w, h) = region_rect(page, &polys, settings)?;
        let px = w as f32 * h as f32;
        // The budget is shared by the pages with areas (all re-preview at once).
        let mut pages: Vec<usize> = self.enhance_areas.iter().map(|(p, _)| *p).collect();
        pages.sort_unstable();
        pages.dedup();
        let budget = (PREVIEW_MAX_PX / pages.len().max(1) as f32).max(PREVIEW_MIN_PX);
        let scale = (budget / px).sqrt().min(1.0);
        let region = enhance_region(page, &polys, settings, scale)?;
        let sp = small.width() as f32 / page.width() as f32;
        let (tx, ty) = (
            (region.x0 as f32 * sp) as u32,
            (region.y0 as f32 * sp) as u32,
        );
        let tw = ((region.w as f32 * sp).round() as u32).max(1);
        let th = ((region.h as f32 * sp).round() as u32).max(1);
        let image =
            image::imageops::resize(&region.image, tw, th, image::imageops::FilterType::Triangle);
        let allowed = image::imageops::resize(
            &region.allowed,
            tw,
            th,
            image::imageops::FilterType::Nearest,
        );
        let mut out = small.clone();
        for (x, y, p) in image.enumerate_pixels() {
            let (ox, oy) = (tx + x, ty + y);
            if allowed.get_pixel(x, y)[0] > 0 && ox < out.width() && oy < out.height() {
                out.put_pixel(ox, oy, *p);
            }
        }
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [out.width() as usize, out.height() as usize],
            out.as_raw(),
        ))
    }

    /// Apply: enhances every page's areas at full resolution on a worker.
    pub fn start_enhance(&mut self, notify: Arc<dyn Fn() + Send + Sync>) -> bool {
        let Some(dialog) = &self.enhance else {
            return false;
        };
        if self.page_job.is_some() || self.enhance_areas.is_empty() || dialog.settings.is_identity()
        {
            return false;
        }
        let settings = dialog.settings;
        let pages: Vec<RgbaImage> = self.pages.iter().map(|p| p.image.clone()).collect();
        let areas: Vec<Vec<Vec<(f32, f32)>>> = (0..pages.len())
            .map(|i| self.enhance_polys(i).into_iter().cloned().collect())
            .collect();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = cancel.clone();
        let total = pages.len();
        std::thread::Builder::new()
            .name("enhance".to_owned())
            .spawn(move || {
                let mut out = Vec::with_capacity(pages.len());
                for (i, mut page) in pages.into_iter().enumerate() {
                    if stop.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = tx.send(RecolorEvent::Finished(None));
                        notify();
                        return;
                    }
                    let polys: Vec<&Vec<(f32, f32)>> = areas[i].iter().collect();
                    if !polys.is_empty() {
                        if let Some(r) = enhance_region(&page, &polys, &settings, 1.0) {
                            image::imageops::replace(&mut page, &r.image, r.x0 as i64, r.y0 as i64);
                        }
                    }
                    out.push(page);
                    let _ = tx.send(RecolorEvent::Progress(i + 1, total));
                    notify();
                }
                let _ = tx.send(RecolorEvent::Finished(Some(out)));
                notify();
            })
            .expect("spawn enhance");
        self.page_job = Some(RecolorJob {
            rx,
            cancel,
            done: 0,
            total,
        });
        self.page_job_kind = JobKind::Enhance;
        self.status = Some(format!("Enhancing lines on {total} page(s)…"));
        true
    }

    pub(super) fn show_enhance_window(
        &mut self,
        ctx: &egui::Context,
        actions: &mut Vec<PartsAction>,
    ) {
        let Some(mut dialog) = self.enhance.clone() else {
            return;
        };
        let before = dialog.clone();
        let (mut keep, mut apply, mut revert, mut clear, mut cancel) =
            (true, false, false, false, false);
        let areas = self.enhance_areas.len();
        let progress = self.recolor_progress();
        let original = self.recolored();
        egui::Window::new("Enhance Lines")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, egui::Vec2::new(-24.0, 72.0))
            .show(ctx, |ui| {
                ui.label("Darker, joined, and bolder thin dark lines inside the areas:");
                let s = &mut dialog.settings;
                let mut strength = (s.strength * 100.0).round();
                let mut smooth = (s.smooth * 100.0).round();
                egui::Grid::new("enhance-controls").show(ui, |ui| {
                    ui.label("Line strength");
                    ui.add(egui::Slider::new(&mut strength, 0.0..=100.0).suffix(" %"));
                    ui.end_row();
                    ui.label("Close gaps");
                    ui.add(
                        egui::Slider::new(&mut s.gaps_mm, 0.0..=2.0)
                            .step_by(0.1)
                            .suffix(" mm"),
                    );
                    ui.end_row();
                    ui.label("Thickness");
                    ui.add(
                        egui::Slider::new(&mut s.thickness, 1.0..=2.0)
                            .step_by(0.1)
                            .suffix(" ×"),
                    );
                    ui.end_row();
                    ui.label("Smooth grain");
                    ui.add(egui::Slider::new(&mut smooth, 0.0..=100.0).suffix(" %"));
                    ui.end_row();
                });
                s.strength = strength / 100.0;
                s.smooth = smooth / 100.0;
                ui.separator();
                ui.horizontal(|ui| {
                    ui.toggle_value(&mut dialog.area_mode, "Draw Areas")
                        .on_hover_text("Drag on a page: rectangle · Shift + drag: lasso");
                    if ui
                        .add_enabled(areas > 0, egui::Button::new("Clear Areas"))
                        .clicked()
                    {
                        clear = true;
                    }
                });
                if areas == 0 {
                    ui.weak("Draw an area on a page first (drag: rectangle · Shift + drag: lasso)");
                } else {
                    ui.weak(format!(
                        "Enhancing inside {areas} area{} (other pages unchanged)",
                        if areas == 1 { "" } else { "s" }
                    ));
                }
                ui.separator();
                if let Some((done, total)) = progress {
                    ui.add(
                        egui::ProgressBar::new(done as f32 / total.max(1) as f32).text(format!(
                            "Enhancing page {} of {total}",
                            (done + 1).min(total)
                        )),
                    );
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                } else {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                areas > 0 && !dialog.settings.is_identity(),
                                egui::Button::new("Apply"),
                            )
                            .on_disabled_hover_text("Draw an area first")
                            .clicked()
                        {
                            apply = true;
                        }
                        if ui
                            .add_enabled(original, egui::Button::new("Revert to Original"))
                            .clicked()
                        {
                            revert = true;
                        }
                        if ui.button("Close").clicked() {
                            keep = false;
                        }
                    });
                }
            });
        let changed = dialog.settings != before.settings;
        if keep {
            self.enhance = Some(dialog);
        } else {
            self.enhance = None;
            self.enhance_textures.clear();
        }
        if changed {
            self.enhance_changed();
        }
        if clear {
            self.clear_enhance_areas();
        }
        if cancel {
            self.cancel_recolor();
        }
        if revert {
            self.revert_recolor();
        }
        if apply {
            actions.push(PartsAction::ApplyEnhance);
        }
    }
}
