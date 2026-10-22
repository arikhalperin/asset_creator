//! The Parts Layout tab: a paper-model PDF's pages with the detected parts,
//! the A4 sheets they are packed onto, a tray for unplaced and oversized
//! parts, and the Split window.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use egui::{Color32, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use image::{GrayImage, RgbaImage};

use crate::pdf::images::{self, SizeSource};
use crate::pdf::layout::{self, Item, Layout};
use crate::pdf::pack::{self, Placement};
use crate::pdf::parts::{self, Part};
use crate::pdf::render::{self, Page};
use crate::pdf::{mm_to_px, split};
use crate::recolor::{self, delta_e, Recolor};

mod enhance;
pub use enhance::EnhanceDialog;

/// Id of a Parts Layout tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartsId(pub u64);

/// Displayed textures are this fraction of the 300-dpi pixels.
const PREVIEW_SCALE: f32 = 0.25;

/// Progress from the background loader.
enum LoadEvent {
    Pages(usize),
    /// The next page.
    Page(Box<Loaded>),
    /// A page loaded again (image DPI override).
    Replace(usize, Box<Loaded>),
    /// A folder image that could not be read (file name, reason).
    Skipped(String, String),
    Failed(String),
}

/// A page with its parts, made on a worker: the page preview and each part's
/// cut-out image and preview; for image pages, the file and its size source.
struct Loaded {
    page: Page,
    parts: Vec<Part>,
    preview: RgbaImage,
    cut: Vec<(RgbaImage, RgbaImage)>,
    source: Option<(PathBuf, SizeSource)>,
}

impl Loaded {
    fn of(page: Page, index: usize, source: Option<(PathBuf, SizeSource)>) -> Self {
        let parts = parts::detect(&page.image, index);
        let preview_img = preview(&page.image);
        let cut = parts
            .iter()
            .map(|p| {
                let img = p.image(&page.image);
                let pre = preview(&img);
                (img, pre)
            })
            .collect();
        Self {
            page,
            parts,
            preview: preview_img,
            cut,
            source,
        }
    }
}

/// Progress of a running export.
enum ExportEvent {
    Progress(usize, usize),
    Finished(Result<Vec<PathBuf>, String>),
}

/// A running Export A4 Pages… on a worker thread.
struct ExportJob {
    rx: Receiver<ExportEvent>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    done: usize,
    total: usize,
}

/// What the tab asks the app to do.
#[derive(Debug, Clone, PartialEq)]
pub enum PartsAction {
    /// Export A4 Pages… (with PNGs).
    Export { pngs: bool },
    /// MobileSAM refine at a page point (pixels at 300 dpi).
    Refine { page: usize, point: (f32, f32) },
    /// Load an image page again at another DPI.
    ReloadPage { page: usize, dpi: f32 },
    /// MobileSAM outline of an area to protect from recolouring.
    Protect { page: usize, point: (f32, f32) },
    /// Apply the Recolor… window's mapping to all pages.
    ApplyRecolor,
    /// Open Page in Editor (page right-click).
    OpenInEditor { page: usize },
    /// Apply the Enhance Lines… window to the pages with areas.
    ApplyEnhance,
}

/// A page job: recolour or enhance every page.
#[derive(Debug, Clone, Copy, PartialEq)]
enum JobKind {
    Recolor,
    Enhance,
}

/// The Recolor… window's state.
#[derive(Debug, Clone, PartialEq)]
pub struct RecolorDialog {
    pub rc: Recolor,
    /// Swatch picked by a click on a page.
    pub selected: Option<usize>,
    /// Clicks on pages outline areas to protect (MobileSAM).
    pub protect_mode: bool,
    /// Drags on pages draw areas to recolour (rectangle; Shift: lasso).
    pub area_mode: bool,
}

/// A running recolour of all pages.
struct RecolorJob {
    rx: Receiver<RecolorEvent>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    done: usize,
    total: usize,
}

enum RecolorEvent {
    Progress(usize, usize),
    Finished(Option<Vec<RgbaImage>>),
}

/// A recorded split, replayed when part images are re-cut.
type SplitOp = (usize, (f32, f32), (f32, f32), f32, split::GlueOn);

/// The Split window's state.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitDialog {
    pub item: usize,
    /// The cut line in item pixels.
    pub line: Option<((f32, f32), (f32, f32))>,
    pub glue_mm: f32,
    /// Which piece gets the glue strip.
    pub glue_on: split::GlueOn,
    drag_start: Option<(f32, f32)>,
}

/// A manual A4 frame on a page: the printable area (turned 90° when
/// `rotated`) with its top-left at `x`, `y` page pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub page: usize,
    pub x: i32,
    pub y: i32,
    pub rotated: bool,
}

impl Frame {
    /// Width and height on the page, pixels.
    pub fn size(&self) -> (u32, u32) {
        // A full A4 sheet.
        let (pw, ph) = layout::sheet_px();
        if self.rotated {
            (ph, pw)
        } else {
            (pw, ph)
        }
    }
}

/// A part or tray item being dragged onto a sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SheetDrag {
    item: usize,
    /// Grab point inside the item's (placed) box, in pixels.
    grab: (f32, f32),
}

/// One open PDF.
pub struct PartsTab {
    pub name: String,
    pub path: PathBuf,
    pub pages: Vec<Page>,
    /// Expected page count once known.
    pub page_count: Option<usize>,
    pub error: Option<String>,
    /// Detected (and edited) parts of all pages.
    pub proposals: Vec<Part>,
    /// Selected proposals (indices into `proposals`).
    pub selected: BTreeSet<usize>,
    pub layout: Layout,
    /// Selected layout item.
    pub item: Option<usize>,
    /// Clicks on pages refine/add parts with MobileSAM.
    pub refine: bool,
    /// Whether MobileSAM is available (the app updates it).
    pub sam_ready: bool,
    pub pngs: bool,
    pub split: Option<SplitDialog>,
    pub status: Option<String>,
    loader: Option<Receiver<LoadEvent>>,
    page_previews: Vec<RgbaImage>,
    /// Cut-out image and preview of each proposal (parallel to `proposals`).
    part_images: Vec<RgbaImage>,
    part_previews: Vec<RgbaImage>,
    /// Preview of each layout item (`None`: made when first drawn).
    item_previews: Vec<Option<RgbaImage>>,
    page_textures: Vec<Option<TextureHandle>>,
    item_textures: Vec<Option<(u64, TextureHandle)>>,
    /// Bumps when items change (texture cache key).
    generation: u64,
    drag: Option<SheetDrag>,
    /// Where sheets and placed items were drawn last frame (screen points).
    pub sheet_rects: Vec<(usize, Rect)>,
    pub item_rects: Vec<(usize, Rect)>,
    /// Pages in "Manual (A4 frames)" mode.
    pub manual: Vec<bool>,
    /// Manual A4 frames of all pages, in creation order.
    pub frames: Vec<Frame>,
    /// Selected frame.
    pub frame: Option<usize>,
    /// Where frames were drawn last frame (screen points).
    pub frame_rects: Vec<(usize, Rect)>,
    export_job: Option<ExportJob>,
    /// Zoom of the page view (1 = page width fits the column).
    pub page_zoom: f32,
    /// A frame being dragged: index, its position at the press, press point.
    frame_drag: Option<(usize, (i32, i32), Pos2)>,
    /// Image files of a folder that could not be read (name, reason).
    pub skipped: Vec<(String, String)>,
    /// The Recolor… window.
    pub recolor: Option<RecolorDialog>,
    /// The pages before the first recolour (for Revert).
    original_pages: Option<Vec<RgbaImage>>,
    /// Areas never recoloured, per page (page pixels).
    pub protect: Vec<Option<GrayImage>>,
    /// Recolour Area: drawn areas (page, polygon in page pixels); once
    /// there are any, only pixels inside them are recoloured.
    pub areas: Vec<(usize, Vec<(f32, f32)>)>,
    /// An area being drawn.
    area_drag: Option<AreaDrag>,
    page_job: Option<RecolorJob>,
    /// What the running page job does.
    page_job_kind: JobKind,
    /// Enhance Lines… window.
    pub enhance: Option<enhance::EnhanceDialog>,
    /// Enhance Lines areas (page, polygon in page pixels).
    pub enhance_areas: Vec<(usize, Vec<(f32, f32)>)>,
    /// Enhanced page previews while the window is open.
    enhance_textures: Vec<Option<TextureHandle>>,
    /// The pages already carry the window's enhancement (no preview on top
    /// until a setting or an area changes).
    enhance_applied: bool,
    /// Recoloured page previews while the window is open.
    recolor_textures: Vec<Option<TextureHandle>>,
    /// Per page preview: colours and swatch membership (valid for one
    /// tolerance; colour/detail changes reuse it).
    recolor_prepared: Vec<Option<recolor::Prepared>>,
    /// Preview preparation running on a worker after the window opens.
    recolor_preparing: Option<Receiver<Vec<recolor::Prepared>>>,
    protect_textures: Vec<Option<TextureHandle>>,
    /// Splits made since the last rebuild (replayed by `recut`).
    splits: Vec<SplitOp>,
    /// Bumps when page pixels change (MobileSAM cache key).
    pub page_version: u64,
    /// Where pages were drawn last frame (screen points) and their scale.
    pub page_rects: Vec<(usize, Rect, f32)>,
    /// For image pages: the file and where its size came from.
    pub image_pages: Vec<Option<(PathBuf, SizeSource)>>,
    /// DPI field of each image page.
    pub dpi_edit: Vec<f32>,
    reloads: Vec<Receiver<LoadEvent>>,
}

