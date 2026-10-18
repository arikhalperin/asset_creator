//! Video tabs, the Extract Frames dialog, and the Frame Set panel.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use egui::{Color32, Rect, Sense, TextureHandle, TextureOptions, Vec2};
use image::RgbaImage;

use crate::frame_set::{frame_document_name, FrameSet};
use crate::video::extract::{ExtractJob, ExtractMode, ExtractSpec};
use crate::video::player::{Event, Notify, Player, Request, Thumbnails};
use crate::video::{format_timecode, VideoInfo, VideoSource};

/// Identifies an open video tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VideoId(pub u64);

/// Transport commands (buttons and shortcuts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    PlayPause,
    Step(i64),
    Start,
    End,
}

/// What a video tab asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoAction {
    Grab,
    Extract,
}

/// A grabbed frame ready to open as a document.
#[derive(Debug, Clone, PartialEq)]
pub struct GrabbedFrame {
    pub name: String,
    pub image: RgbaImage,
}

/// Height of the thumbnail strip images in pixels.
pub const STRIP_HEIGHT: u32 = 48;
const STRIP_GAP: f32 = 4.0;

/// The frame currently displayed in a video tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Displayed {
    pub index: u64,
    /// False for a keyframe preview while scrubbing.
    pub exact: bool,
}

struct Strip {
    /// Frame each slot asked for.
    targets: Vec<u64>,
    /// Decoded thumbnails: actual frame number and texture.
    slots: Vec<Option<(u64, TextureHandle)>>,
    worker: Option<Thumbnails>,
}

/// Where the last frame drew things (for tests and hit-testing).
#[derive(Debug, Clone, PartialEq)]
pub struct VideoLayout {
    pub frame: Rect,
    pub timeline: Rect,
    pub thumbnails: Vec<Rect>,
}

impl Default for VideoLayout {
    fn default() -> Self {
        Self {
            frame: Rect::NOTHING,
            timeline: Rect::NOTHING,
            thumbnails: Vec::new(),
        }
    }
}

/// One open video.
pub struct VideoTab {
    info: VideoInfo,
    player: Player,
    notify: Notify,
    texture: Option<TextureHandle>,
    displayed: Option<Displayed>,
    /// The displayed frame's pixels (exact frames only).
    image: Option<RgbaImage>,
    playing: bool,
    /// Timeline position.
    scrub: u64,
    /// The request whose answer is expected (older answers are ignored).
    latest: Option<Request>,
    /// Grab waiting for this exact frame.
    grab: Option<u64>,
    strip: Strip,
    error: Option<String>,
    layout: VideoLayout,
}

impl VideoTab {
    /// Starts the tab's worker on an opened source and shows frame 0.
    pub fn new(source: VideoSource, ctx: Option<&egui::Context>) -> Self {
        let notify: Notify = match ctx {
            Some(ctx) => {
                let ctx = ctx.clone();
                Arc::new(move || ctx.request_repaint())
            }
            None => Arc::new(|| {}),
        };
        let info = source.info().clone();
        let player = Player::new(source, notify.clone());
        let mut tab = Self {
            info,
            player,
            notify,
            texture: None,
            displayed: None,
            image: None,
            playing: false,
            scrub: 0,
            latest: None,
            grab: None,
            strip: Strip {
                targets: Vec::new(),
                slots: Vec::new(),
                worker: None,
            },
            error: None,
            layout: VideoLayout::default(),
        };
        tab.send(Request::Exact(0));
        tab
    }

    pub fn info(&self) -> &VideoInfo {
        &self.info
    }

    pub fn displayed(&self) -> Option<Displayed> {
        self.displayed
    }

