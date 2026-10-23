//! Whole-video extraction UI: the Extract Assets dialog and the Review tab.

use std::collections::{BTreeSet, HashMap};

use egui::{Color32, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};

use crate::extract::detect::DetectParams;
use crate::extract::video::{Cluster, VideoExtractJob, VideoExtractSpec};
use crate::video::{format_timecode, VideoInfo};
use crate::video_ui::VideoId;

/// The Extract Assets dialog of a video tab and its running job.
pub struct VideoAssetsDialog {
    pub video: VideoId,
    pub info: VideoInfo,
    pub from: u64,
    pub to: u64,
    pub every: u64,
    pub detect: DetectParams,
    job: Option<VideoExtractJob>,
    pub error: Option<String>,
}

/// What happened in the dialog this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetsOutcome {
    Open,
    Start,
    Cancel,
}

impl VideoAssetsDialog {
    pub fn new(video: VideoId, info: VideoInfo) -> Self {
        Self {
            video,
            to: info.last_frame(),
            info,
            from: 0,
            every: 1,
            detect: DetectParams::default(),
            job: None,
            error: None,
        }
    }

    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    pub fn progress(&self) -> Option<f32> {
        self.job.as_ref().map(VideoExtractJob::progress)
    }

    /// The running job's "worker exited" flag (see [`VideoExtractJob::exited`]).
    pub fn worker_exited(&self) -> Option<std::sync::Arc<std::sync::atomic::AtomicBool>> {
        self.job.as_ref().map(VideoExtractJob::exited)
    }

    pub fn spec(&self) -> VideoExtractSpec {
        let to = self.to.min(self.info.last_frame());
        VideoExtractSpec {
            path: self.info.path.clone(),
            from: self.from.min(to),
            to,
            every: self.every.clamp(1, 1000),
            detect: self.detect,
        }
    }

    pub fn start(&mut self, notify: std::sync::Arc<dyn Fn() + Send + Sync>) {
        self.error = None;
        self.job = Some(VideoExtractJob::start(self.spec(), notify));
    }

    pub fn cancel_job(&mut self) {
        if let Some(mut job) = self.job.take() {
            job.cancel();
        }
    }

    pub fn take_result(&mut self) -> Option<Result<Vec<Cluster>, String>> {
        let result = self.job.as_ref()?.try_result()?;
        self.job = None;
        Some(result)
    }

    pub fn show(&mut self, ctx: &egui::Context) -> AssetsOutcome {
        let mut outcome = AssetsOutcome::Open;
        let last = self.info.last_frame();
        egui::Window::new("Extract Assets")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(&self.info.name);
                let running = self.job.is_some();
                ui.add_enabled_ui(!running, |ui| {
                    egui::Grid::new("extract_assets")
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.label("From frame");
                            ui.add(egui::DragValue::new(&mut self.from).range(0..=last));
                            ui.end_row();
                            ui.label("To frame");
                            ui.add(egui::DragValue::new(&mut self.to).range(0..=last));
                            ui.end_row();
                            ui.label("Every N frames");
                            ui.add(egui::DragValue::new(&mut self.every).range(1..=1000));
                            ui.end_row();
                            ui.label("Minimum Size");
                            ui.add(
                                egui::DragValue::new(&mut self.detect.min_size)
                                    .range(1..=10_000)
                                    .suffix(" px²"),
                            );
                            ui.end_row();
                            ui.label("Merge Distance");
                            ui.add(
                                egui::DragValue::new(&mut self.detect.merge_distance)
                                    .range(0..=64)
                                    .suffix(" px"),
                            );
                            ui.end_row();
                        });
                });
                if let Some(p) = self.progress() {
                    ui.add(egui::ProgressBar::new(p).show_percentage());
                }
                if let Some(e) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        outcome = AssetsOutcome::Cancel;
                    }
                    if ui
                        .add_enabled(!running, egui::Button::new("Extract"))
                        .clicked()
                    {
                        outcome = AssetsOutcome::Start;
                    }
                });
            });
        if self.to < self.from {
            self.to = self.from;
        }
        outcome
    }
}

/// Identifies a Review tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReviewId(pub u64);

/// A Review tab: the clusters of one run.
pub struct ReviewTab {
    /// The video's file name (source of the assets).
    pub video: String,
    pub fps: f64,
    pub clusters: Vec<Cluster>,
    /// The video file (recorded as the assets' source).
    pub video_path: Option<std::path::PathBuf>,
    pub keep: Vec<bool>,
    pub selected: BTreeSet<usize>,
    textures: HashMap<(usize, usize), TextureHandle>,
}

/// What the user did in a Review tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewAction {
    Merge(Vec<usize>),
    AddKept,
}

const ROW: f32 = 64.0;

impl ReviewTab {
    pub fn new(video: String, fps: f64, clusters: Vec<Cluster>) -> Self {
        Self {
            keep: vec![true; clusters.len()],
            video,
            fps,
            clusters,
            video_path: None,
            selected: BTreeSet::new(),
            textures: HashMap::new(),
        }
    }