impl PartsTab {
    /// Starts rendering and detecting `path` on a worker thread with PDFium
    /// from `lib`.
    pub fn open(path: &Path, lib: PathBuf, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::channel();
        let p = path.to_path_buf();
        std::thread::Builder::new()
            .name("pdf-render".to_owned())
            .spawn(move || {
                let count = match render::page_count(&lib, &p) {
                    Ok(n) => n,
                    Err(e) => {
                        let _ = tx.send(LoadEvent::Failed(e));
                        notify();
                        return;
                    }
                };
                let _ = tx.send(LoadEvent::Pages(count));
                notify();
                for i in 0..count {
                    match render::render_page(&lib, &p, i) {
                        Ok(page) => {
                            let event = LoadEvent::Page(Box::new(Loaded::of(page, i, None)));
                            if tx.send(event).is_err() {
                                return; // The tab was closed.
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(LoadEvent::Failed(e));
                            notify();
                            return;
                        }
                    }
                    notify();
                }
            })
            .expect("spawn pdf loader");
        Self::with_loader(path, rx)
    }

    /// Starts loading the image `files` (one page each, in order) of the
    /// folder `dir` on a worker thread.
    pub fn open_images(
        dir: &Path,
        files: Vec<PathBuf>,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("image-pages".to_owned())
            .spawn(move || {
                let _ = tx.send(LoadEvent::Pages(files.len()));
                notify();
                // Unreadable files are skipped (and reported); the rest load.
                let mut index = 0;
                for file in files {
                    let name = file
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let event = match images::load_page(&file, None) {
                        Ok((page, source)) => {
                            let loaded = Loaded::of(page, index, Some((file, source)));
                            index += 1;
                            LoadEvent::Page(Box::new(loaded))
                        }
                        Err(e) => LoadEvent::Skipped(name, e),
                    };
                    if tx.send(event).is_err() {
                        return;
                    }
                    notify();
                }
            })
            .expect("spawn image loader");
        Self::with_loader(dir, rx)
    }

    /// Loads image page `page` again at `dpi` (re-scaled, parts found again)
    /// on a worker thread.
    pub fn reload_page(
        &mut self,
        page: usize,
        dpi: f32,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> bool {
        let Some(Some((file, _))) = self.image_pages.get(page).cloned() else {
            return false;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("image-page-reload".to_owned())
            .spawn(move || {
                let event = match images::load_page(&file, Some(dpi)) {
                    Ok((p, source)) => LoadEvent::Replace(
                        page,
                        Box::new(Loaded::of(p, page, Some((file, source)))),
                    ),
                    Err(e) => LoadEvent::Failed(e),
                };
                let _ = tx.send(event);
                notify();
            })
            .expect("spawn page reload");
        self.reloads.push(rx);
        self.status = Some(format!("Page {}: re-scaling to {dpi:.0} dpi…", page + 1));
        true
    }

    /// Whether a page reload (DPI override) is running.
    pub fn reloading(&self) -> bool {
        !self.reloads.is_empty()
    }

    fn with_loader(path: &Path, rx: Receiver<LoadEvent>) -> Self {
        Self {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: path.to_path_buf(),
            pages: Vec::new(),
            page_count: None,
            error: None,
            proposals: Vec::new(),
            selected: BTreeSet::new(),
            layout: Layout::default(),
            item: None,
            refine: false,
            sam_ready: false,
            pngs: false,
            split: None,
            status: None,
            loader: Some(rx),
            page_previews: Vec::new(),
            part_images: Vec::new(),
            part_previews: Vec::new(),
            item_previews: Vec::new(),
            page_textures: Vec::new(),
            item_textures: Vec::new(),
            generation: 0,
            drag: None,
            sheet_rects: Vec::new(),
            item_rects: Vec::new(),
            manual: Vec::new(),
            frames: Vec::new(),
            frame: None,
            frame_rects: Vec::new(),
            export_job: None,
            page_zoom: 1.0,
            frame_drag: None,
            skipped: Vec::new(),
            recolor: None,
            original_pages: None,
            protect: Vec::new(),
            areas: Vec::new(),
            area_drag: None,
            page_job: None,
            page_job_kind: JobKind::Recolor,
            enhance: None,
            enhance_areas: Vec::new(),
            enhance_textures: Vec::new(),
            enhance_applied: false,
            recolor_textures: Vec::new(),
            recolor_prepared: Vec::new(),
            recolor_preparing: None,
            protect_textures: Vec::new(),
            splits: Vec::new(),
            page_version: 0,
            page_rects: Vec::new(),
            image_pages: Vec::new(),
            dpi_edit: Vec::new(),
            reloads: Vec::new(),
        }
    }

    fn add_page(&mut self, loaded: Loaded) {
        let dpi = match loaded.source.as_ref().map(|s| s.1) {
            Some(SizeSource::FileDpi(d) | SizeSource::Override(d)) => d,
            Some(SizeSource::AssumedA3 | SizeSource::IgnoredDpi(_)) => {
                // The DPI that A3 implies, as the field's starting value.
                let w = loaded.page.image.width() as f32;
                let img_w = loaded
                    .source
                    .as_ref()
                    .and_then(|(f, _)| image::image_dimensions(f).ok())
                    .map_or(w, |(iw, _)| iw as f32);
                (img_w / (loaded.page.width_mm / 25.4)).round()
            }
            None => crate::pdf::DPI,
        };
        self.pages.push(loaded.page);
        self.manual.push(false);
        self.page_previews.push(loaded.preview);
        self.page_textures.push(None);
        self.image_pages.push(loaded.source);
        self.dpi_edit.push(dpi);
        self.proposals.extend(loaded.parts);
        for (img, pre) in loaded.cut {
            self.part_images.push(img);
            self.part_previews.push(pre);
        }
    }

    /// Swaps in a reloaded page: its old parts go, the new ones come in.
    fn replace_page(&mut self, i: usize, loaded: Loaded) {
        if i >= self.pages.len() {
            return;
        }
        for k in (0..self.proposals.len()).rev() {
            if self.proposals[k].page == i {
                self.proposals.remove(k);
                self.part_images.remove(k);
                self.part_previews.remove(k);
            }
        }
        self.selected.clear();
        let size = loaded
            .source
            .as_ref()
            .map(|s| s.1.describe())
            .unwrap_or_default();
        self.pages[i] = loaded.page;
        self.page_previews[i] = loaded.preview;
        self.page_textures[i] = None;
        self.image_pages[i] = loaded.source;
        self.proposals.extend(loaded.parts);
        for (img, pre) in loaded.cut {
            self.part_images.push(img);
            self.part_previews.push(pre);
        }
        self.rebuild();
        self.status = Some(format!(
            "Page {}: re-scaled ({size}); parts found again",
            i + 1
        ));
    }

    /// Takes finished pages from the loader and export progress. Returns
    /// whether anything changed.
    pub fn pump(&mut self) -> bool {
        let mut exported = self.pump_export() | self.pump_recolor();
        // Page reloads (DPI overrides).
        let mut events = Vec::new();
        self.reloads.retain(|rx| match rx.try_recv() {
            Ok(e) => {
                events.push(e);
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => false,
        });
        for e in events {
            exported = true;
            match e {
                LoadEvent::Replace(i, loaded) => self.replace_page(i, *loaded),
                LoadEvent::Failed(e) => self.status = Some(format!("Could not re-scale: {e}")),
                _ => {}
            }
        }
        let Some(rx) = &self.loader else {
            return exported;
        };
        let events: Vec<LoadEvent> = rx.try_iter().collect();
        let changed = !events.is_empty();
        let mut done = false;
        for event in events {
            match event {
                LoadEvent::Pages(n) => self.page_count = Some(n),
                LoadEvent::Page(loaded) => self.add_page(*loaded),
                LoadEvent::Skipped(name, reason) => {
                    self.skipped.push((name, reason));
                    if let Some(n) = &mut self.page_count {
                        *n = n.saturating_sub(1);
                    }
                }
                LoadEvent::Replace(i, loaded) => self.replace_page(i, *loaded),
                LoadEvent::Failed(e) => {
                    self.error = Some(e);
                    done = true;
                }
            }
        }
        if self.page_count.is_some_and(|n| self.pages.len() == n) {
            done = true;
            if self.pages.is_empty() && self.error.is_none() {
                self.error = Some("none of the images could be read".to_owned());
            }
        }
        if done {
            self.loader = None;
            self.rebuild();
        }
        changed
    }

    pub fn loading(&self) -> bool {
        self.loader.is_some()
    }

    fn pump_export(&mut self) -> bool {
        let Some(job) = &mut self.export_job else {
            return false;
        };
        let mut finished = None;
        let mut changed = false;
        while let Ok(event) = job.rx.try_recv() {
            changed = true;
            match event {
                ExportEvent::Progress(done, total) => {
                    job.done = done;
                    job.total = total;
                }
                ExportEvent::Finished(result) => finished = Some(result),
            }
        }
        if let Some(result) = finished {
            self.export_job = None;
            self.status = Some(match result {
                Ok(files) => format!(
                    "Exported {} file(s): {}",
                    files.len(),
                    files
                        .first()
                        .map(|f| f.display().to_string())
                        .unwrap_or_default()
                ),
                Err(e) if e == "cancelled" => "Export cancelled".to_owned(),
                Err(e) => format!("Could not export: {e}"),
            });
        }
        changed
    }

    /// Whether an export is running, and its progress (sheets done, total).
    pub fn export_progress(&self) -> Option<(usize, usize)> {
        self.export_job.as_ref().map(|j| (j.done, j.total))
    }

    /// Starts Export A4 Pages… to `path` on a worker thread (packed sheets,
    /// then frame sheets). Returns false if nothing to export or one runs.
    pub fn start_export(&mut self, path: &Path, notify: Arc<dyn Fn() + Send + Sync>) -> bool {
        if self.export_job.is_some() {
            return false;
        }
        let used: Vec<usize> = self
            .layout
            .placements
            .iter()
            .flatten()
            .map(|p| p.sheet)
            .collect::<BTreeSet<usize>>()
            .into_iter()
            .collect();
        let frames = self.export_frames();
        let total = used.len() + frames.len();
        if total == 0 {
            self.status = Some("Nothing to export".to_owned());
            return false;
        }
        // The worker gets its own copies (the tab stays editable).
        let layout = self.layout.clone();
        let pages: std::collections::HashMap<usize, RgbaImage> = frames
            .iter()
            .map(|f| f.page)
            .collect::<BTreeSet<usize>>()
            .into_iter()
            .filter_map(|p| self.pages.get(p).map(|page| (p, page.image.clone())))
            .collect();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = cancel.clone();
        let (path, pngs) = (path.to_path_buf(), self.pngs);
        std::thread::Builder::new()
            .name("pdf-export".to_owned())
            .spawn(move || {
                let sheet = |i: usize| {
                    if i < used.len() {
                        layout.compose(used[i])
                    } else {
                        let f = frames[i - used.len()];
                        layout::compose_frame(&pages[&f.page], f.x, f.y, f.rotated)
                    }
                };
                let progress = |done: usize, total: usize| {
                    let _ = tx.send(ExportEvent::Progress(done, total));
                    notify();
                };
                let result = layout::write_export(total, sheet, &path, pngs, &progress, &stop);
                let _ = tx.send(ExportEvent::Finished(result));
                notify();
            })
            .expect("spawn pdf export");
        self.export_job = Some(ExportJob {
            rx,
            cancel,
            done: 0,
            total,
        });
        self.status = Some(format!("Exporting {total} A4 sheet(s)…"));
        true
    }

    /// Stops a running export (its files are removed).
    pub fn cancel_export(&mut self) {
        if let Some(job) = &self.export_job {
            job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Re-creates the layout items from the proposals and packs them.
    pub fn rebuild(&mut self) {
        self.splits.clear();
        let had_layout = !self.layout.items.is_empty();
        // Parts of manual pages are printed through their frames instead.
        let keep: Vec<usize> = (0..self.proposals.len())
            .filter(|&k| {
                !self
                    .manual
                    .get(self.proposals[k].page)
                    .copied()
                    .unwrap_or(false)
            })
            .collect();
        let items = keep
            .iter()
            .map(|&k| Item {
                label: format!("{}.{}", self.proposals[k].page + 1, k + 1),
                image: self.part_images[k].clone(),
            })
            .collect();
        self.layout = Layout::new(items);
        self.item_previews = keep
            .iter()
            .map(|&k| Some(self.part_previews[k].clone()))
            .collect();
        if had_layout {
            self.status = Some("Parts changed: the sheets were re-packed".to_owned());
        }
        self.item = None;
        self.split = None;
        self.generation += 1;
        self.item_textures.clear();
    }

    /// Merges the selected proposals of one page into one part.
    pub fn merge_selected(&mut self) -> bool {
        let chosen: Vec<usize> = self.selected.iter().copied().collect();
        if chosen.len() < 2 {
            return false;
        }
        let page = self.proposals[chosen[0]].page;
        if chosen.iter().any(|&i| self.proposals[i].page != page) {
            self.status = Some("Merge needs parts on the same page".to_owned());
            return false;
        }
        let refs: Vec<&Part> = chosen.iter().map(|&i| &self.proposals[i]).collect();
        let Some(merged) = Part::merge(&refs) else {
            return false;
        };
        for &i in chosen.iter().rev() {
            self.proposals.remove(i);
            self.part_images.remove(i);
            self.part_previews.remove(i);
        }
        let img = merged.image(&self.pages[page].image);
        self.part_previews.insert(chosen[0], preview(&img));
        self.part_images.insert(chosen[0], img);
        self.proposals.insert(chosen[0], merged);
        self.selected = [chosen[0]].into();
        self.rebuild();
        true
    }

    /// Deletes the selected proposals (e.g. a title).
    pub fn delete_selected(&mut self) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        for &i in self.selected.iter().rev() {
            self.proposals.remove(i);
            self.part_images.remove(i);
            self.part_previews.remove(i);
        }
        self.selected.clear();
        self.rebuild();
        true
    }

    /// The innermost proposal under a page point: the smallest one whose
    /// mask (its inside included) contains it. Empty paper — even inside a
    /// concave part's box — is no part.
    pub fn proposal_at(&self, page: usize, point: (f32, f32)) -> Option<usize> {
        let (x, y) = (point.0.max(0.0) as u32, point.1.max(0.0) as u32);
        let inside = |p: &Part| {
            p.page == page
                && x >= p.rect.x
                && y >= p.rect.y
                && x < p.rect.right()
                && y < p.rect.bottom()
        };
        let area = |p: &Part| p.rect.width as u64 * p.rect.height as u64;
        let by_mask = self
            .proposals
            .iter()
            .enumerate()
            .filter(|(_, p)| inside(p) && p.mask.get_pixel(x - p.rect.x, y - p.rect.y)[0] > 0)
            .min_by_key(|(_, p)| area(p))
            .map(|(i, _)| i);
        by_mask
    }

    /// A MobileSAM mask for a click on `page` at `point`: replaces the part
    /// under the click, or adds a part.
    pub fn apply_refine(&mut self, page: usize, point: (f32, f32), mask: &GrayImage) -> bool {
        let Some(new) = Part::from_page_mask(page, mask) else {
            return false;
        };
        let hit = self.proposal_at(page, point);
        let Some(page_image) = self.pages.get(page).map(|p| &p.image) else {
            return false;
        };
        let img = new.image(page_image);
        let pre = preview(&img);
        match hit {
            Some(i) => {
                self.proposals[i] = new;
                self.part_images[i] = img;
                self.part_previews[i] = pre;
                self.selected = [i].into();
            }
            None => {
                self.proposals.push(new);
                self.part_images.push(img);
                self.part_previews.push(pre);
                self.selected = [self.proposals.len() - 1].into();
            }
        }
        self.rebuild();
        true
    }

    /// Splits layout item `i` along a line (item pixels).
    pub fn split_item(&mut self, i: usize, a: (f32, f32), b: (f32, f32), glue_mm: f32) -> bool {
        self.split_item_on(i, a, b, glue_mm, split::GlueOn::Smaller)
    }

    /// [`Self::split_item`] with the glue strip on the chosen piece.
    pub fn split_item_on(
        &mut self,
        i: usize,
        a: (f32, f32),
        b: (f32, f32),
        glue_mm: f32,
        on: split::GlueOn,
    ) -> bool {
        if !self.layout.split(i, a, b, glue_mm, on) {
            self.status = Some("The line must cross the part".to_owned());
            return false;
        }
        self.splits.push((i, a, b, glue_mm, on));
        self.item_previews.resize(self.layout.items.len(), None);
        self.item_previews[i] = None;
        self.generation += 1;
        self.item_textures.clear();
        self.layout.place_tray();
        let still = self.layout.oversized().len();
        self.status = Some(if still > 0 {
            format!("Split. {still} part(s) still do not fit on A4")
        } else {
            "Split. All parts fit on A4".to_owned()
        });
        true
    }

    /// Turns "Manual (A4 frames)" on or off for a page. On: its parts leave
    /// the layout and it gets a first frame if it has none.
    pub fn set_manual(&mut self, page: usize, on: bool) {
        if page >= self.pages.len() || self.manual[page] == on {
            return;
        }
        self.manual[page] = on;
        // Hidden proposals must not stay selected (Merge/Delete act on them).
        let proposals = &self.proposals;
        self.selected
            .retain(|&k| proposals.get(k).is_some_and(|p| p.page != page));
        if on && !self.frames.iter().any(|f| f.page == page) {
            self.add_frame(page);
        }
        self.rebuild();
        self.status = Some(if on {
            format!(
                "Page {}: manual A4 frames; its parts left the layout",
                page + 1
            )
        } else {
            format!("Page {}: its parts are back in the layout", page + 1)
        });
    }

    /// Adds a frame to a page: at its top-left, or beside the page's last
    /// frame (next row when it would start past the page's right edge).
    pub fn add_frame(&mut self, page: usize) -> usize {
        let (pw, _) = self
            .pages
            .get(page)
            .map_or((0, 0), |p| p.image.dimensions());
        let frame = match self.frames.iter().rev().find(|f| f.page == page) {
            Some(last) => {
                let (w, h) = last.size();
                let x = last.x + w as i32;
                if x < pw as i32 {
                    Frame { x, ..*last }
                } else {
                    Frame {
                        x: 0,
                        y: last.y + h as i32,
                        ..*last
                    }
                }
            }
            // Turned to match the page: two frames then tile an A3 page.
            None => {
                let (pw, ph) = self
                    .pages
                    .get(page)
                    .map_or((0, 0), |p| p.image.dimensions());
                Frame {
                    page,
                    x: 0,
                    y: 0,
                    rotated: ph > pw,
                }
            }
        };
        self.frames.push(frame);
        self.frame = Some(self.frames.len() - 1);
        self.frames.len() - 1
    }

    pub fn remove_frame(&mut self, i: usize) {
        if i < self.frames.len() {
            self.frames.remove(i);
            self.frame = None;
        }
    }

    /// Turns a frame 90° about its top-left.
    pub fn rotate_frame(&mut self, i: usize) {
        if let Some(f) = self.frames.get_mut(i) {
            f.rotated = !f.rotated;
        }
    }

    /// Frames of manual pages, in page then creation order.
    pub fn export_frames(&self) -> Vec<Frame> {
        let mut out: Vec<(usize, Frame)> = self
            .frames
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, f)| self.manual.get(f.page).copied().unwrap_or(false))
            .collect();
        out.sort_by_key(|(i, f)| (f.page, *i));
        out.into_iter().map(|(_, f)| f).collect()
    }

    /// All export sheets: the packed sheets, then one per frame.
    pub fn sheets(&self) -> Vec<RgbaImage> {
        // Packed sheets without parts are not printed.
        let used: std::collections::BTreeSet<usize> = self
            .layout
            .placements
            .iter()
            .flatten()
            .map(|p| p.sheet)
            .collect();
        let mut sheets: Vec<RgbaImage> = used.iter().map(|&s| self.layout.compose(s)).collect();
        for f in self.export_frames() {
            if let Some(page) = self.pages.get(f.page) {
                sheets.push(layout::compose_frame(&page.image, f.x, f.y, f.rotated));
            }
        }
        sheets
    }

    // --- Recolour ---------------------------------------------------------------

    /// Recolor…: finds the paint palette of all pages and opens the window.
    pub fn open_recolor(&mut self) {
        if self.pages.is_empty() {
            return;
        }
        self.enhance = None;
        self.enhance_textures.clear();
        let (swatches, paper) = self.recolor_palette();
        if swatches.is_empty() {
            self.status = Some("No paint colours found on these pages".to_owned());
            return;
        }
        // Prepare the page previews in the background so the first change
        // previews at once.
        let previews = self.page_previews.clone();
        let sizes: Vec<u32> = self.pages.iter().map(|p| p.image.width()).collect();
        let (sw, tol) = (swatches.clone(), recolor::DEFAULT_TOLERANCE);
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("recolor-prepare".to_owned())
            .spawn(move || {
                let prepared = previews
                    .iter()
                    .zip(sizes)
                    .map(|(small, full)| {
                        let r = (mm_to_px(0.5) * small.width() as f32 / full as f32)
                            .round()
                            .max(1.0) as usize;
                        recolor::prepare(small, &sw, paper, tol, r)
                    })
                    .collect();
                let _ = tx.send(prepared);
            })
            .expect("spawn recolor prepare");
        self.recolor_preparing = Some(rx);
        self.recolor = Some(RecolorDialog {
            rc: Recolor::new(swatches, paper),
            selected: None,
            protect_mode: false,
            area_mode: false,
        });
        self.recolor_textures.clear();
        self.recolor_prepared.clear();
    }

    /// Marks the recolour preview as changed.
    pub fn recolor_changed(&mut self) {
        self.recolor_textures.clear();
    }

    /// The paint palette (and paper colour): of the drawn areas when there
    /// are any, else of all pages.
    fn recolor_palette(&self) -> (Vec<recolor::Swatch>, Option<[u8; 3]>) {
        let paper = recolor::paper_of(&self.pages[0].image);
        if self.areas.is_empty() {
            let refs: Vec<&RgbaImage> = self.pages.iter().map(|p| &p.image).collect();
            return (recolor::palette(&refs, paper), paper);
        }
        let crops: Vec<RgbaImage> = (0..self.pages.len())
            .filter_map(|i| self.area_crop(i))
            .collect();
        let refs: Vec<&RgbaImage> = crops.iter().collect();
        (recolor::palette(&refs, paper), paper)
    }

    /// The bounding box of page `i`'s areas, transparent outside them.
    fn area_crop(&self, i: usize) -> Option<RgbaImage> {
        let img = &self.pages.get(i)?.image;
        let polys: Vec<&Vec<(f32, f32)>> = self
            .areas
            .iter()
            .filter(|(p, _)| *p == i)
            .map(|(_, a)| a)
            .collect();
        let points = polys.iter().flat_map(|p| p.iter());
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in points {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        let x0 = x0.floor().clamp(0.0, img.width() as f32) as u32;
        let y0 = y0.floor().clamp(0.0, img.height() as f32) as u32;
        let x1 = x1.ceil().clamp(0.0, img.width() as f32) as u32;
        let y1 = y1.ceil().clamp(0.0, img.height() as f32) as u32;
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let mut mask = GrayImage::new(x1 - x0, y1 - y0);
        for poly in polys {
            let shifted: Vec<(f32, f32)> = poly
                .iter()
                .map(|&(x, y)| (x - x0 as f32, y - y0 as f32))
                .collect();
            fill_polygon(&mut mask, &shifted);
        }
        let mut crop = image::imageops::crop_imm(img, x0, y0, x1 - x0, y1 - y0).to_image();
        for (p, m) in crop.pixels_mut().zip(mask.pixels()) {
            if m[0] == 0 {
                p[3] = 0;
            }
        }
        Some(crop)
    }

    /// Pixels of page `i` (at `w`×`h`) that recolouring must not change:
    /// protected ones and, once areas are drawn, everything outside them.
    fn blocked_mask(&self, i: usize, w: u32, h: u32) -> Option<GrayImage> {
        let protect = self.protect.get(i).cloned().flatten().map(|m| {
            if m.dimensions() == (w, h) {
                m
            } else {
                image::imageops::resize(&m, w, h, image::imageops::FilterType::Nearest)
            }
        });
        if self.areas.is_empty() {
            return protect;
        }
        let s = w as f32 / self.pages.get(i)?.image.width() as f32;
        let mut allowed = GrayImage::new(w, h);
        for (_, poly) in self.areas.iter().filter(|(p, _)| *p == i) {
            let scaled: Vec<(f32, f32)> = poly.iter().map(|&(x, y)| (x * s, y * s)).collect();
            fill_polygon(&mut allowed, &scaled);
        }
        for p in allowed.pixels_mut() {
            p[0] = if p[0] > 0 { 0 } else { 255 };
        }
        if let Some(m) = protect {
            for (a, b) in allowed.pixels_mut().zip(m.pixels()) {
                a[0] = a[0].max(b[0]);
            }
        }
        Some(allowed)
    }

    /// Recolour Area: adds a drawn area (page pixels) to page `page`; tiny
    /// ones (under 2 mm across) are ignored.
    pub fn add_area(&mut self, page: usize, poly: Vec<(f32, f32)>) -> bool {
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
        self.areas.push((page, poly));
        self.areas_changed();
        true
    }

    /// Clear Areas: whole pages are recoloured again.
    pub fn clear_areas(&mut self) {
        if self.areas.is_empty() {
            return;
        }
        self.areas.clear();
        self.areas_changed();
    }

    /// The palette follows the areas; chosen colours stay with matching
    /// paints.
    fn areas_changed(&mut self) {
        self.recolor_textures.clear();
        if self.recolor.is_none() {
            return;
        }
        let (swatches, _) = self.recolor_palette();
        let Some(dialog) = &mut self.recolor else {
            return;
        };
        let old = &dialog.rc;
        let targets: Vec<Option<[u8; 3]>> = swatches
            .iter()
            .map(|s| {
                old.swatches
                    .iter()
                    .zip(&old.targets)
                    .filter(|(o, t)| t.is_some() && delta_e(o.lab, s.lab) < recolor::MERGE_DE)
                    .min_by(|a, b| delta_e(a.0.lab, s.lab).total_cmp(&delta_e(b.0.lab, s.lab)))
                    .and_then(|(_, t)| *t)
            })
            .collect();
        if swatches.is_empty() {
            self.status = Some("No paint colours inside the areas".to_owned());
        }
        dialog.rc.swatches = swatches;
        dialog.rc.targets = targets;
        dialog.selected = None;
        self.recolor_preparing = None;
        self.recolor_prepared.clear();
    }

    /// Adds a MobileSAM mask (page-sized) to a page's protected area.
    pub fn add_protect(&mut self, page: usize, mask: &GrayImage) {
        if self.protect.len() < self.pages.len() {
            self.protect.resize(self.pages.len(), None);
        }
        let Some(p) = self.pages.get(page) else {
            return;
        };
        if mask.dimensions() != p.image.dimensions() {
            return;
        }
        let slot = &mut self.protect[page];
        match slot {
            Some(m) => {
                for (a, b) in m.pixels_mut().zip(mask.pixels()) {
                    a[0] = a[0].max(b[0]);
                }
            }
            None => *slot = Some(mask.clone()),
        }
        self.protect_textures.clear();
        self.recolor_textures.clear();
        self.status = Some(format!(
            "Page {}: area protected from recolouring",
            page + 1
        ));
    }

    pub fn clear_protect(&mut self) {
        self.protect.clear();
        self.protect_textures.clear();
        self.recolor_textures.clear();
    }

    /// The swatch under a page point (its own colour, else its surroundings).
    pub fn swatch_at(&self, page: usize, point: (f32, f32)) -> Option<usize> {
        let dialog = self.recolor.as_ref()?;
        let img = &self.pages.get(page)?.image;
        let (x, y) = (point.0 as i64, point.1 as i64);
        if x < 0 || y < 0 || x >= img.width() as i64 || y >= img.height() as i64 {
            return None;
        }
        let r = (mm_to_px(0.5) as i64).max(1);
        let x0 = (x - r).max(0) as u32;
        let y0 = (y - r).max(0) as u32;
        let x1 = ((x + r) as u32).min(img.width() - 1);
        let y1 = ((y + r) as u32).min(img.height() - 1);
        let patch = image::imageops::crop_imm(img, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image();
        let member = recolor::membership(
            &patch,
            &dialog.rc.swatches,
            dialog.rc.paper,
            dialog.rc.tolerance,
            r as usize,
        );
        let k = (y as u32 - y0) as usize * patch.width() as usize + (x as u32 - x0) as usize;
        member
            .get(k)
            .copied()
            .filter(|m| *m != u8::MAX)
            .map(|m| m as usize)
    }

    /// Apply: recolours every page at full resolution on a worker thread.
    pub fn start_recolor(&mut self, notify: Arc<dyn Fn() + Send + Sync>) -> bool {
        let Some(dialog) = &self.recolor else {
            return false;
        };
        if self.page_job.is_some() || dialog.rc.is_identity() {
            return false;
        }
        let rc = dialog.rc.clone();
        let pages: Vec<RgbaImage> = self.pages.iter().map(|p| p.image.clone()).collect();
        let protect: Vec<Option<GrayImage>> = pages
            .iter()
            .enumerate()
            .map(|(i, p)| self.blocked_mask(i, p.width(), p.height()))
            .collect();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = cancel.clone();
        let total = pages.len();
        std::thread::Builder::new()
            .name("recolor".to_owned())
            .spawn(move || {
                let r = mm_to_px(0.5).round() as usize;
                let mut out = Vec::with_capacity(pages.len());
                for (i, page) in pages.iter().enumerate() {
                    if stop.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = tx.send(RecolorEvent::Finished(None));
                        notify();
                        return;
                    }
                    out.push(recolor::apply(page, &rc, r, protect[i].as_ref()));
                    let _ = tx.send(RecolorEvent::Progress(i + 1, total));
                    notify();
                }
                let _ = tx.send(RecolorEvent::Finished(Some(out)));
                notify();
            })
            .expect("spawn recolor");
        self.page_job = Some(RecolorJob {
            rx,
            cancel,
            done: 0,
            total,
        });
        self.page_job_kind = JobKind::Recolor;
        self.status = Some(format!("Recolouring {total} page(s)…"));
        true
    }

    /// Progress of a running recolour (pages done, total).
    pub fn recolor_progress(&self) -> Option<(usize, usize)> {
        self.page_job.as_ref().map(|j| (j.done, j.total))
    }

    pub fn cancel_recolor(&mut self) {
        if let Some(job) = &self.page_job {
            job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn pump_recolor(&mut self) -> bool {
        if let Some(rx) = &self.recolor_preparing {
            if let Ok(prepared) = rx.try_recv() {
                self.recolor_prepared = prepared.into_iter().map(Some).collect();
                self.recolor_preparing = None;
            }
        }
        let Some(job) = &mut self.page_job else {
            return false;
        };
        let mut finished = None;
        let mut changed = false;
        while let Ok(e) = job.rx.try_recv() {
            changed = true;
            match e {
                RecolorEvent::Progress(d, t) => {
                    job.done = d;
                    job.total = t;
                }
                RecolorEvent::Finished(r) => finished = Some(r),
            }
        }
        if let Some(result) = finished {
            self.page_job = None;
            match result {
                Some(new_pages) => {
                    if self.original_pages.is_none() {
                        self.original_pages =
                            Some(self.pages.iter().map(|p| p.image.clone()).collect());
                    }
                    self.set_page_images(new_pages);
                    self.enhance_applied = self.page_job_kind == JobKind::Enhance;
                    self.status = Some(
                        match self.page_job_kind {
                            JobKind::Recolor => "Recoloured; parts and sheets keep their places",
                            JobKind::Enhance => {
                                "Lines enhanced; parts and sheets keep their places"
                            }
                        }
                        .to_owned(),
                    );
                }
                None => {
                    self.status = Some(
                        match self.page_job_kind {
                            JobKind::Recolor => "Recolour cancelled",
                            JobKind::Enhance => "Enhance cancelled",
                        }
                        .to_owned(),
                    )
                }
            }
        }
        changed
    }

    /// Whether the pages were recoloured (Revert is possible).
    pub fn recolored(&self) -> bool {
        self.original_pages.is_some()
    }

    /// Revert: the original page colours come back.
    pub fn revert_recolor(&mut self) -> bool {
        let Some(original) = self.original_pages.take() else {
            return false;
        };
        self.set_page_images(original);
        self.enhance_applied = false;
        self.status = Some("Original colours restored".to_owned());
        true
    }

    /// Update Parts Layout Page: `image` (edited in the editor) replaces
    /// page `page`, the parts are re-cut without re-packing, and Revert to
    /// Original still restores the first images.
    pub fn set_edited_page(&mut self, page: usize, image: RgbaImage) -> Result<(), String> {
        let current = self
            .pages
            .get(page)
            .ok_or_else(|| format!("Page {} is no longer there", page + 1))?;
        if current.image.dimensions() != image.dimensions() {
            let (w, h) = current.image.dimensions();
            return Err(format!(
                "The canvas is {}×{} but the page is {w}×{h}",
                image.width(),
                image.height()
            ));
        }
        if self.page_job.is_some() {
            return Err("Wait for the recolour to finish".to_owned());
        }
        let mut images: Vec<RgbaImage> = self.pages.iter().map(|p| p.image.clone()).collect();
        if self.original_pages.is_none() {
            self.original_pages = Some(images.clone());
        }
        images[page] = image;
        self.set_page_images(images);
        self.status = Some(format!(
            "Page {} updated from the editor; parts keep their places",
            page + 1
        ));
        Ok(())
    }

    /// Replaces the page pixels (same sizes) and re-cuts the parts, keeping
    /// outlines, splits, and placements.
    fn set_page_images(&mut self, images: Vec<RgbaImage>) {
        self.page_version += 1;
        for (page, img) in self.pages.iter_mut().zip(images) {
            page.image = img;
        }
        self.page_previews = self.pages.iter().map(|p| preview(&p.image)).collect();
        self.page_textures = vec![None; self.pages.len()];
        self.recolor_textures.clear();
        self.recolor_prepared.clear();
        self.enhance_textures.clear();
        self.recut();
    }

    /// Re-cuts every part from the current pages without re-packing: the
    /// items are rebuilt from the proposals, the recorded splits replayed,
    /// and the placements restored.
    fn recut(&mut self) {
        for (k, part) in self.proposals.iter().enumerate() {
            let img = part.image(&self.pages[part.page].image);
            self.part_previews[k] = preview(&img);
            self.part_images[k] = img;
        }
        let placements = self.layout.placements.clone();
        let sheets = self.layout.sheets;
        let keep: Vec<usize> = (0..self.proposals.len())
            .filter(|&k| {
                !self
                    .manual
                    .get(self.proposals[k].page)
                    .copied()
                    .unwrap_or(false)
            })
            .collect();
        let items: Vec<Item> = keep
            .iter()
            .map(|&k| Item {
                label: format!("{}.{}", self.proposals[k].page + 1, k + 1),
                image: self.part_images[k].clone(),
            })
            .collect();
        self.layout.items = items;
        self.layout.placements = vec![None; self.layout.items.len()];
        for &(i, a, b, glue, on) in &self.splits {
            self.layout.split(i, a, b, glue, on);
        }
        if placements.len() == self.layout.items.len() {
            self.layout.placements = placements;
        }
        self.layout.sheets = sheets;
        self.item_previews = vec![None; self.layout.items.len()];
        self.generation += 1;
        self.item_textures.clear();
    }

    fn recolor_texture(&mut self, ctx: &egui::Context, i: usize) -> Option<TextureHandle> {
        let dialog = self.recolor.as_ref()?;
        if dialog.rc.is_identity() {
            return None;
        }
        if self.recolor_textures.len() < self.pages.len() {
            self.recolor_textures.resize(self.pages.len(), None);
        }
        if self.recolor_textures[i].is_none() {
            let small = self.page_previews.get(i)?;
            let (w, h) = small.dimensions();
            let scale = w as f32 / self.pages[i].image.width() as f32;
            let r = (mm_to_px(0.5) * scale).round().max(1.0) as usize;
            let protect = self.blocked_mask(i, w, h);
            if let Some(rx) = &self.recolor_preparing {
                // Take the background preparation if it is done (or wait
                // for it: it is about to be needed anyway).
                if let Ok(prepared) = rx.recv() {
                    self.recolor_prepared = prepared.into_iter().map(Some).collect();
                }
                self.recolor_preparing = None;
            }
            if self.recolor_prepared.len() < self.pages.len() {
                self.recolor_prepared.resize(self.pages.len(), None);
            }
            let stale = self.recolor_prepared[i]
                .as_ref()
                .is_none_or(|p| p.tolerance != dialog.rc.tolerance);
            if stale {
                self.recolor_prepared[i] = Some(recolor::prepare(
                    small,
                    &dialog.rc.swatches,
                    dialog.rc.paper,
                    dialog.rc.tolerance,
                    r,
                ));
            }
            let prep = self.recolor_prepared[i].as_ref().expect("just prepared");
            let out = recolor::apply_prepared(small, prep, &dialog.rc, protect.as_ref());
            let c =
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], out.as_raw());
            self.recolor_textures[i] =
                Some(ctx.load_texture(format!("pdf-recolor-{i}"), c, TextureOptions::LINEAR));
        }
        self.recolor_textures[i].clone()
    }

    fn protect_texture(&mut self, ctx: &egui::Context, i: usize) -> Option<TextureHandle> {
        let mask = self.protect.get(i)?.as_ref()?;
        if self.protect_textures.len() < self.pages.len() {
            self.protect_textures.resize(self.pages.len(), None);
        }
        if self.protect_textures[i].is_none() {
            let (w, h) = self.page_previews[i].dimensions();
            let small = image::imageops::resize(mask, w, h, image::imageops::FilterType::Nearest);
            let tint = RgbaImage::from_fn(w, h, |x, y| {
                if small.get_pixel(x, y)[0] > 0 && (x + y) % 6 < 3 {
                    image::Rgba([230, 40, 160, 150])
                } else {
                    image::Rgba([0, 0, 0, 0])
                }
            });
            let c =
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], tint.as_raw());
            self.protect_textures[i] =
                Some(ctx.load_texture(format!("pdf-protect-{i}"), c, TextureOptions::NEAREST));
        }
        self.protect_textures[i].clone()
    }

    fn show_recolor_window(&mut self, ctx: &egui::Context, actions: &mut Vec<PartsAction>) {
        let Some(mut dialog) = self.recolor.take() else {
            return;
        };
        let before = dialog.clone();
        let mut keep = true;
        let mut apply = false;
        let mut revert = false;
        let mut clear = false;
        let mut cancel = false;
        let mut clear_areas = false;
        let areas = self.areas.len();
        let progress = self.recolor_progress();
        let sam = self.sam_ready;
        let recolored = self.recolored();
        egui::Window::new("Recolor Paint")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-24.0, 72.0))
            .show(ctx, |ui| {
                ui.label("Paint colours of the model (click a page to pick one):");
                recolor_controls(ui, &mut dialog.rc, dialog.selected);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(sam, |ui| {
                        if ui
                            .toggle_value(&mut dialog.protect_mode, "Protect (MobileSAM)")
                            .on_disabled_hover_text(
                                "Needs the MobileSAM model (Extract Object downloads it)",
                            )
                            .changed()
                            && dialog.protect_mode
                        {
                            dialog.area_mode = false;
                        }
                    });
                    if ui.button("Clear Protected").clicked() {
                        clear = true;
                    }
                });
                if !sam {
                    ui.weak("Protect needs the MobileSAM model.");
                }
                ui.horizontal(|ui| {
                    if ui
                        .toggle_value(&mut dialog.area_mode, "Recolour Area")
                        .on_hover_text("Drag on a page: rectangle · Shift + drag: lasso")
                        .changed()
                        && dialog.area_mode
                    {
                        dialog.protect_mode = false;
                    }
                    if ui
                        .add_enabled(areas > 0, egui::Button::new("Clear Areas"))
                        .clicked()
                    {
                        clear_areas = true;
                    }
                });
                if areas > 0 {
                    ui.weak(format!(
                        "Only inside {areas} area{} (other pages unchanged)",
                        if areas == 1 { "" } else { "s" }
                    ));
                } else if dialog.area_mode {
                    ui.weak("Drag on a page: rectangle · Shift + drag: lasso");
                }
                ui.separator();
                if let Some((done, total)) = progress {
                    ui.add(
                        egui::ProgressBar::new(done as f32 / total.max(1) as f32).text(format!(
                            "Recolouring page {} of {total}",
                            (done + 1).min(total)
                        )),
                    );
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                } else {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!dialog.rc.is_identity(), egui::Button::new("Apply"))
                            .clicked()
                        {
                            apply = true;
                        }
                        if ui
                            .add_enabled(recolored, egui::Button::new("Revert to Original"))
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
        let changed = dialog != before;
        if keep {
            self.recolor = Some(dialog);
        } else {
            self.recolor_textures.clear();
        }
        if changed {
            self.recolor_changed();
        }
        if clear {
            self.clear_protect();
        }
        if cancel {
            self.cancel_recolor();
        }
        if clear_areas {
            self.clear_areas();
        }
        if revert {
            self.revert_recolor();
        }
        if apply {
            actions.push(PartsAction::ApplyRecolor);
        }
    }