    /// The displayed frame's pixels, when it is exact.
    pub fn displayed_image(&self) -> Option<&RgbaImage> {
        self.image.as_ref()
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// The frame the timeline points at.
    pub fn position(&self) -> u64 {
        self.scrub
    }

    /// Whether a request is still being decoded.
    pub fn is_busy(&self) -> bool {
        self.latest.is_some() && !self.playing
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn layout(&self) -> &VideoLayout {
        &self.layout
    }

    /// Actual frame numbers of the thumbnails decoded so far (by slot).
    pub fn thumbnails(&self) -> Vec<Option<u64>> {
        self.strip
            .slots
            .iter()
            .map(|s| s.as_ref().map(|(i, _)| *i))
            .collect()
    }

    fn send(&mut self, request: Request) {
        self.latest = Some(request);
        self.player.request(request);
    }

    /// Scrubbing: show the nearest keyframe at or before `index`.
    pub fn seek_preview(&mut self, index: u64) {
        self.playing = false;
        self.scrub = index.min(self.info.last_frame());
        self.send(Request::Preview(self.scrub));
    }

    /// Shows exactly frame `index`.
    pub fn seek(&mut self, index: u64) {
        self.playing = false;
        self.scrub = index.min(self.info.last_frame());
        self.send(Request::Exact(self.scrub));
    }

    pub fn transport(&mut self, command: Transport) {
        match command {
            Transport::PlayPause => {
                if self.playing {
                    // Pause on the frame showing, decoded exactly.
                    let at = self.displayed.map_or(self.scrub, |d| d.index);
                    self.seek(at);
                } else {
                    let from = if self.scrub >= self.info.last_frame() {
                        0
                    } else {
                        self.scrub
                    };
                    self.scrub = from;
                    self.playing = true;
                    self.send(Request::Play(from));
                }
            }
            Transport::Step(delta) => {
                let base = self.displayed.map_or(self.scrub, |d| d.index) as i64;
                let target = (base + delta).clamp(0, self.info.last_frame() as i64) as u64;
                self.seek(target);
            }
            Transport::Start => self.seek(0),
            Transport::End => self.seek(self.info.last_frame()),
        }
    }

    /// Grab Frame: the exact current frame (decoded first if needed) is
    /// returned by [`Self::poll`].
    pub fn grab(&mut self) {
        // The timeline position (a preview may still show its keyframe);
        // during playback, the frame on screen.
        let at = if self.playing {
            self.displayed.map_or(self.scrub, |d| d.index)
        } else {
            self.scrub
        };
        self.grab = Some(at);
        let ready = !self.playing
            && self.image.is_some()
            && self.displayed
                == Some(Displayed {
                    index: at,
                    exact: true,
                })
            && self.latest.is_none();
        if !ready {
            self.seek(at);
        }
    }

    pub fn grab_pending(&self) -> bool {
        self.grab.is_some()
    }

    /// Handles worker results; returns a grabbed frame once ready.
    pub fn poll(&mut self, ctx: &egui::Context) -> Option<GrabbedFrame> {
        for event in self.player.poll() {
            match event {
                Event::Frame {
                    frame,
                    exact,
                    playing,
                    request,
                } => {
                    if self.latest != Some(request) {
                        continue; // answer to a superseded request
                    }
                    if !playing {
                        self.latest = None;
                    }
                    self.displayed = Some(Displayed {
                        index: frame.index,
                        exact,
                    });
                    if playing || exact {
                        self.scrub = frame.index;
                    }
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [frame.image.width() as usize, frame.image.height() as usize],
                        frame.image.as_raw(),
                    );
                    match &mut self.texture {
                        Some(texture) => texture.set(color, TextureOptions::LINEAR),
                        None => {
                            self.texture =
                                Some(ctx.load_texture("video-frame", color, TextureOptions::LINEAR))
                        }
                    }
                    self.image = exact.then_some(frame.image);
                }
                Event::Ended(last) => {
                    if self.playing {
                        self.playing = false;
                        self.latest = None;
                        self.scrub = last;
                    }
                }
                Event::FrameCount(count) => {
                    self.info.frame_count = count;
                    self.scrub = self.scrub.min(self.info.last_frame());
                }
                Event::Error(message) => {
                    self.error = Some(message);
                    self.latest = None;
                    self.playing = false;
                }
            }
        }
        if let Some(worker) = &self.strip.worker {
            for (slot, frame) in worker.poll() {
                if let Some(s) = self.strip.slots.get_mut(slot) {
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [frame.image.width() as usize, frame.image.height() as usize],
                        frame.image.as_raw(),
                    );
                    let texture = ctx.load_texture(
                        format!("video-thumb-{slot}"),
                        color,
                        TextureOptions::LINEAR,
                    );
                    *s = Some((frame.index, texture));
                }
            }
        }
        let waiting =
            self.latest.is_some() || self.playing || self.strip.slots.iter().any(Option::is_none);
        if waiting {
            // Fallback in case a worker started without a repaint callback.
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        let at = self.grab?;
        match (self.displayed, &self.image) {
            (Some(Displayed { index, exact: true }), Some(image))
                if index == at && self.latest.is_none() =>
            {
                self.grab = None;
                Some(GrabbedFrame {
                    name: frame_document_name(&self.info.stem(), at),
                    image: image.clone(),
                })
            }
            _ if self.error.is_some() => {
                self.grab = None;
                None
            }
            _ => None,
        }
    }

    /// (Re)starts thumbnail decoding when the number of slots changes.
    fn layout_strip(&mut self, count: usize) {
        if count == self.strip.targets.len() {
            return;
        }
        let last = self.info.last_frame();
        let targets: Vec<u64> = match count {
            0 => Vec::new(),
            1 => vec![0],
            n => (0..n as u64).map(|i| i * last / (n as u64 - 1)).collect(),
        };
        self.strip.slots = vec![None; targets.len()];
        self.strip.worker = (!targets.is_empty()).then(|| {
            Thumbnails::new(
                self.info.path.clone(),
                targets.clone(),
                STRIP_HEIGHT,
                self.notify.clone(),
            )
        });
        self.strip.targets = targets;
    }

    /// Draws the tab; returns Grab / Extract clicks.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Vec<VideoAction> {
        let mut actions = Vec::new();
        let full = ui.available_rect_before_wrap();
        let controls_height = 150.0;
        let frame_area = Rect::from_min_size(
            full.min,
            Vec2::new(full.width(), (full.height() - controls_height).max(40.0)),
        );
        ui.allocate_rect(frame_area, Sense::hover());
        let painter = ui.painter_at(frame_area);
        painter.rect_filled(frame_area, 0.0, Color32::from_gray(20));
        let (w, h) = (self.info.width as f32, self.info.height as f32);
        let scale = (frame_area.width() / w).min(frame_area.height() / h);
        let frame_rect = Rect::from_center_size(frame_area.center(), Vec2::new(w, h) * scale);
        if let Some(texture) = &self.texture {
            painter.image(
                texture.id(),
                frame_rect,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        self.layout.frame = frame_rect;

        ui.add_space(4.0);
        let shown = self.displayed.map_or(self.scrub, |d| d.index);
        let mut info = format!(
            "{} × {}   {:.3} fps   {}   frame {} / {}   {}",
            self.info.width,
            self.info.height,
            self.info.fps(),
            format_timecode(self.info.duration()),
            shown,
            self.info.frame_count,
            format_timecode(self.info.timestamp(shown)),
        );
        if self.displayed.is_some_and(|d| !d.exact) {
            info.push_str("   (keyframe preview)");
        }
        ui.horizontal(|ui| {
            ui.label(info);
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });

        // Thumbnail strip.
        let strip_w = ui.available_width();
        let thumb_w = (STRIP_HEIGHT as f32 * w / h).max(8.0);
        let count = ((strip_w + STRIP_GAP) / (thumb_w + STRIP_GAP))
            .floor()
            .max(1.0) as usize;
        self.layout_strip(count.min(64));
        let (strip_rect, _) = ui.allocate_exact_size(
            Vec2::new(strip_w, STRIP_HEIGHT as f32 + 4.0),
            Sense::hover(),
        );
        let current_slot = self.current_slot(shown);
        self.layout.thumbnails.clear();
        let mut clicked = None;
        for (i, slot) in self.strip.slots.iter().enumerate() {
            let rect = Rect::from_min_size(
                strip_rect.min + Vec2::new(i as f32 * (thumb_w + STRIP_GAP), 2.0),
                Vec2::new(thumb_w, STRIP_HEIGHT as f32),
            );
            self.layout.thumbnails.push(rect);
            let response = ui.interact(rect, ui.id().with(("thumb", i)), Sense::click());
            let painter = ui.painter();
            match slot {
                Some((index, texture)) => {
                    painter.image(
                        texture.id(),
                        rect,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    if response.clicked() {
                        clicked = Some(*index);
                    }
                    response.on_hover_text(format!(
                        "frame {index}  {}",
                        format_timecode(self.info.timestamp(*index))
                    ));
                }
                None => {
                    painter.rect_filled(rect, 2.0, Color32::from_gray(40));
                }
            }
            if current_slot == Some(i) {
                painter.rect_stroke(
                    rect.expand(1.5),
                    2.0,
                    egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                    egui::StrokeKind::Outside,
                );
            }
        }
        if let Some(index) = clicked {
            self.seek(index);
        }

        // Timeline.
        let last = self.info.last_frame();
        ui.spacing_mut().slider_width = ui.available_width();
        let mut value = self.scrub;
        let response = ui.add(
            egui::Slider::new(&mut value, 0..=last.max(1))
                .show_value(false)
                .smart_aim(false),
        );
        self.layout.timeline = response.rect;
        let value = value.min(last);
        if response.dragged() {
            if value != self.scrub || self.playing {
                self.seek_preview(value);
            }
        } else if response.drag_stopped() || (response.changed() && value != self.scrub) {
            self.seek(value);
        }

        ui.horizontal(|ui| {
            let mut command = None;
            if ui.button("⏮ Start").clicked() {
                command = Some(Transport::Start);
            }
            if ui.button("◀ Step").clicked() {
                command = Some(Transport::Step(-1));
            }
            let play = if self.playing { "Pause" } else { "Play" };
            if ui.button(play).clicked() {
                command = Some(Transport::PlayPause);
            }
            if ui.button("Step ▶").clicked() {
                command = Some(Transport::Step(1));
            }
            if ui.button("End ⏭").clicked() {
                command = Some(Transport::End);
            }
            if let Some(command) = command {
                self.transport(command);
            }
            ui.separator();
            if ui.button("Grab Frame").clicked() {
                actions.push(VideoAction::Grab);
            }
            if ui.button("Extract Frames…").clicked() {
                actions.push(VideoAction::Extract);
            }
        });
        actions
    }

    /// The thumbnail slot whose frame range contains `index` (highlighted).
    pub fn current_slot(&self, index: u64) -> Option<usize> {
        // The latest thumbnail at or before `index`; of equal ones, the first.
        let mut best: Option<(usize, u64)> = None;
        for (i, slot) in self.strip.slots.iter().enumerate() {
            if let Some((frame, _)) = slot {
                if *frame <= index && best.is_none_or(|(_, f)| *frame > f) {
                    best = Some((i, *frame));
                }
            }
        }
        best.map(|(i, _)| i)
    }
}

/// Mode choice in the Extract Frames dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractChoice {
    EveryNth,
    SceneChanges,
}

/// The Extract Frames dialog and its running job.
pub struct ExtractDialog {
    pub video: VideoId,
    pub info: VideoInfo,
    pub mode: ExtractChoice,
    pub n: u64,
    /// Percent, 1–100.
    pub threshold: u32,
    pub min_scene: u64,
    pub from: u64,
    pub to: u64,
    job: Option<ExtractJob>,
    pub error: Option<String>,
}

/// What happened in the Extract Frames dialog this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractOutcome {
    Open,
    Start,
    Cancel,
}

impl ExtractDialog {
    pub fn new(video: VideoId, info: VideoInfo) -> Self {
        Self {
            video,
            to: info.last_frame(),
            info,
            mode: ExtractChoice::EveryNth,
            n: 30,
            threshold: 30,
            min_scene: 10,
            from: 0,
            job: None,
            error: None,
        }
    }

    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    pub fn progress(&self) -> Option<f32> {
        self.job.as_ref().map(ExtractJob::progress)
    }