    /// Replaces the clusters at `rows` with their merge (placed at the first).
    pub fn merge(&mut self, rows: &[usize]) -> bool {
        let mut rows: Vec<usize> = rows
            .iter()
            .copied()
            .filter(|r| *r < self.clusters.len())
            .collect();
        rows.sort_unstable();
        rows.dedup();
        if rows.len() < 2 {
            return false;
        }
        let parts: Vec<Cluster> = rows.iter().map(|&r| self.clusters[r].clone()).collect();
        let keep = rows.iter().any(|&r| self.keep[r]);
        for &r in rows.iter().rev() {
            self.clusters.remove(r);
            self.keep.remove(r);
        }
        let at = rows[0].min(self.clusters.len());
        self.clusters.insert(at, Cluster::merge(parts));
        self.keep.insert(at, keep);
        self.selected = [at].into();
        self.textures.clear();
        true
    }

    fn texture(&mut self, ctx: &egui::Context, row: usize, pose: usize) -> TextureHandle {
        let image = &self.clusters[row].poses[pose].image;
        self.textures
            .entry((row, pose))
            .or_insert_with(|| {
                let c = egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                );
                ctx.load_texture(format!("review-{row}-{pose}"), c, TextureOptions::LINEAR)
            })
            .clone()
    }

    pub fn show(&mut self, ui: &mut egui::Ui) -> Vec<ReviewAction> {
        let mut actions = Vec::new();
        let kept = self.keep.iter().filter(|k| **k).count();
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} — {} clusters ({} animations), {kept} kept",
                self.video,
                self.clusters.len(),
                self.clusters.iter().filter(|c| c.is_animation()).count()
            ));
            if ui
                .add_enabled(self.selected.len() >= 2, egui::Button::new("Merge"))
                .clicked()
            {
                actions.push(ReviewAction::Merge(self.selected.iter().copied().collect()));
            }
            if ui
                .add_enabled(kept > 0, egui::Button::new("Add Kept to Library"))
                .clicked()
            {
                actions.push(ReviewAction::AddKept);
            }
        });
        ui.separator();
        let modifiers = ui.input(|i| i.modifiers);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for row in 0..self.clusters.len() {
                let poses = self.clusters[row].poses.len();
                let seen = self.clusters[row].frames.len();
                let first = self.clusters[row].first_frame();
                let selected = self.selected.contains(&row);
                let textures: Vec<(TextureHandle, [u32; 2])> = (0..poses.min(16))
                    .map(|p| {
                        let size = self.clusters[row].poses[p].image.dimensions();
                        (self.texture(ui.ctx(), row, p), [size.0, size.1])
                    })
                    .collect();
                let mut checkbox = Rect::NOTHING;
                let response = ui
                    .horizontal(|ui| {
                        checkbox = ui.checkbox(&mut self.keep[row], "Keep").rect;
                        // Filmstrip (or one thumbnail).
                        let width = (textures.len() as f32 * (ROW + 4.0)).max(ROW);
                        let (strip, _) =
                            ui.allocate_exact_size(Vec2::new(width, ROW), Sense::hover());
                        for (i, (tex, [w, h])) in textures.iter().enumerate() {
                            let cell = Rect::from_min_size(
                                strip.min + Vec2::new(i as f32 * (ROW + 4.0), 0.0),
                                Vec2::splat(ROW),
                            );
                            ui.painter()
                                .rect_filled(cell, 2.0, ui.visuals().extreme_bg_color);
                            let s = (ROW / *w as f32).min(ROW / *h as f32);
                            ui.painter().image(
                                tex.id(),
                                Rect::from_center_size(
                                    cell.center(),
                                    Vec2::new(*w as f32, *h as f32) * s,
                                ),
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        }
                        ui.vertical(|ui| {
                            let text = if poses > 1 {
                                format!("Animation: {poses} poses, seen in {seen} frames")
                            } else {
                                format!("Single: seen in {seen} frames")
                            };
                            ui.label(text);
                            ui.small(format!(
                                "first at frame {first} ({})",
                                format_timecode(first as f64 / self.fps.max(1e-6))
                            ));
                        });
                    })
                    .response;
                // Clicks on the row (not its checkbox) select it.
                let target = Rect::from_min_max(
                    Pos2::new(checkbox.right() + 2.0, response.rect.top()),
                    response.rect.max,
                );
                let click = ui.interact(target, ui.id().with(("review-row", row)), Sense::click());
                if selected {
                    ui.painter().rect_stroke(
                        response.rect.expand(2.0),
                        2.0,
                        Stroke::new(2.0, ui.visuals().selection.stroke.color),
                        egui::StrokeKind::Inside,
                    );
                }
                if click.clicked() {
                    if modifiers.command {
                        if !self.selected.remove(&row) {
                            self.selected.insert(row);
                        }
                    } else if modifiers.shift && !self.selected.is_empty() {
                        let anchor = *self.selected.first().expect("non-empty");
                        self.selected.extend(anchor.min(row)..=anchor.max(row));
                    } else {
                        self.selected = [row].into();
                    }
                }
                ui.separator();
            }
        });
        actions
    }
}