    /// Unplaced items, oversized ones first.
    pub fn tray(&self) -> Vec<usize> {
        let mut t: Vec<usize> = (0..self.layout.items.len())
            .filter(|&i| self.layout.placements[i].is_none())
            .collect();
        t.sort_by_key(|&i| self.layout.items[i].fits());
        t
    }

    fn page_texture(&mut self, ctx: &egui::Context, i: usize) -> Option<TextureHandle> {
        if self.page_textures.get(i)?.is_none() {
            let small = self.page_previews.get(i)?;
            let (w, h) = small.dimensions();
            let c =
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], small.as_raw());
            self.page_textures[i] =
                Some(ctx.load_texture(format!("pdf-page-{i}"), c, TextureOptions::LINEAR));
        }
        self.page_textures[i].clone()
    }

    fn item_texture(&mut self, ctx: &egui::Context, i: usize) -> TextureHandle {
        if self.item_textures.len() < self.layout.items.len() {
            self.item_textures.resize(self.layout.items.len(), None);
        }
        if let Some((g, t)) = &self.item_textures[i] {
            if *g == self.generation {
                return t.clone();
            }
        }
        if self.item_previews.len() < self.layout.items.len() {
            self.item_previews.resize(self.layout.items.len(), None);
        }
        if self.item_previews[i].is_none() {
            self.item_previews[i] = Some(preview(&self.layout.items[i].image));
        }
        let small = self.item_previews[i].as_ref().expect("just made");
        let (w, h) = small.dimensions();
        let c = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], small.as_raw());
        let t = ctx.load_texture(
            format!("pdf-item-{i}-{}", self.generation),
            c,
            TextureOptions::LINEAR,
        );
        self.item_textures[i] = Some((self.generation, t.clone()));
        t
    }

    /// Draws the tab.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Vec<PartsAction> {
        let mut actions = Vec::new();
        self.pump();
        // Toolbar.
        ui.horizontal_wrapped(|ui| {
            if let Some(e) = &self.error {
                ui.colored_label(
                    Color32::from_rgb(220, 80, 60),
                    format!("Could not open: {e}"),
                );
                if self.pages.is_empty() {
                    return;
                }
            }
            if self.loading() {
                ui.spinner();
                ui.label(format!(
                    "Rendering and finding parts… page {} of {}",
                    (self.pages.len() + 1).min(self.page_count.unwrap_or(1)),
                    self.page_count.map_or("?".to_owned(), |n| n.to_string())
                ));
                return;
            }
            let placed = self.layout.placements.iter().flatten().count();
            let frames = self.export_frames().len();
            ui.label(format!(
                "{} parts, {placed} on {} A4 sheet{}{}",
                self.layout.items.len(),
                self.layout.sheets,
                if self.layout.sheets == 1 { "" } else { "s" },
                if frames > 0 {
                    format!(
                        " + {frames} frame sheet{}",
                        if frames == 1 { "" } else { "s" }
                    )
                } else {
                    String::new()
                }
            ));
            ui.separator();
            if ui
                .add_enabled(self.selected.len() >= 2, egui::Button::new("Merge"))
                .clicked()
            {
                self.merge_selected();
            }
            if ui
                .add_enabled(!self.selected.is_empty(), egui::Button::new("Delete"))
                .clicked()
            {
                self.delete_selected();
            }
            ui.add_enabled_ui(self.sam_ready, |ui| {
                ui.toggle_value(&mut self.refine, "Refine (MobileSAM)")
                    .on_disabled_hover_text(
                        "Needs the MobileSAM model (Extract Object downloads it)",
                    );
            });
            ui.separator();
            if ui.button("Re-pack").clicked() {
                self.layout.repack();
            }
            if ui.button("Add Sheet").clicked() {
                self.layout.add_sheet();
            }
            if ui.button("Remove Empty Sheets").clicked() {
                for s in (0..self.layout.sheets).rev() {
                    self.layout.remove_sheet(s);
                }
            }
            if ui.button("Place Tray Items").clicked() {
                self.layout.place_tray();
            }
            let placed_item = self.item.filter(|&i| self.layout.placements[i].is_some());
            if ui
                .add_enabled(placed_item.is_some(), egui::Button::new("Rotate 90°"))
                .clicked()
                && !self.layout.rotate(placed_item.unwrap())
            {
                self.status = Some("No room to turn it there".to_owned());
            }
            if ui
                .add_enabled(placed_item.is_some(), egui::Button::new("To Tray"))
                .clicked()
            {
                self.layout.unplace(placed_item.unwrap());
            }
            if ui
                .add_enabled(self.item.is_some(), egui::Button::new("Split…"))
                .clicked()
            {
                self.split = self.item.map(|item| SplitDialog {
                    item,
                    line: None,
                    glue_mm: split::DEFAULT_GLUE_MM,
                    glue_on: split::GlueOn::Smaller,
                    drag_start: None,
                });
            }
            ui.separator();
            if let Some((done, total)) = self.export_progress() {
                ui.add(
                    egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                        .desired_width(140.0)
                        .text(format!(
                            "Exporting sheet {} of {total}",
                            (done + 1).min(total)
                        )),
                );
                if ui.button("Cancel Export").clicked() {
                    self.cancel_export();
                }
            } else {
                if ui.button("Recolor…").clicked() && self.recolor.is_none() {
                    self.open_recolor();
                }
                if ui.button("Enhance Lines…").clicked() && self.enhance.is_none() {
                    self.open_enhance();
                }
                ui.checkbox(&mut self.pngs, "Also PNGs");
                if ui.button("Export A4 Pages…").clicked() {
                    actions.push(PartsAction::Export { pngs: self.pngs });
                }
            }
        });
        let over = self.layout.oversized();
        if !over.is_empty() && !self.loading() {
            for &i in &over {
                let (w, h) = self.layout.items[i].size_mm();
                ui.colored_label(
                    Color32::from_rgb(230, 150, 30),
                    format!(
                        "⚠ Part {} does not fit on A4: {w:.0} × {h:.0} mm — select it and use Split…",
                        self.layout.items[i].label
                    ),
                );
            }
        }
        if !self.skipped.is_empty() {
            let list: Vec<String> = self
                .skipped
                .iter()
                .map(|(name, reason)| format!("{name} ({reason})"))
                .collect();
            ui.colored_label(
                Color32::from_rgb(230, 150, 30),
                format!(
                    "⚠ Skipped {} unreadable file(s): {}",
                    list.len(),
                    list.join("; ")
                ),
            );
        }
        if let Some(s) = &self.status {
            ui.weak(s);
        }
        ui.separator();
        let ctx = ui.ctx().clone();
        let avail = ui.available_size();
        ui.horizontal_top(|ui| {
            // Source pages.
            let left_w = (avail.x * 0.42).max(160.0);
            let column = egui::Layout::top_down(egui::Align::Min);
            ui.allocate_ui_with_layout(Vec2::new(left_w, avail.y), column, |ui| {
                // Zoom for placing frames precisely.
                ui.horizontal(|ui| {
                    ui.label("Zoom");
                    if ui.small_button("−").clicked() {
                        self.page_zoom = (self.page_zoom / 1.25).max(1.0);
                    }
                    ui.label(format!("{:.0} %", self.page_zoom * 100.0));
                    if ui.small_button("+").clicked() {
                        self.page_zoom = (self.page_zoom * 1.25).min(8.0);
                    }
                    if ui.small_button("Fit").clicked() {
                        self.page_zoom = 1.0;
                    }
                });
                let area = egui::ScrollArea::both()
                    .id_salt("pdf-pages")
                    .show(ui, |ui| {
                        let w = (left_w - 16.0) * self.page_zoom;
                        self.show_pages(ui, &ctx, w, &mut actions)
                    });
                // Cmd/Ctrl + scroll (egui turns it into a zoom delta) or a
                // pinch over the pages zooms.
                let hovered = ui.rect_contains_pointer(area.inner_rect);
                let pinch = ui.input(|i| i.zoom_delta());
                if hovered && (pinch - 1.0).abs() > f32::EPSILON {
                    self.page_zoom = (self.page_zoom * pinch).clamp(1.0, 8.0);
                }
            });
            ui.separator();
            let right_w = (avail.x - left_w - 24.0).max(160.0);
            ui.allocate_ui_with_layout(Vec2::new(right_w, avail.y), column, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("pdf-sheets")
                    .show(ui, |ui| self.show_sheets(ui, &ctx, right_w - 16.0));
            });
        });
        self.show_split_window(&ctx);
        self.show_recolor_window(&ctx, &mut actions);
        self.show_enhance_window(&ctx, &mut actions);
        actions
    }

    fn show_pages(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        width: f32,
        actions: &mut Vec<PartsAction>,
    ) {
        let modifiers = ui.input(|i| i.modifiers);
        self.frame_rects.clear();
        self.page_rects.clear();
        for i in 0..self.pages.len() {
            let (pw, ph) = self.pages[i].image.dimensions();
            let (wmm, hmm) = (self.pages[i].width_mm, self.pages[i].height_mm);
            let mut manual = self.manual[i];
            let source = self.image_pages.get(i).cloned().flatten();
            ui.horizontal_wrapped(|ui| {
                match &source {
                    Some((file, size)) => {
                        ui.label(format!(
                            "Page {}  —  {wmm:.0} × {hmm:.0} mm ({})",
                            i + 1,
                            size.describe()
                        ))
                        .on_hover_text(file.display().to_string());
                        ui.add(
                            egui::DragValue::new(&mut self.dpi_edit[i])
                                .range(50.0..=4800.0)
                                .suffix(" dpi"),
                        );
                        if ui
                            .add_enabled(!self.reloading(), egui::Button::new("Apply DPI"))
                            .clicked()
                        {
                            actions.push(PartsAction::ReloadPage {
                                page: i,
                                dpi: self.dpi_edit[i],
                            });
                        }
                    }
                    None => {
                        ui.label(format!("Page {}  —  {wmm:.0} × {hmm:.0} mm", i + 1));
                    }
                }
                ui.checkbox(&mut manual, "Manual (A4 frames)")
                    .on_hover_text(
                        "Print this page through A4 frames you place, instead of packing its parts",
                    );
                if self.manual[i] {
                    if ui.button("Add Frame").clicked() {
                        self.add_frame(i);
                    }
                    let sel = self
                        .frame
                        .filter(|&f| self.frames.get(f).is_some_and(|f| f.page == i));
                    if ui
                        .add_enabled(sel.is_some(), egui::Button::new("Rotate Frame"))
                        .clicked()
                    {
                        self.rotate_frame(sel.unwrap());
                    }
                    if ui
                        .add_enabled(sel.is_some(), egui::Button::new("Remove Frame"))
                        .clicked()
                    {
                        self.remove_frame(sel.unwrap());
                    }
                }
            });
            if manual != self.manual[i] {
                self.set_manual(i, manual);
            }
            let scale = width / pw as f32;
            let (rect, response) = ui
                .allocate_exact_size(Vec2::new(width, ph as f32 * scale), Sense::click_and_drag());
            self.page_rects.push((i, rect, scale));
            let shown = self
                .enhance_texture(ctx, i)
                .or_else(|| self.recolor_texture(ctx, i))
                .or_else(|| self.page_texture(ctx, i));
            if let Some(t) = shown {
                ui.painter().image(
                    t.id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            if let Some(t) = self.protect_texture(ctx, i) {
                ui.painter().image(
                    t.id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            // Recolour areas (and the one being drawn).
            let drawing = self.recolor.as_ref().is_some_and(|d| d.area_mode)
                || self.enhance.as_ref().is_some_and(|d| d.area_mode);
            let to_screen = |&(x, y): &(f32, f32)| rect.min + Vec2::new(x, y) * scale;
            let to_page = |p: Pos2| {
                let q = (p - rect.min) / scale;
                (q.x, q.y)
            };
            if drawing {
                if response.drag_started() {
                    let origin = ui.input(|inp| inp.pointer.press_origin());
                    if let Some(o) = origin.or(response.interact_pointer_pos()) {
                        self.area_drag = Some((i, vec![to_page(o)], modifiers.shift));
                    }
                }
                if let (Some((k, points, lasso)), Some(p), true) = (
                    &mut self.area_drag,
                    response.interact_pointer_pos(),
                    response.dragged(),
                ) {
                    if *k == i {
                        if !*lasso {
                            points.truncate(1);
                        }
                        points.push(to_page(p));
                    }
                }
                if response.drag_stopped() {
                    if let Some((k, points, lasso)) = self.area_drag.take() {
                        if k == i {
                            let poly = area_polygon(points, lasso);
                            if self.enhance.is_some() {
                                self.add_enhance_area(i, poly);
                            } else {
                                self.add_area(i, poly);
                            }
                        }
                    }
                }
            }
            // The open window's areas: Enhance Lines (orange) or Recolour (blue).
            let (shown_areas, colour) = if self.enhance.is_some() {
                (&self.enhance_areas, Color32::from_rgb(235, 110, 20))
            } else {
                (&self.areas, Color32::from_rgb(40, 140, 255))
            };
            let area_stroke = Stroke::new(2.0, colour);
            for (_, poly) in shown_areas.iter().filter(|(p, _)| *p == i) {
                let pts: Vec<Pos2> = poly.iter().map(to_screen).collect();
                ui.painter().add(egui::Shape::closed_line(pts, area_stroke));
            }
            if let Some((k, points, lasso)) = &self.area_drag {
                if *k == i {
                    let poly = area_polygon(points.clone(), *lasso);
                    let pts: Vec<Pos2> = poly.iter().map(to_screen).collect();
                    ui.painter().add(egui::Shape::closed_line(
                        pts,
                        Stroke::new(1.5, Color32::from_rgb(255, 150, 0)),
                    ));
                }
            }
            let is_manual = self.manual[i];
            for (k, part) in self
                .proposals
                .iter()
                .enumerate()
                .filter(|(_, p)| p.page == i && !is_manual)
            {
                let r = Rect::from_min_size(
                    rect.min + Vec2::new(part.rect.x as f32, part.rect.y as f32) * scale,
                    Vec2::new(part.rect.width as f32, part.rect.height as f32) * scale,
                );
                let sel = self.selected.contains(&k);
                let color = if sel {
                    Color32::from_rgb(40, 140, 255)
                } else {
                    Color32::from_rgb(255, 170, 0)
                };
                ui.painter().rect_stroke(
                    r,
                    0.0,
                    Stroke::new(if sel { 2.5 } else { 1.5 }, color),
                    egui::StrokeKind::Outside,
                );
                ui.painter().text(
                    r.left_top() + Vec2::new(2.0, 1.0),
                    egui::Align2::LEFT_TOP,
                    format!("{}", k + 1),
                    egui::FontId::proportional(11.0),
                    color,
                );
            }
            if is_manual {
                // Frames: drawn over the page, dragged to move, clicked to select.
                let mut clicked_frame = false;
                for (fi, f) in self
                    .frames
                    .iter_mut()
                    .enumerate()
                    .filter(|(_, f)| f.page == i)
                {
                    let (fw, fh) = f.size();
                    let r = Rect::from_min_size(
                        rect.min + Vec2::new(f.x as f32, f.y as f32) * scale,
                        Vec2::new(fw as f32, fh as f32) * scale,
                    );
                    let selected = self.frame == Some(fi);
                    let color = if selected {
                        Color32::from_rgb(40, 140, 255)
                    } else {
                        Color32::from_rgb(120, 90, 220)
                    };
                    ui.painter().rect_filled(r, 0.0, color.gamma_multiply(0.12));
                    ui.painter().rect_stroke(
                        r,
                        0.0,
                        Stroke::new(if selected { 2.5 } else { 1.5 }, color),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        r.left_top() + Vec2::new(4.0, 3.0),
                        egui::Align2::LEFT_TOP,
                        format!("A4 {}", fi + 1),
                        egui::FontId::proportional(12.0),
                        color,
                    );
                    let resp =
                        ui.interact(r, ui.id().with(("pdf-frame", fi)), Sense::click_and_drag());
                    if resp.clicked() || resp.drag_started() {
                        self.frame = Some(fi);
                        clicked_frame = true;
                    }
                    if resp.drag_started() {
                        // Measure from the press: the frame follows the
                        // pointer exactly at any zoom (no per-frame rounding).
                        let origin = ui.input(|inp| inp.pointer.press_origin());
                        if let Some(o) = origin.or(resp.interact_pointer_pos()) {
                            self.frame_drag = Some((fi, (f.x, f.y), o));
                        }
                    }
                    if resp.dragged() {
                        if let (Some((k, start, origin)), Some(p)) =
                            (self.frame_drag, ui.ctx().pointer_latest_pos())
                        {
                            if k == fi {
                                f.x = start.0 + ((p.x - origin.x) / scale).round() as i32;
                                f.y = start.1 + ((p.y - origin.y) / scale).round() as i32;
                            }
                        }
                    }
                    if resp.drag_stopped() {
                        self.frame_drag = None;
                    }
                    self.frame_rects.push((fi, r));
                }
                if response.clicked() && !clicked_frame {
                    self.frame = None;
                }
            } else if response.clicked() && !drawing {
                if let Some(pos) = response.interact_pointer_pos() {
                    let p = ((pos - rect.min) / scale).to_pos2();
                    let protecting = self.recolor.as_ref().is_some_and(|d| d.protect_mode);
                    if protecting {
                        actions.push(PartsAction::Protect {
                            page: i,
                            point: (p.x, p.y),
                        });
                    } else if self.recolor.is_some() {
                        let picked = self.swatch_at(i, (p.x, p.y));
                        if let Some(d) = &mut self.recolor {
                            d.selected = picked;
                        }
                    } else if self.refine {
                        actions.push(PartsAction::Refine {
                            page: i,
                            point: (p.x, p.y),
                        });
                    } else {
                        let hit = self.proposal_at(i, (p.x, p.y));
                        match hit {
                            Some(k) if modifiers.command => {
                                if !self.selected.remove(&k) {
                                    self.selected.insert(k);
                                }
                            }
                            Some(k) => self.selected = [k].into(),
                            None => self.selected.clear(),
                        }
                    }
                }
            }
            response.context_menu(|ui| {
                if ui.button("Open Page in Editor").clicked() {
                    actions.push(PartsAction::OpenInEditor { page: i });
                    ui.close();
                }
            });
            ui.add_space(8.0);
        }
    }

    fn show_sheets(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, width: f32) {
        let (sw, sh) = crate::pdf::layout::sheet_px();
        let margin = mm_to_px(pack::MARGIN_MM).round();
        let cols = if width > 520.0 { 2 } else { 1 };
        let sheet_w = (width - 12.0 * (cols as f32 - 1.0)) / cols as f32;
        let scale = sheet_w / sw as f32;
        let pointer = ui.ctx().pointer_latest_pos();
        let released = ui.input(|i| i.pointer.any_released());
        let mut sheet_rects = Vec::new();
        let sheets = self.layout.sheets;
        let mut s = 0;
        while s < sheets {
            ui.horizontal_top(|ui| {
                for _ in 0..cols {
                    if s >= sheets {
                        break;
                    }
                    ui.vertical(|ui| {
                        ui.label(format!("A4 sheet {}", s + 1));
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::new(sheet_w, sh as f32 * scale),
                            Sense::hover(),
                        );
                        ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
                        ui.painter().rect_stroke(
                            rect.shrink(margin * scale),
                            0.0,
                            Stroke::new(1.0, Color32::from_gray(220)),
                            egui::StrokeKind::Inside,
                        );
                        sheet_rects.push((s, rect));
                    });
                    s += 1;
                }
            });
            ui.add_space(8.0);
        }
        self.sheet_rects = sheet_rects.clone();
        self.item_rects.clear();
        // Parts on the sheets.
        for i in 0..self.layout.items.len() {
            let Some(p) = self.layout.placements[i] else {
                continue;
            };
            let Some(&(_, rect)) = sheet_rects.iter().find(|(s, _)| *s == p.sheet) else {
                continue;
            };
            let (x, y, w, h) = p.rect(self.layout.items[i].size());
            let r = Rect::from_min_size(
                rect.min + Vec2::new(margin + x as f32, margin + y as f32) * scale,
                Vec2::new(w as f32, h as f32) * scale,
            );
            self.item_rects.push((i, r));
            let t = self.item_texture(ctx, i);
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            if p.rotated {
                // Draw turned 90° clockwise.
                let mut mesh = egui::Mesh::with_texture(t.id());
                let c = [
                    r.right_top(),
                    r.right_bottom(),
                    r.left_bottom(),
                    r.left_top(),
                ];
                let uvs = [
                    uv.left_top(),
                    uv.right_top(),
                    uv.right_bottom(),
                    uv.left_bottom(),
                ];
                for k in 0..4 {
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: c[k],
                        uv: uvs[k],
                        color: Color32::WHITE,
                    });
                }
                mesh.indices.extend([0, 1, 2, 0, 2, 3]);
                ui.painter().add(egui::Shape::mesh(mesh));
            } else {
                ui.painter().image(t.id(), r, uv, Color32::WHITE);
            }
            let id = ui.id().with(("pdf-item", i));
            let resp = ui.interact(r, id, Sense::click_and_drag());
            if resp.clicked() {
                self.item = Some(i);
            }
            if resp.drag_started() {
                // The grab point is where the press began (the drag starts a
                // few points later).
                let origin = ui.input(|inp| inp.pointer.press_origin());
                if let Some(pos) = origin.or(resp.interact_pointer_pos()) {
                    self.item = Some(i);
                    self.drag = Some(SheetDrag {
                        item: i,
                        grab: ((pos.x - r.min.x) / scale, (pos.y - r.min.y) / scale),
                    });
                }
            }
            if self.item == Some(i) {
                ui.painter().rect_stroke(
                    r,
                    0.0,
                    Stroke::new(2.0, Color32::from_rgb(40, 140, 255)),
                    egui::StrokeKind::Outside,
                );
            }
        }
        // Tray.
        ui.separator();
        ui.label("Tray");
        let tray = self.tray();
        if tray.is_empty() {
            ui.weak("Every part is on a sheet.");
        }
        ui.horizontal_wrapped(|ui| {
            for i in tray {
                let item_fits = self.layout.items[i].fits();
                let (w, h) = self.layout.items[i].size();
                let s = (90.0 / w.max(h) as f32).min(scale * 2.0);
                ui.vertical(|ui| {
                    let (r, resp) = ui.allocate_exact_size(
                        Vec2::new(w as f32 * s, h as f32 * s).max(Vec2::splat(20.0)),
                        Sense::click_and_drag(),
                    );
                    let t = self.item_texture(ctx, i);
                    ui.painter().rect_filled(r, 0.0, Color32::WHITE);
                    ui.painter().image(
                        t.id(),
                        r,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    if resp.clicked() {
                        self.item = Some(i);
                    }
                    if resp.drag_started() && item_fits {
                        self.item = Some(i);
                        self.drag = Some(SheetDrag {
                            item: i,
                            grab: (0.0, 0.0),
                        });
                    }
                    let (wm, hm) = self.layout.items[i].size_mm();
                    let label = if item_fits {
                        format!("{}  {wm:.0}×{hm:.0} mm", self.layout.items[i].label)
                    } else {
                        format!("⚠ {}  {wm:.0}×{hm:.0} mm", self.layout.items[i].label)
                    };
                    ui.small(label);
                    if self.item == Some(i) {
                        ui.painter().rect_stroke(
                            r,
                            0.0,
                            Stroke::new(2.0, Color32::from_rgb(40, 140, 255)),
                            egui::StrokeKind::Outside,
                        );
                    }
                });
            }
        });
        // Drop.
        if let (Some(drag), true) = (self.drag, released) {
            self.drag = None;
            if let Some(pos) = pointer {
                if let Some(&(sheet, rect)) = sheet_rects.iter().find(|(_, r)| r.contains(pos)) {
                    let x = (pos.x - rect.min.x) / scale - margin - drag.grab.0;
                    let y = (pos.y - rect.min.y) / scale - margin - drag.grab.1;
                    let rotated = self.layout.placements[drag.item].is_some_and(|p| p.rotated);
                    let place = Placement {
                        sheet,
                        x: x.max(0.0).round() as u32,
                        y: y.max(0.0).round() as u32,
                        rotated,
                    };
                    // Snap to free space within 3 mm.
                    let snap = mm_to_px(3.0) as u32;
                    if !self.layout.move_item_near(drag.item, place, snap) {
                        self.status = Some(
                            "It does not fit there (overlap or outside the printable area)"
                                .to_owned(),
                        );
                    }
                }
            }
        }
    }

    fn show_split_window(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.split.take() else {
            return;
        };
        let Some(item) = self.layout.items.get(dialog.item) else {
            return;
        };
        let (w, h) = item.size();
        let (wmm, hmm) = item.size_mm();
        let label = item.label.clone();
        let texture = self.item_texture(ctx, dialog.item);
        let mut keep = true;
        let mut apply = None;
        egui::Window::new(format!("Split Part {label}"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!(
                    "{wmm:.0} × {hmm:.0} mm. Drag a straight cut line across the part."
                ));
                let s = (520.0 / w as f32).min(360.0 / h as f32);
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(w as f32 * s, h as f32 * s), Sense::drag());
                ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
                ui.painter().image(
                    texture.id(),
                    rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
                let to_item = |p: Pos2| ((p.x - rect.min.x) / s, (p.y - rect.min.y) / s);
                if resp.drag_started() {
                    dialog.drag_start = resp.interact_pointer_pos().map(to_item);
                }
                if resp.dragged() {
                    if let (Some(a), Some(p)) = (dialog.drag_start, resp.interact_pointer_pos()) {
                        dialog.line = Some((a, to_item(p)));
                    }
                }
                if let Some((a, b)) = dialog.line {
                    let to_screen = |p: (f32, f32)| rect.min + Vec2::new(p.0, p.1) * s;
                    ui.painter().line_segment(
                        [to_screen(a), to_screen(b)],
                        Stroke::new(2.0, Color32::from_rgb(220, 40, 40)),
                    );
                    // Mark the two pieces "1" (side ≥ 0) and "2".
                    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
                    let (nx, ny) = (-dy / len, dx / len);
                    let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                    let off = 22.0 / s;
                    for (k, sign) in [("1", 1.0f32), ("2", -1.0)] {
                        let p = (mid.0 + nx * off * sign, mid.1 + ny * off * sign);
                        // p is on piece k's side (split::side_of ≥ 0 is piece 1).
                        ui.painter().text(
                            to_screen(p),
                            egui::Align2::CENTER_CENTER,
                            k,
                            egui::FontId::proportional(16.0),
                            Color32::from_rgb(220, 40, 40),
                        );
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("Glue strip");
                    ui.add(egui::Slider::new(&mut dialog.glue_mm, 0.0..=20.0).suffix(" mm"));
                });
                ui.horizontal(|ui| {
                    ui.label("Strip on");
                    ui.selectable_value(
                        &mut dialog.glue_on,
                        split::GlueOn::Smaller,
                        "Smaller piece",
                    );
                    ui.selectable_value(&mut dialog.glue_on, split::GlueOn::First, "Piece 1");
                    ui.selectable_value(&mut dialog.glue_on, split::GlueOn::Second, "Piece 2");
                });
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                    if ui
                        .add_enabled(dialog.line.is_some(), egui::Button::new("Split"))
                        .clicked()
                    {
                        apply = dialog.line;
                    }
                });
            });
        if let Some((a, b)) = apply {
            // Extend the drawn segment across the whole part.
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len > 1.0 {
                let k = (w + h) as f32 * 2.0 / len;
                let a2 = (a.0 - dx * k, a.1 - dy * k);
                let b2 = (b.0 + dx * k, b.1 + dy * k);
                if self.split_item_on(dialog.item, a2, b2, dialog.glue_mm, dialog.glue_on) {
                    keep = false;
                }
            }
        }
        if keep {
            self.split = Some(dialog);
        }
    }

    /// Writes the A4 sheets: packed sheets, then frame sheets.
    pub fn export(&self, path: &Path) -> Result<Vec<PathBuf>, String> {
        layout::export_sheets(&self.sheets(), path, self.pngs)
    }

    /// Size of item `i` in mm, for messages.
    pub fn item_mm(&self, i: usize) -> Option<(f32, f32)> {
        self.layout.items.get(i).map(Item::size_mm)
    }
}