    pub fn spec(&self) -> ExtractSpec {
        let mode = match self.mode {
            ExtractChoice::EveryNth => ExtractMode::EveryNth(self.n.clamp(1, 100_000)),
            ExtractChoice::SceneChanges => ExtractMode::SceneChanges {
                threshold: self.threshold.clamp(1, 100) as f32 / 100.0,
                min_scene: self.min_scene.clamp(1, 1000),
            },
        };
        let to = self.to.min(self.info.last_frame());
        ExtractSpec {
            path: self.info.path.clone(),
            mode,
            from: self.from.min(to),
            to,
        }
    }

    pub fn start(&mut self, notify: Notify) {
        self.error = None;
        self.job = Some(ExtractJob::start(self.spec(), notify));
    }

    /// Stops a running job (its frames are discarded).
    pub fn cancel_job(&mut self) {
        if let Some(mut job) = self.job.take() {
            job.cancel();
        }
    }

    /// The finished job's result, if it has finished.
    pub fn take_result(&mut self) -> Option<Result<Vec<crate::frame_set::FrameSetEntry>, String>> {
        let result = self.job.as_ref()?.try_result()?;
        self.job = None;
        Some(result)
    }

    /// Draws the dialog window.
    pub fn show(&mut self, ctx: &egui::Context) -> ExtractOutcome {
        let mut outcome = ExtractOutcome::Open;
        let last = self.info.last_frame();
        egui::Window::new("Extract Frames")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(&self.info.name);
                let running = self.job.is_some();
                ui.add_enabled_ui(!running, |ui| {
                    egui::Grid::new("extract_frames")
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.label("Mode");
                            ui.horizontal(|ui| {
                                ui.selectable_value(
                                    &mut self.mode,
                                    ExtractChoice::EveryNth,
                                    "Every Nth frame",
                                );
                                ui.selectable_value(
                                    &mut self.mode,
                                    ExtractChoice::SceneChanges,
                                    "Scene changes",
                                );
                            });
                            ui.end_row();
                            ui.label("From frame");
                            ui.add(egui::DragValue::new(&mut self.from).range(0..=last));
                            ui.end_row();
                            ui.label("To frame");
                            ui.add(egui::DragValue::new(&mut self.to).range(0..=last));
                            ui.end_row();
                            match self.mode {
                                ExtractChoice::EveryNth => {
                                    ui.label("Every N frames");
                                    ui.add(egui::DragValue::new(&mut self.n).range(1..=100_000));
                                    ui.end_row();
                                }
                                ExtractChoice::SceneChanges => {
                                    ui.label("Threshold");
                                    ui.add(
                                        egui::Slider::new(&mut self.threshold, 1..=100)
                                            .suffix(" %"),
                                    );
                                    ui.end_row();
                                    ui.label("Minimum scene length");
                                    ui.add(
                                        egui::DragValue::new(&mut self.min_scene)
                                            .range(1..=1000)
                                            .suffix(" frames"),
                                    );
                                    ui.end_row();
                                }
                            }
                        });
                });
                if let Some(progress) = self.progress() {
                    ui.add(egui::ProgressBar::new(progress).show_percentage());
                }
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        outcome = ExtractOutcome::Cancel;
                    }
                    if ui
                        .add_enabled(!running, egui::Button::new("Extract"))
                        .clicked()
                    {
                        outcome = ExtractOutcome::Start;
                    }
                });
            });
        if self.to < self.from {
            self.to = self.from;
        }
        outcome
    }
}