/// Longest side of an on-screen texture (GPUs limit texture sizes; 2048 is
/// safe everywhere).
const MAX_TEXTURE_SIDE: f32 = 2048.0;

/// A `PREVIEW_SCALE` copy for on-screen textures, never larger than
/// `MAX_TEXTURE_SIDE` on its longest side.
fn preview(img: &RgbaImage) -> RgbaImage {
    let longest = img.width().max(img.height()) as f32;
    let scale = PREVIEW_SCALE.min(MAX_TEXTURE_SIDE / longest);
    let w = ((img.width() as f32 * scale) as u32).clamp(1, MAX_TEXTURE_SIDE as u32);
    let h = ((img.height() as f32 * scale) as u32).clamp(1, MAX_TEXTURE_SIDE as u32);
    image::imageops::thumbnail(img, w, h)
}

/// An area being drawn: page, points (page pixels), lasso.
type AreaDrag = (usize, Vec<(f32, f32)>, bool);

/// A drawn area's polygon: a lasso's points, or the rectangle spanned by a
/// drag's first and last point.
fn area_polygon(points: Vec<(f32, f32)>, lasso: bool) -> Vec<(f32, f32)> {
    if lasso {
        return points;
    }
    match (points.first(), points.last()) {
        (Some(&a), Some(&b)) if points.len() > 1 => vec![a, (b.0, a.1), b, (a.0, b.1)],
        _ => Vec::new(),
    }
}

/// Fills a closed polygon (even-odd rule at pixel centres) into `mask`.
fn fill_polygon(mask: &mut GrayImage, poly: &[(f32, f32)]) {
    if poly.len() < 3 {
        return;
    }
    let (w, h) = mask.dimensions();
    let top = poly.iter().map(|p| p.1).fold(f32::MAX, f32::min);
    let bottom = poly.iter().map(|p| p.1).fold(f32::MIN, f32::max);
    let y0 = top.floor().clamp(0.0, h as f32) as u32;
    let y1 = bottom.ceil().clamp(0.0, h as f32) as u32;
    let mut xs = Vec::new();
    for y in y0..y1 {
        let cy = y as f32 + 0.5;
        xs.clear();
        for k in 0..poly.len() {
            let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
            if (a.1 <= cy) != (b.1 <= cy) {
                xs.push(a.0 + (cy - a.1) / (b.1 - a.1) * (b.0 - a.0));
            }
        }
        xs.sort_by(f32::total_cmp);
        for pair in xs.as_chunks::<2>().0 {
            let xa = (pair[0] - 0.5).ceil().clamp(0.0, w as f32) as u32;
            let xb = ((pair[1] - 0.5).floor() + 1.0).clamp(0.0, w as f32) as u32;
            for x in xa..xb {
                mask.put_pixel(x, y, image::Luma([255]));
            }
        }
    }
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// `#RRGGBB` (the `#` is optional).
fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let t = text.trim().trim_start_matches('#');
    if t.len() != 6 || !t.is_ascii() {
        return None;
    }
    let c = |k: usize| u8::from_str_radix(&t[k..k + 2], 16).ok();
    Some([c(0)?, c(2)?, c(4)?])
}