/// Selection and texture cache of the Frame Set panel.
#[derive(Default)]
pub struct FrameSetView {
    pub selected: BTreeSet<u64>,
    textures: HashMap<u64, TextureHandle>,
}

/// What the user did in the Frame Set panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameSetAction {
    Open(u64),
    Remove(Vec<u64>),
    Clear,
}

/// Size of a Frame Set grid cell's image area.
const CELL: Vec2 = Vec2::new(128.0, 72.0);

pub fn frame_set_panel(
    ui: &mut egui::Ui,
    set: &FrameSet,
    view: &mut FrameSetView,
) -> Vec<FrameSetAction> {
    let mut actions = Vec::new();
    view.textures.retain(|id, _| set.get(*id).is_some());
    view.selected.retain(|id| set.get(*id).is_some());
    ui.horizontal(|ui| {
        ui.label(format!(
            "{} frame{}",
            set.len(),
            if set.len() == 1 { "" } else { "s" }
        ));
        let single = (view.selected.len() == 1).then(|| *view.selected.first().unwrap());
        if ui
            .add_enabled(single.is_some(), egui::Button::new("Open"))
            .clicked()
        {
            actions.push(FrameSetAction::Open(single.unwrap()));
        }
        if ui
            .add_enabled(!view.selected.is_empty(), egui::Button::new("Remove"))
            .clicked()
        {
            actions.push(FrameSetAction::Remove(
                view.selected.iter().copied().collect(),
            ));
        }
        if ui
            .add_enabled(!set.is_empty(), egui::Button::new("Clear"))
            .clicked()
        {
            actions.push(FrameSetAction::Clear);
        }
    });
    if set.is_empty() {
        ui.weak("Extract frames from a video to collect them here.");
        return actions;
    }
    ui.separator();
    let modifiers = ui.input(|i| i.modifiers);
    ui.horizontal_wrapped(|ui| {
        for entry in set.entries() {
            let texture = view.textures.entry(entry.id).or_insert_with(|| {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [
                        entry.thumbnail.width() as usize,
                        entry.thumbnail.height() as usize,
                    ],
                    entry.thumbnail.as_raw(),
                );
                ui.ctx().load_texture(
                    format!("frame-set-{}", entry.id),
                    image,
                    TextureOptions::LINEAR,
                )
            });
            let selected = view.selected.contains(&entry.id);
            let response = ui
                .vertical(|ui| {
                    ui.set_width(CELL.x);
                    let (rect, response) = ui.allocate_exact_size(CELL, Sense::click());
                    let (tw, th) = (
                        entry.thumbnail.width() as f32,
                        entry.thumbnail.height() as f32,
                    );
                    let scale = (CELL.x / tw).min(CELL.y / th);
                    let image_rect =
                        Rect::from_center_size(rect.center(), Vec2::new(tw, th) * scale);
                    ui.painter().image(
                        texture.id(),
                        image_rect,
                        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    if selected {
                        ui.painter().rect_stroke(
                            rect,
                            2.0,
                            egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                            egui::StrokeKind::Inside,
                        );
                    }
                    ui.small(format!("{} #{}", entry.video_name, entry.frame));
                    ui.small(format_timecode(entry.timestamp));
                    response
                })
                .inner;
            if response.double_clicked() {
                actions.push(FrameSetAction::Open(entry.id));
            } else if response.clicked() {
                if modifiers.command {
                    if !view.selected.remove(&entry.id) {
                        view.selected.insert(entry.id);
                    }
                } else if modifiers.shift && !view.selected.is_empty() {
                    let ids: Vec<u64> = set.entries().iter().map(|e| e.id).collect();
                    let anchor = ids
                        .iter()
                        .position(|id| view.selected.contains(id))
                        .unwrap_or(0);
                    let here = ids.iter().position(|id| *id == entry.id).unwrap_or(0);
                    let (a, b) = (anchor.min(here), anchor.max(here));
                    view.selected.extend(&ids[a..=b]);
                } else {
                    view.selected.clear();
                    view.selected.insert(entry.id);
                }
            }
        }
    });
    actions
}