/// Swatch list (base colour, share, new colour or keep), Tolerance, and
/// Detail strength — shared by the Parts Layout window and the editor.
pub fn recolor_controls(ui: &mut egui::Ui, rc: &mut Recolor, selected: Option<usize>) {
    for (i, sw) in rc.swatches.iter().enumerate() {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
            ui.painter()
                .rect_filled(r, 2.0, Color32::from_rgb(sw.rgb[0], sw.rgb[1], sw.rgb[2]));
            if selected == Some(i) {
                ui.painter().rect_stroke(
                    r.expand(2.0),
                    2.0,
                    Stroke::new(2.0, Color32::from_rgb(40, 140, 255)),
                    egui::StrokeKind::Outside,
                );
            }
            ui.label(format!("{:.0} %", sw.share * 100.0));
            ui.label("→");
            // The new colour (the paint itself until another is picked):
            // picker, R G B values, or a hex code.
            let mut new = rc.targets[i].unwrap_or(sw.rgb);
            let mut edited = ui
                .color_edit_button_srgb(&mut new)
                .on_hover_text("Pick the new colour for this paint")
                .changed();
            for (c, name) in new.iter_mut().zip(["R ", "G ", "B "]) {
                edited |= ui
                    .add(egui::DragValue::new(c).range(0..=255).prefix(name))
                    .changed();
            }
            let id = ui.id().with(("recolor-hex", i));
            let mut text = ui
                .data_mut(|d| d.get_temp::<String>(id))
                .unwrap_or_else(|| hex(new));
            let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(64.0));
            if field.changed() {
                if let Some(c) = parse_hex(&text) {
                    new = c;
                    edited = true;
                }
            }
            if !field.has_focus() {
                text = hex(new);
            }
            ui.data_mut(|d| d.insert_temp(id, text));
            if edited {
                rc.targets[i] = (new != sw.rgb).then_some(new);
            }
            if rc.targets[i].is_some() {
                if ui
                    .small_button("Keep")
                    .on_hover_text("Keep the original colour")
                    .clicked()
                {
                    rc.targets[i] = None;
                }
            } else {
                ui.weak("unchanged");
            }
        });
    }
    if rc.is_identity() {
        ui.weak(
            "Click a colour after → (or type R G B or #RRGGBB) to choose a paint's new colour.",
        );
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Tolerance");
        ui.add(egui::Slider::new(&mut rc.tolerance, 4.0..=40.0).suffix(" ΔE"));
    });
    let mut detail = (rc.detail * 100.0).round();
    ui.horizontal(|ui| {
        ui.label("Detail strength");
        ui.add(egui::Slider::new(&mut detail, 0.0..=150.0).suffix(" %"));
    });
    rc.detail = detail / 100.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn areas_fill_rectangles_and_lassos() {
        let mut m = GrayImage::new(20, 20);
        fill_polygon(&mut m, &area_polygon(vec![(2.0, 3.0), (6.0, 9.0)], false));
        let count = |m: &GrayImage| m.pixels().filter(|p| p[0] > 0).count();
        assert_eq!(count(&m), 4 * 6);
        assert!(m.get_pixel(2, 3)[0] > 0 && m.get_pixel(6, 3)[0] == 0);
        let mut t = GrayImage::new(20, 20);
        fill_polygon(&mut t, &[(0.0, 0.0), (20.0, 0.0), (0.0, 20.0)]);
        assert!(t.get_pixel(2, 2)[0] > 0 && t.get_pixel(18, 18)[0] == 0);
        assert!((count(&t) as i32 - 200).abs() <= 20, "{}", count(&t));
    }

    #[test]
    fn hex_codes_parse() {
        assert_eq!(parse_hex("#2846A0"), Some([0x28, 0x46, 0xA0]));
        assert_eq!(parse_hex(" 2846a0 "), Some([0x28, 0x46, 0xA0]));
        assert_eq!(parse_hex("#zz12"), None);
        assert_eq!(parse_hex("#2846A"), None);
        assert_eq!(hex([0x28, 0x46, 0xA0]), "#2846A0");
    }

    #[test]
    fn previews_fit_any_gpu() {
        // An A0 page at 300 dpi.
        let big = RgbaImage::new(9933, 14043);
        let p = preview(&big);
        assert!(p.width().max(p.height()) <= MAX_TEXTURE_SIDE as u32);
        assert_eq!(preview(&RgbaImage::new(400, 200)).dimensions(), (100, 50));
    }
}
