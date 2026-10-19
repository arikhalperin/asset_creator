//! The editing canvas: view transform math, a tiled composite cache with
//! dirty-region refresh, and the egui widget that draws and navigates it.
//!
//! Coordinates: "image" coordinates are document pixels; "view" coordinates
//! are egui points relative to the canvas rectangle's top-left corner.
//! `zoom` is physical screen pixels per image pixel, so 1.0 is Actual Size.

use egui::{
    Color32, ColorImage, CursorIcon, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Shape,
    Stroke, TextureHandle, TextureOptions, TextureWrapMode, Vec2,
};

use crate::app_state::OpenDocument;
use crate::document::{Document, PixelRect};
use crate::selection::{self, CombineMode, Segment};
use crate::tools::{SelectionRequest, SelectionShape, Tool, ToolOptions};
use crate::transform::{self, Affine, Handle};

/// Something the active tool asks the application to do.
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasEvent {
    Selection(SelectionRequest),
    /// Start a paint stroke at `point`; `straight` (Shift+click) paints a line
    /// from the previous stroke's end and is followed by `StrokeEnd`.
    StrokeBegin {
        point: (f32, f32),
        straight: bool,
    },
    StrokeMove((f32, f32)),
    StrokeEnd,
    /// Option/Alt+click with the Clone Stamp.
    CloneSource((f32, f32)),
    /// Eyedropper sample of an image pixel.
    Pick {
        pixel: (u32, u32),
        background: bool,
    },
    Bucket((u32, u32)),
    Gradient {
        start: (f32, f32),
        end: (f32, f32),
    },
    /// Move tool drag (image coordinates).
    MoveBegin((f32, f32)),
    MoveDrag((f32, f32)),
    MoveEnd,
    /// Free Transform: a handle (or the box) was grabbed / dragged / released.
    TransformGrab {
        handle: Handle,
        point: (f32, f32),
        shift: bool,
    },
    TransformDrag {
        point: (f32, f32),
        shift: bool,
    },
    TransformRelease,
    /// Extract Object: a click (replacing the prompt, or adding an include /
    /// exclude point with Shift / Option).
    ExtractPoint {
        point: (f32, f32),
        mode: PointMode,
    },
    /// Extract Object: a dragged box.
    ExtractBox {
        start: (f32, f32),
        end: (f32, f32),
    },
}

/// How an Extract Object click changes the prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointMode {
    Replace,
    Include,
    Exclude,
}

/// The Free Transform box to draw and interact with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformBox {
    pub affine: Affine,
    pub bounds: PixelRect,
}
use image::{Rgba, RgbaImage};

pub const MIN_ZOOM: f32 = 0.01;
pub const MAX_ZOOM: f32 = 64.0;
/// The pixel grid is drawn when zoom is strictly greater than this.
pub const GRID_MIN_ZOOM: f32 = 8.0;
/// Zoom In / Zoom Out factor.
pub const ZOOM_STEP: f32 = std::f32::consts::SQRT_2;
/// Side length of a cache tile in image pixels.
pub const TILE_SIZE: u32 = 256;
/// Side length of one checkerboard square in points.
pub const CHECKER_SQUARE: f32 = 8.0;

const CHECKER_LIGHT: u8 = 0xD6;
const CHECKER_DARK: u8 = 0xA8;
const GRID_COLOR: Color32 = Color32::from_rgba_premultiplied(40, 40, 40, 110);
/// Marching-ants dash and gap length in points.
pub const ANT_DASH: f32 = 4.0;
/// Marching-ants speed: one dash length per 0.25 s.
pub const ANT_SPEED: f32 = ANT_DASH / 0.25;
/// Clicking this close (points) to the first polygon vertex closes the polygon.
pub const POLYGON_CLOSE_DISTANCE: f32 = 6.0;

/// Phase of the marching-ants pattern at `time` seconds.
pub fn ant_phase(time: f64) -> f32 {
    ((time * ANT_SPEED as f64) % (2.0 * ANT_DASH as f64)) as f32
}

/// A selection gesture in progress (image coordinates).
#[derive(Debug, Clone, PartialEq)]
enum Gesture {
    Drag {
        tool: Tool,
        mode: CombineMode,
        start: Pos2,
        current: Pos2,
        path: Vec<Pos2>,
    },
    Polygon {
        mode: CombineMode,
        points: Vec<Pos2>,
    },
}

/// Full-resolution tiles: nearest-neighbor when magnified, linear when
/// minified. Mipmaps come from the CPU-built pyramid in [`TileCache`].
pub const TILE_TEXTURE_OPTIONS: TextureOptions = TextureOptions {
    magnification: egui::TextureFilter::Nearest,
    minification: egui::TextureFilter::Linear,
    wrap_mode: TextureWrapMode::ClampToEdge,
    mipmap_mode: None,
};

/// Zoom and pan of one canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewState {
    /// Physical screen pixels per image pixel.
    pub zoom: f32,
    /// View position (points) of the image's top-left corner.
    pub offset: Vec2,
}

impl ViewState {
    /// Size of one image pixel in points.
    pub fn points_per_pixel(&self, pixels_per_point: f32) -> f32 {
        self.zoom / pixels_per_point
    }

    pub fn image_to_view(&self, image: Pos2, pixels_per_point: f32) -> Pos2 {
        (image.to_vec2() * self.points_per_pixel(pixels_per_point) + self.offset).to_pos2()
    }

    pub fn view_to_image(&self, view: Pos2, pixels_per_point: f32) -> Pos2 {
        ((view.to_vec2() - self.offset) / self.points_per_pixel(pixels_per_point)).to_pos2()
    }

    /// The image's rectangle in view coordinates.
    pub fn image_rect(&self, image_size: [u32; 2], pixels_per_point: f32) -> Rect {
        Rect::from_min_max(
            self.image_to_view(Pos2::ZERO, pixels_per_point),
            self.image_to_view(
                Pos2::new(image_size[0] as f32, image_size[1] as f32),
                pixels_per_point,
            ),
        )
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.offset += delta;
    }

    /// Sets zoom (clamped) keeping the image point under `anchor` fixed.
    pub fn set_zoom_around(&mut self, anchor: Pos2, zoom: f32, pixels_per_point: f32) {
        let image_point = self.view_to_image(anchor, pixels_per_point);
        self.zoom = clamp_zoom(zoom);
        self.offset =
            anchor.to_vec2() - image_point.to_vec2() * self.points_per_pixel(pixels_per_point);
    }

    /// Multiplies zoom by `factor` keeping the image point under `anchor` fixed.
    pub fn zoom_around(&mut self, anchor: Pos2, factor: f32, pixels_per_point: f32) {
        self.set_zoom_around(anchor, self.zoom * factor, pixels_per_point);
    }

    /// `zoom`, with the image centered in a view of `view_size` points.
    pub fn centered(
        zoom: f32,
        view_size: Vec2,
        image_size: [u32; 2],
        pixels_per_point: f32,
    ) -> Self {
        let zoom = clamp_zoom(zoom);
        let image_points =
            Vec2::new(image_size[0] as f32, image_size[1] as f32) * zoom / pixels_per_point;
        Self {
            zoom,
            offset: (view_size - image_points) / 2.0,
        }
    }

    /// The largest zoom showing the whole image, centered.
    pub fn fit(view_size: Vec2, image_size: [u32; 2], pixels_per_point: f32) -> Self {
        let zoom = (view_size.x * pixels_per_point / image_size[0] as f32)
            .min(view_size.y * pixels_per_point / image_size[1] as f32);
        Self::centered(zoom, view_size, image_size, pixels_per_point)
    }

    /// 100%, centered.
    pub fn actual_size(view_size: Vec2, image_size: [u32; 2], pixels_per_point: f32) -> Self {
        Self::centered(1.0, view_size, image_size, pixels_per_point)
    }

    /// Fit if the image is larger than the view, otherwise 100%; centered.
    pub fn initial(view_size: Vec2, image_size: [u32; 2], pixels_per_point: f32) -> Self {
        let view_pixels = view_size * pixels_per_point;
        if image_size[0] as f32 > view_pixels.x || image_size[1] as f32 > view_pixels.y {
            Self::fit(view_size, image_size, pixels_per_point)
        } else {
            Self::actual_size(view_size, image_size, pixels_per_point)
        }
    }

    /// Whether the pixel grid should be drawn at this zoom.
    pub fn shows_pixel_grid(&self) -> bool {
        self.zoom > GRID_MIN_ZOOM
    }
}

pub fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

/// View commands from menus and shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewAction {
    ZoomIn,
    ZoomOut,
    Fit,
    ActualSize,
}

/// How an image is divided into cache tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileGrid {
    pub width: u32,
    pub height: u32,
    pub cols: u32,
    pub rows: u32,
}

impl TileGrid {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            cols: width.div_ceil(TILE_SIZE),
            rows: height.div_ceil(TILE_SIZE),
        }
    }

    pub fn len(&self) -> usize {
        (self.cols * self.rows) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn tile_rect(&self, index: usize) -> PixelRect {
        let col = index as u32 % self.cols;
        let row = index as u32 / self.cols;
        let x = col * TILE_SIZE;
        let y = row * TILE_SIZE;
        PixelRect::new(
            x,
            y,
            TILE_SIZE.min(self.width - x),
            TILE_SIZE.min(self.height - y),
        )
    }

    /// Indices of tiles overlapping `rect` (clipped to the image).
    pub fn tiles_intersecting(&self, rect: PixelRect) -> Vec<usize> {
        let Some(rect) = rect.intersect(PixelRect::new(0, 0, self.width, self.height)) else {
            return Vec::new();
        };
        let (c0, c1) = (rect.x / TILE_SIZE, (rect.right() - 1) / TILE_SIZE);
        let (r0, r1) = (rect.y / TILE_SIZE, (rect.bottom() - 1) / TILE_SIZE);
        (r0..=r1)
            .flat_map(|r| (c0..=c1).map(move |c| (r * self.cols + c) as usize))
            .collect()
    }
}

/// Upper bound on pyramid depth; level 7 is 1/128 scale, below the 1% zoom floor.
pub const MAX_MIP_LEVELS: usize = 8;

/// The pyramid level to draw at `zoom`: full resolution when magnified or at
/// 100%, otherwise the smallest level that is still at least display
/// resolution, so the GPU only ever minifies by less than 2× (linear filter).
pub fn mip_level_for_zoom(zoom: f32, level_count: usize) -> usize {
    if zoom >= 1.0 || level_count <= 1 {
        return 0;
    }
    let level = (1.0 / zoom).log2().floor().max(0.0) as usize;
    level.min(level_count - 1)
}

/// Size of pyramid level `level` for a `width`×`height` image.
pub fn level_size(width: u32, height: u32, level: usize) -> (u32, u32) {
    let scale = 1u32 << level;
    (width.div_ceil(scale).max(1), height.div_ceil(scale).max(1))
}

/// Maps a full-resolution rectangle to the covering rectangle at `level`.
pub fn rect_at_level(rect: PixelRect, level: usize) -> PixelRect {
    let scale = 1u32 << level;
    let x = rect.x / scale;
    let y = rect.y / scale;
    PixelRect::new(
        x,
        y,
        rect.right().div_ceil(scale) - x,
        rect.bottom().div_ceil(scale) - y,
    )
}

/// Writes the 2× box-filtered version of `rect` (in `dst` coordinates) from
/// `src` into `dst`. Averages in premultiplied alpha so colour hidden under
/// transparent pixels does not bleed into visible ones.
pub fn downsample_into(src: &RgbaImage, dst: &mut RgbaImage, rect: PixelRect) {
    let Some(rect) = rect.intersect(PixelRect::new(0, 0, dst.width(), dst.height())) else {
        return;
    };
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            let mut sum = [0.0f32; 4];
            let mut count = 0.0f32;
            for (sx, sy) in [
                (2 * x, 2 * y),
                (2 * x + 1, 2 * y),
                (2 * x, 2 * y + 1),
                (2 * x + 1, 2 * y + 1),
            ] {
                if sx < src.width() && sy < src.height() {
                    let [r, g, b, a] = src.get_pixel(sx, sy).0;
                    let alpha = a as f32 / 255.0;
                    sum[0] += r as f32 * alpha;
                    sum[1] += g as f32 * alpha;
                    sum[2] += b as f32 * alpha;
                    sum[3] += a as f32;
                    count += 1.0;
                }
            }
            let alpha = sum[3] / count;
            let pixel = if alpha <= 0.0 {
                [0, 0, 0, 0]
            } else {
                let unpremultiply =
                    |c: f32| (c / count / (alpha / 255.0)).round().clamp(0.0, 255.0) as u8;
                [
                    unpremultiply(sum[0]),
                    unpremultiply(sum[1]),
                    unpremultiply(sum[2]),
                    alpha.round() as u8,
                ]
            };
            dst.put_pixel(x, y, Rgba(pixel));
        }
    }
}

/// One level of the mip pyramid: CPU pixels plus one texture per tile.
struct MipLevel {
    grid: TileGrid,
    image: RgbaImage,
    textures: Vec<Option<TextureHandle>>,
    dirty: Vec<bool>,
    uploads: u64,
}

impl MipLevel {
    fn new(width: u32, height: u32) -> Self {
        let grid = TileGrid::new(width, height);
        Self {
            grid,
            image: RgbaImage::new(width, height),
            textures: vec![None; grid.len()],
            dirty: vec![true; grid.len()],
            uploads: 0,
        }
    }
}

/// The document composite cached as GPU textures, one per tile, in a mip
/// pyramid of 2× box-filtered levels (level 0 is full resolution). Mipmaps
/// are built here on the CPU because egui's wgpu renderer creates textures
/// without mip levels.
pub struct TileCache {
    width: u32,
    height: u32,
    levels: Vec<MipLevel>,
    /// Full-resolution regions changed since the last refresh.
    pending: Vec<PixelRect>,
}

impl TileCache {
    /// A cache with every tile of every level pending upload.
    pub fn new(width: u32, height: u32) -> Self {
        let mut levels = vec![MipLevel::new(width, height)];
        while levels.len() < MAX_MIP_LEVELS {
            let (w, h) = level_size(width, height, levels.len());
            let last = &levels[levels.len() - 1].grid;
            if (w, h) == (last.width, last.height) {
                break;
            }
            levels.push(MipLevel::new(w, h));
        }
        Self {
            width,
            height,
            levels,
            pending: vec![PixelRect::new(0, 0, width, height)],
        }
    }

    /// The full-resolution tile grid.
    pub fn grid(&self) -> TileGrid {
        self.levels[0].grid
    }

    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    pub fn level_grid(&self, level: usize) -> TileGrid {
        self.levels[level].grid
    }

    /// CPU pixels of a pyramid level (valid after a refresh).
    pub fn level_image(&self, level: usize) -> &RgbaImage {
        &self.levels[level].image
    }

    pub fn mark_dirty(&mut self, rect: PixelRect) {
        let Some(rect) = rect.intersect(PixelRect::new(0, 0, self.width, self.height)) else {
            return;
        };
        self.pending.push(rect);
        for (k, level) in self.levels.iter_mut().enumerate() {
            for index in level.grid.tiles_intersecting(rect_at_level(rect, k)) {
                level.dirty[index] = true;
            }
        }
    }

    /// Full-resolution tiles waiting to be uploaded.
    pub fn dirty_count(&self) -> usize {
        self.levels[0].dirty.iter().filter(|d| **d).count()
    }

    /// Full-resolution tile uploads since creation.
    pub fn uploads(&self) -> u64 {
        self.levels[0].uploads
    }

    /// Tile uploads at one pyramid level since creation.
    pub fn level_uploads(&self, level: usize) -> u64 {
        self.levels[level].uploads
    }

    /// Recomposites changed regions, rebuilds the affected parts of every
    /// pyramid level, and uploads dirty tiles. Rebuilds everything if the
    /// document size changed.
    pub fn refresh(&mut self, ctx: &egui::Context, doc: &Document, name: &str) {
        if self.width != doc.width() || self.height != doc.height() {
            let uploads: Vec<u64> = self.levels.iter().map(|l| l.uploads).collect();
            *self = Self::new(doc.width(), doc.height());
            for (level, count) in self.levels.iter_mut().zip(uploads) {
                level.uploads = count;
            }
        }
        for rect in std::mem::take(&mut self.pending) {
            let pixels = doc.composite_region(rect);
            image::imageops::replace(
                &mut self.levels[0].image,
                &pixels,
                rect.x as i64,
                rect.y as i64,
            );
            for k in 1..self.levels.len() {
                let (above, below) = self.levels.split_at_mut(k);
                downsample_into(
                    &above[k - 1].image,
                    &mut below[0].image,
                    rect_at_level(rect, k),
                );
            }
        }
        for (k, level) in self.levels.iter_mut().enumerate() {
            // Full resolution is shown crisp when magnified; smaller levels are
            // only ever drawn slightly minified.
            let options = if k == 0 {
                TILE_TEXTURE_OPTIONS
            } else {
                TextureOptions::LINEAR
            };
            for index in 0..level.grid.len() {
                if !level.dirty[index] {
                    continue;
                }
                let rect = level.grid.tile_rect(index);
                let tile = image::imageops::crop_imm(
                    &level.image,
                    rect.x,
                    rect.y,
                    rect.width,
                    rect.height,
                )
                .to_image();
                let image = ColorImage::from_rgba_unmultiplied(
                    [rect.width as usize, rect.height as usize],
                    tile.as_raw(),
                );
                match &mut level.textures[index] {
                    Some(texture) => texture.set(image, options),
                    slot => {
                        *slot = Some(ctx.load_texture(
                            format!("{name}-mip{k}-tile-{index}"),
                            image,
                            options,
                        ))
                    }
                }
                level.dirty[index] = false;
                level.uploads += 1;
            }
        }
    }
}

/// Per-document canvas state kept by the UI.
pub struct CanvasState {
    view: Option<ViewState>,
    tiles: TileCache,
    pending: Vec<ViewAction>,
    hover_pixel: Option<(u32, u32)>,
    panning: bool,
    rect: Option<Rect>,
    gesture: Option<Gesture>,
    /// Free Transform handle being dragged, with the image point it was grabbed at.
    transform_grab: Option<(Handle, (f32, f32))>,
    /// A paint stroke (or eyedropper drag) is in progress.
    painting: bool,
    clone_source: Option<Pos2>,
    clone_offset: Option<Vec2>,
    /// Selection outline in image coordinates, keyed by selection revision.
    outline: Option<(u64, Vec<Segment>)>,
}

impl CanvasState {
    pub fn new(doc: &Document) -> Self {
        Self {
            view: None,
            tiles: TileCache::new(doc.width(), doc.height()),
            pending: Vec::new(),
            hover_pixel: None,
            panning: false,
            rect: None,
            gesture: None,
            transform_grab: None,
            painting: false,
            clone_source: None,
            clone_offset: None,
            outline: None,
        }
    }

    /// Abandons any selection gesture in progress (e.g. an open polygon).
    pub fn cancel_gesture(&mut self) {
        self.gesture = None;
        self.transform_grab = None;
        self.painting = false;
    }

    /// Whether a paint stroke or eyedropper drag is in progress.
    pub fn is_painting(&self) -> bool {
        self.painting
    }

    /// The Clone Stamp source point (image coordinates), if set.
    pub fn clone_source(&self) -> Option<(f32, f32)> {
        self.clone_source.map(|p| (p.x, p.y))
    }

    /// Sets the Clone Stamp source and forgets the aligned offset.
    pub fn set_clone_source(&mut self, point: (f32, f32)) {
        self.clone_source = Some(Pos2::new(point.0, point.1));
        self.clone_offset = None;
    }

    /// Offset from painted pixel to source pixel, once a clone stroke started.
    pub fn clone_offset(&self) -> Option<(f32, f32)> {
        self.clone_offset.map(|v| (v.x, v.y))
    }

    pub fn set_clone_offset(&mut self, offset: (f32, f32)) {
        self.clone_offset = Some(Vec2::new(offset.0, offset.1));
    }

    /// Vertices of the polygon being drawn, if any.
    pub fn polygon_points(&self) -> Option<usize> {
        match &self.gesture {
            Some(Gesture::Polygon { points, .. }) => Some(points.len()),
            _ => None,
        }
    }

    /// Whether a selection gesture is in progress.
    pub fn has_gesture(&self) -> bool {
        self.gesture.is_some()
    }

    /// `None` until the canvas has been shown once.
    pub fn view(&self) -> Option<ViewState> {
        self.view
    }

    /// Screen rectangle the canvas occupied when last shown.
    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    pub fn tiles(&self) -> &TileCache {
        &self.tiles
    }

    pub fn mark_dirty(&mut self, rect: PixelRect) {
        self.tiles.mark_dirty(rect);
    }

    /// Image pixel under the pointer, when it is over the image.
    pub fn hover_pixel(&self) -> Option<(u32, u32)> {
        self.hover_pixel
    }

    /// Queues a view command; it applies on the next frame the canvas is shown.
    pub fn request(&mut self, action: ViewAction) {
        self.pending.push(action);
    }

    fn apply(&mut self, action: ViewAction, view_size: Vec2, image: [u32; 2], ppp: f32) {
        let current = self
            .view
            .unwrap_or_else(|| ViewState::initial(view_size, image, ppp));
        let center = (view_size / 2.0).to_pos2();
        self.view = Some(match action {
            ViewAction::ZoomIn | ViewAction::ZoomOut => {
                let factor = if action == ViewAction::ZoomIn {
                    ZOOM_STEP
                } else {
                    1.0 / ZOOM_STEP
                };
                let mut view = current;
                view.zoom_around(center, factor, ppp);
                view
            }
            ViewAction::Fit => ViewState::fit(view_size, image, ppp),
            ViewAction::ActualSize => ViewState::actual_size(view_size, image, ppp),
        });
    }

    /// Draws the canvas filling the available space, handles navigation, and
    /// runs the active tool. Returns a finished selection gesture, if any.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        open: &OpenDocument,
        checker: &TextureHandle,
        show_grid: bool,
        tool: Tool,
        options: &ToolOptions,
        transform_box: Option<TransformBox>,
    ) -> Vec<CanvasEvent> {
        let doc = &open.document;
        let image = [doc.width(), doc.height()];
        let ppp = ui.ctx().pixels_per_point();
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let view_size = rect.size();
        self.rect = Some(rect);

        if self.view.is_none() {
            self.view = Some(ViewState::initial(view_size, image, ppp));
        }
        for action in std::mem::take(&mut self.pending) {
            self.apply(action, view_size, image, ppp);
        }
        let mut view = self.view.expect("initialized above");

        // --- navigation ---------------------------------------------------
        let hovered = response.contains_pointer();
        let (space_down, space_pressed, scroll, zoom, pointer) = ui.input(|i| {
            (
                i.key_down(Key::Space),
                i.key_pressed(Key::Space),
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.pointer.hover_pos(),
            )
        });
        let typing = ui.ctx().text_edit_focused();
        let space_down = space_down && !typing;
        if hovered && space_pressed && !typing {
            // Keep Space from activating whatever widget had focus.
            response.request_focus();
        }

        self.panning = (response.dragged_by(PointerButton::Primary) && space_down)
            || response.dragged_by(PointerButton::Middle);
        if self.panning {
            view.pan(response.drag_delta());
        }
        if hovered {
            if zoom != 1.0 {
                if let Some(pointer) = pointer {
                    view.zoom_around((pointer - rect.min).to_pos2(), zoom, ppp);
                }
            } else if scroll != Vec2::ZERO {
                view.pan(scroll);
            }
        }
        if self.panning {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if hovered && space_down {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        self.view = Some(view);

        let mut events = Vec::new();
        if let Some(tb) = transform_box {
            if !(self.panning || space_down) {
                events = self.run_transform(ui, &response, rect, &view, ppp, tb);
            }
        } else if !(self.panning || space_down) || self.painting {
            if tool.is_selection_tool() {
                if let Some(request) =
                    self.run_tool(ui, &response, rect, &view, ppp, image, tool, options)
                {
                    events.push(CanvasEvent::Selection(request));
                }
            } else if !space_down || self.painting {
                events = self.run_paint_tool(ui, &response, rect, &view, ppp, image, tool);
            }
        }
        let over_image = pointer
            .filter(|_| hovered)
            .map(|p| view.view_to_image((p - rect.min).to_pos2(), ppp))
            .is_some_and(|p| {
                p.x >= 0.0 && p.y >= 0.0 && p.x < image[0] as f32 && p.y < image[1] as f32
            });
        let brush_cursor = transform_box.is_none()
            && tool.is_brush_tool()
            && hovered
            && !self.panning
            && !space_down;
        let interacting = hovered && !self.panning && !space_down;
        if let (Some(tb), true, Some(p)) = (transform_box, interacting, pointer) {
            let image_point = view.view_to_image((p - rect.min).to_pos2(), ppp);
            let tolerance = 6.0 / view.points_per_pixel(ppp);
            let handle = self.transform_grab.map(|(h, _)| h).or_else(|| {
                transform::hit_test(
                    &tb.affine,
                    tb.bounds,
                    (image_point.x, image_point.y),
                    tolerance,
                )
            });
            ui.ctx().set_cursor_icon(match handle {
                Some(Handle::Move) => CursorIcon::Move,
                Some(Handle::Scale(0, _)) => CursorIcon::ResizeVertical,
                Some(Handle::Scale(_, 0)) => CursorIcon::ResizeHorizontal,
                Some(Handle::Scale(a, b)) if a == b => CursorIcon::ResizeNwSe,
                Some(Handle::Scale(_, _)) => CursorIcon::ResizeNeSw,
                Some(Handle::Rotate) => CursorIcon::Alias,
                None => CursorIcon::Default,
            });
        } else if transform_box.is_none() && tool == Tool::Move && interacting {
            ui.ctx().set_cursor_icon(CursorIcon::Move);
        } else if brush_cursor {
            ui.ctx().set_cursor_icon(CursorIcon::None);
        } else if over_image && !self.panning && !space_down {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }

        self.hover_pixel = pointer
            .filter(|_| hovered)
            .map(|p| view.view_to_image((p - rect.min).to_pos2(), ppp))
            .filter(|p| p.x >= 0.0 && p.y >= 0.0 && p.x < image[0] as f32 && p.y < image[1] as f32)
            .map(|p| (p.x as u32, p.y as u32));

        // --- painting -----------------------------------------------------
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);

        let image_rect = view.image_rect(image, ppp).translate(rect.min.to_vec2());
        let visible = image_rect.intersect(rect);
        if !visible.is_positive() {
            return events;
        }

        // Screen-space checkerboard: one texture repeat covers two squares.
        let period = 2.0 * CHECKER_SQUARE;
        let uv = Rect::from_min_max(
            (visible.min.to_vec2() / period).to_pos2(),
            (visible.max.to_vec2() / period).to_pos2(),
        );
        painter.image(checker.id(), visible, uv, Color32::WHITE);

        self.tiles
            .refresh(ui.ctx(), doc, &format!("document-{}", open.id.0));
        let visible_pixels = visible_pixel_rect(&view, visible, rect, image, ppp);
        let full_uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        let level = mip_level_for_zoom(view.zoom, self.tiles.level_count());
        let scale = (1u32 << level) as f32;
        let mip = &self.tiles.levels[level];
        for index in mip
            .grid
            .tiles_intersecting(rect_at_level(visible_pixels, level))
        {
            let Some(texture) = &mip.textures[index] else {
                continue;
            };
            // Tile corners in full-resolution image pixels; the last row and
            // column of a level may extend past the image by under one level pixel.
            let tile = mip.grid.tile_rect(index);
            let corner = |x: u32, y: u32| {
                Pos2::new(
                    (x as f32 * scale).min(image[0] as f32),
                    (y as f32 * scale).min(image[1] as f32),
                )
            };
            let min = view.image_to_view(corner(tile.x, tile.y), ppp);
            let max = view.image_to_view(corner(tile.right(), tile.bottom()), ppp);
            let screen = Rect::from_min_max(min, max).translate(rect.min.to_vec2());
            painter.image(texture.id(), screen, full_uv, Color32::WHITE);
        }

        if show_grid && view.shows_pixel_grid() {
            let stroke = Stroke::new(1.0 / ppp, GRID_COLOR);
            let step = view.points_per_pixel(ppp);
            let origin = image_rect.min;
            for x in visible_pixels.x..=visible_pixels.right() {
                let sx = origin.x + x as f32 * step;
                painter.line_segment(
                    [Pos2::new(sx, visible.min.y), Pos2::new(sx, visible.max.y)],
                    stroke,
                );
            }
            for y in visible_pixels.y..=visible_pixels.bottom() {
                let sy = origin.y + y as f32 * step;
                painter.line_segment(
                    [Pos2::new(visible.min.x, sy), Pos2::new(visible.max.x, sy)],
                    stroke,
                );
            }
        }

        // --- selection outline and gesture preview ----------------------
        let time = ui.input(|i| i.time);
        let phase = ant_phase(time);
        let to_screen = |p: Pos2| {
            (image_rect.min.to_vec2() + p.to_vec2() * view.points_per_pixel(ppp)).to_pos2()
        };
        let mut shapes = Vec::new();
        if let Some(mask) = doc.selection() {
            if self.outline.as_ref().map(|(rev, _)| *rev) != Some(open.selection_revision) {
                self.outline = Some((open.selection_revision, selection::outline(mask)));
            }
            let clip = rect.expand(1.0);
            for &((x0, y0), (x1, y1)) in &self.outline.as_ref().expect("computed above").1 {
                let a = to_screen(Pos2::new(x0 as f32, y0 as f32));
                let b = to_screen(Pos2::new(x1 as f32, y1 as f32));
                if !clip.intersects(Rect::from_two_pos(a, b)) {
                    continue;
                }
                ants(&mut shapes, &[a, b], phase);
            }
        } else {
            self.outline = None;
        }
        match &self.gesture {
            Some(Gesture::Drag {
                tool,
                start,
                current,
                path,
                ..
            }) => {
                let path: Vec<Pos2> = match tool {
                    Tool::RectSelect | Tool::ExtractObject => {
                        let r = Rect::from_two_pos(to_screen(*start), to_screen(*current));
                        vec![
                            r.left_top(),
                            r.right_top(),
                            r.right_bottom(),
                            r.left_bottom(),
                            r.left_top(),
                        ]
                    }
                    Tool::EllipseSelect => {
                        let r = Rect::from_two_pos(to_screen(*start), to_screen(*current));
                        (0..=64)
                            .map(|i| {
                                let a = i as f32 / 64.0 * std::f32::consts::TAU;
                                r.center()
                                    + Vec2::new(
                                        a.cos() * r.width() / 2.0,
                                        a.sin() * r.height() / 2.0,
                                    )
                            })
                            .collect()
                    }
                    _ => {
                        let mut p: Vec<Pos2> = path.iter().map(|p| to_screen(*p)).collect();
                        if let Some(first) = p.first().copied() {
                            p.push(first);
                        }
                        p
                    }
                };
                ants(&mut shapes, &path, phase);
            }
            Some(Gesture::Polygon { points, .. }) => {
                let mut path: Vec<Pos2> = points.iter().map(|p| to_screen(*p)).collect();
                if let Some(pointer) = pointer.filter(|_| hovered) {
                    path.push(pointer);
                }
                ants(&mut shapes, &path, phase);
            }
            None => {}
        }
        // Free Transform box with handles and center marker.
        if let Some(tb) = transform_box {
            let corners: Vec<Pos2> = tb
                .affine
                .corners(tb.bounds)
                .iter()
                .map(|c| to_screen(Pos2::new(c.0, c.1)))
                .collect();
            let mut outline = corners.clone();
            outline.push(corners[0]);
            shapes.push(Shape::line(
                outline.clone(),
                Stroke::new(3.0, Color32::WHITE),
            ));
            shapes.push(Shape::line(outline, Stroke::new(1.0, Color32::BLACK)));
            for (_, p) in transform::handle_points(&tb.affine, tb.bounds) {
                let r = Rect::from_center_size(to_screen(Pos2::new(p.0, p.1)), Vec2::splat(8.0));
                shapes.push(Shape::rect_filled(r, 0.0, Color32::WHITE));
                shapes.push(Shape::rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, Color32::BLACK),
                    egui::StrokeKind::Inside,
                ));
            }
            let center = tb.affine.map(tb.affine.pivot);
            cross(&mut shapes, to_screen(Pos2::new(center.0, center.1)), 4.0);
        }
        // Gradient start→end preview.
        if let Some(Gesture::Drag {
            tool: Tool::Gradient,
            start,
            current,
            ..
        }) = &self.gesture
        {
            let (a, b) = (to_screen(*start), to_screen(*current));
            shapes.push(Shape::line_segment(
                [a, b],
                Stroke::new(3.0, Color32::WHITE),
            ));
            shapes.push(Shape::line_segment(
                [a, b],
                Stroke::new(1.0, Color32::BLACK),
            ));
            shapes.push(Shape::circle_stroke(
                a,
                3.0,
                Stroke::new(1.0, Color32::BLACK),
            ));
        }
        // Clone source marker, following the brush while painting.
        if tool == Tool::CloneStamp {
            let marker = match (self.painting, self.clone_offset, pointer) {
                (true, Some(offset), Some(p)) => Some(p + offset * view.points_per_pixel(ppp)),
                _ => self.clone_source.map(to_screen),
            };
            if let Some(m) = marker {
                cross(&mut shapes, m, 6.0);
            }
        }
        // Brush outline cursor.
        if let (true, Some(p), Some(brush)) = (brush_cursor, pointer, options.brush_for(tool)) {
            let radius = brush.size / 2.0 * view.points_per_pixel(ppp);
            shapes.push(Shape::circle_stroke(
                p,
                radius + 0.5,
                Stroke::new(1.0, Color32::WHITE),
            ));
            shapes.push(Shape::circle_stroke(
                p,
                radius,
                Stroke::new(1.0, Color32::BLACK),
            ));
            if radius * 2.0 < 4.0 {
                cross(&mut shapes, p, 5.0);
            }
        }
        if !shapes.is_empty() {
            painter.extend(shapes);
        }
        if doc.selection().is_some() || self.gesture.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(40));
        }
        events
    }

    /// Feeds pointer input to the Free Transform box.
    #[allow(clippy::too_many_arguments)]
    fn run_transform(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        rect: Rect,
        view: &ViewState,
        ppp: f32,
        tb: TransformBox,
    ) -> Vec<CanvasEvent> {
        let to_image = |screen: Pos2| {
            let p = view.view_to_image((screen - rect.min).to_pos2(), ppp);
            (p.x, p.y)
        };
        let (pressed, down, shift, press_origin, pointer) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.modifiers.shift,
                i.pointer.press_origin(),
                i.pointer.interact_pos(),
            )
        });
        let mut events = Vec::new();
        if pressed && response.contains_pointer() {
            if let Some(origin) = press_origin.or(pointer) {
                let point = to_image(origin);
                let tolerance = 6.0 / view.points_per_pixel(ppp);
                if let Some(handle) = transform::hit_test(&tb.affine, tb.bounds, point, tolerance) {
                    self.transform_grab = Some((handle, point));
                    events.push(CanvasEvent::TransformGrab {
                        handle,
                        point,
                        shift,
                    });
                }
            }
        } else if self.transform_grab.is_some() {
            if let Some(p) = pointer {
                events.push(CanvasEvent::TransformDrag {
                    point: to_image(p),
                    shift,
                });
            }
            if !down {
                self.transform_grab = None;
                events.push(CanvasEvent::TransformRelease);
            }
        }
        events
    }

    /// Feeds pointer input to the active paint tool.
    #[allow(clippy::too_many_arguments)]
    fn run_paint_tool(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        rect: Rect,
        view: &ViewState,
        ppp: f32,
        image: [u32; 2],
        tool: Tool,
    ) -> Vec<CanvasEvent> {
        let to_image = |screen: Pos2| {
            let p = view.view_to_image((screen - rect.min).to_pos2(), ppp);
            (p.x, p.y)
        };
        let in_image = |p: (f32, f32)| {
            p.0 >= 0.0 && p.1 >= 0.0 && p.0 < image[0] as f32 && p.1 < image[1] as f32
        };
        let pixel = |p: (f32, f32)| (p.0 as u32, p.1 as u32);
        let (pressed, down, modifiers, press_origin, pointer, moves) = ui.input(|i| {
            let moves: Vec<Pos2> = i
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::PointerMoved(p) => Some(*p),
                    _ => None,
                })
                .collect();
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.modifiers,
                i.pointer.press_origin(),
                i.pointer.interact_pos(),
                moves,
            )
        });
        let pressed_here = pressed && response.contains_pointer();
        let mut events = Vec::new();

        // Gradient and Extract Object drags are the paint gestures using `self.gesture`.
        let own_drag = matches!(
            &self.gesture,
            Some(Gesture::Drag { tool: t, .. }) if *t == tool
        );
        if !own_drag || !matches!(tool, Tool::Gradient | Tool::ExtractObject) {
            self.gesture = None;
        }

        match tool {
            Tool::Brush | Tool::Eraser | Tool::CloneStamp => {
                if pressed_here {
                    let start = to_image(press_origin.or(pointer).unwrap_or(rect.center()));
                    if tool == Tool::CloneStamp && modifiers.alt {
                        events.push(CanvasEvent::CloneSource(start));
                        return events;
                    }
                    events.push(CanvasEvent::StrokeBegin {
                        point: start,
                        straight: modifiers.shift,
                    });
                    if modifiers.shift {
                        events.push(CanvasEvent::StrokeEnd);
                    } else {
                        self.painting = true;
                    }
                } else if self.painting {
                    // Every intermediate pointer position this frame, for smooth strokes.
                    let samples = if moves.is_empty() {
                        pointer.into_iter().collect()
                    } else {
                        moves
                    };
                    for p in samples {
                        events.push(CanvasEvent::StrokeMove(to_image(p)));
                    }
                    if !down {
                        events.push(CanvasEvent::StrokeEnd);
                        self.painting = false;
                    }
                }
            }
            Tool::Move => {
                if pressed_here {
                    let start = to_image(press_origin.or(pointer).unwrap_or(rect.center()));
                    events.push(CanvasEvent::MoveBegin(start));
                    self.painting = true;
                } else if self.painting {
                    if let Some(p) = pointer {
                        events.push(CanvasEvent::MoveDrag(to_image(p)));
                    }
                    if !down {
                        events.push(CanvasEvent::MoveEnd);
                        self.painting = false;
                    }
                }
            }
            Tool::Eyedropper => {
                if pressed_here {
                    self.painting = true;
                }
                if self.painting {
                    if let Some(p) = pointer.map(to_image).filter(|p| in_image(*p)) {
                        events.push(CanvasEvent::Pick {
                            pixel: pixel(p),
                            background: modifiers.alt,
                        });
                    }
                    if !down {
                        self.painting = false;
                    }
                }
            }
            Tool::PaintBucket => {
                if response.clicked_by(PointerButton::Primary) {
                    if let Some(p) = pointer.map(to_image).filter(|p| in_image(*p)) {
                        events.push(CanvasEvent::Bucket(pixel(p)));
                    }
                }
            }
            Tool::ExtractObject => {
                if response.drag_started_by(PointerButton::Primary) {
                    if let Some(origin) = press_origin {
                        let start = to_image(origin);
                        let start = Pos2::new(start.0, start.1);
                        self.gesture = Some(Gesture::Drag {
                            tool,
                            mode: CombineMode::Replace,
                            start,
                            current: start,
                            path: Vec::new(),
                        });
                    }
                }
                if let (Some(Gesture::Drag { current, .. }), Some(p)) = (&mut self.gesture, pointer)
                {
                    let p = to_image(p);
                    *current = Pos2::new(p.0, p.1);
                }
                if response.drag_stopped_by(PointerButton::Primary) {
                    if let Some(Gesture::Drag { start, current, .. }) = self.gesture.take() {
                        if (current - start).abs().min_elem() >= 2.0 {
                            events.push(CanvasEvent::ExtractBox {
                                start: (start.x, start.y),
                                end: (current.x, current.y),
                            });
                        }
                    }
                } else if response.clicked_by(PointerButton::Primary) {
                    if let Some(p) = pointer.map(to_image).filter(|p| in_image(*p)) {
                        let mode = if modifiers.alt {
                            PointMode::Exclude
                        } else if modifiers.shift {
                            PointMode::Include
                        } else {
                            PointMode::Replace
                        };
                        events.push(CanvasEvent::ExtractPoint { point: p, mode });
                    }
                }
            }
            Tool::Gradient => {
                if response.drag_started_by(PointerButton::Primary) {
                    if let Some(origin) = press_origin {
                        let start = to_image(origin);
                        let start = Pos2::new(start.0, start.1);
                        self.gesture = Some(Gesture::Drag {
                            tool,
                            mode: CombineMode::Replace,
                            start,
                            current: start,
                            path: Vec::new(),
                        });
                    }
                }
                if let (Some(Gesture::Drag { current, .. }), Some(p)) = (&mut self.gesture, pointer)
                {
                    let p = to_image(p);
                    *current = Pos2::new(p.0, p.1);
                }
                if response.drag_stopped_by(PointerButton::Primary) {
                    if let Some(Gesture::Drag { start, current, .. }) = self.gesture.take() {
                        events.push(CanvasEvent::Gradient {
                            start: (start.x, start.y),
                            end: (current.x, current.y),
                        });
                    }
                }
            }
            _ => {}
        }
        events
    }

    /// Feeds pointer and key input to the active selection tool.
    #[allow(clippy::too_many_arguments)]
    fn run_tool(
        &mut self,
        ui: &mut egui::Ui,
        response: &egui::Response,
        rect: Rect,
        view: &ViewState,
        ppp: f32,
        image: [u32; 2],
        tool: Tool,
        options: &ToolOptions,
    ) -> Option<SelectionRequest> {
        let to_image = |screen: Pos2| view.view_to_image((screen - rect.min).to_pos2(), ppp);
        let (modifiers, press_origin, pointer) = ui.input(|i| {
            (
                i.modifiers,
                i.pointer.press_origin(),
                i.pointer.interact_pos(),
            )
        });
        let mode_now = CombineMode::from_modifiers(modifiers.shift, modifiers.alt, options.mode);
        let tuple = |p: Pos2| (p.x, p.y);

        // A gesture from another tool is abandoned when the tool changes.
        let stale = match &self.gesture {
            Some(Gesture::Drag { tool: t, .. }) => *t != tool,
            Some(Gesture::Polygon { .. }) => tool != Tool::PolygonLasso,
            None => false,
        };
        if stale {
            self.gesture = None;
        }

        match tool {
            Tool::RectSelect | Tool::EllipseSelect | Tool::Lasso => {
                if response.drag_started_by(PointerButton::Primary) {
                    if let Some(origin) = press_origin {
                        let start = to_image(origin);
                        self.gesture = Some(Gesture::Drag {
                            tool,
                            mode: mode_now,
                            start,
                            current: start,
                            path: vec![start],
                        });
                    }
                }
                if let (Some(Gesture::Drag { current, path, .. }), Some(p)) =
                    (&mut self.gesture, pointer)
                {
                    if response.dragged_by(PointerButton::Primary)
                        || response.drag_stopped_by(PointerButton::Primary)
                    {
                        let p = to_image(p);
                        *current = p;
                        let min_step = 1.0 / view.points_per_pixel(ppp);
                        if path.last().is_none_or(|last| last.distance(p) >= min_step) {
                            path.push(p);
                        }
                    }
                }
                if response.drag_stopped_by(PointerButton::Primary) {
                    if let Some(Gesture::Drag {
                        tool,
                        mode,
                        start,
                        current,
                        path,
                    }) = self.gesture.take()
                    {
                        let shape = match tool {
                            Tool::RectSelect => SelectionShape::Rect(tuple(start), tuple(current)),
                            Tool::EllipseSelect => {
                                SelectionShape::Ellipse(tuple(start), tuple(current))
                            }
                            _ => SelectionShape::Polygon(path.into_iter().map(tuple).collect()),
                        };
                        return Some(SelectionRequest { tool, shape, mode });
                    }
                }
                if response.clicked_by(PointerButton::Primary) {
                    return Some(SelectionRequest {
                        tool,
                        shape: SelectionShape::Clear,
                        mode: mode_now,
                    });
                }
                None
            }
            Tool::MagicWand => {
                if !response.clicked_by(PointerButton::Primary) {
                    return None;
                }
                let p = to_image(pointer?);
                if p.x < 0.0 || p.y < 0.0 || p.x >= image[0] as f32 || p.y >= image[1] as f32 {
                    return None;
                }
                Some(SelectionRequest {
                    tool,
                    shape: SelectionShape::Wand(p.x as u32, p.y as u32),
                    mode: mode_now,
                })
            }
            Tool::PolygonLasso => {
                let close = |gesture: Option<Gesture>| match gesture {
                    Some(Gesture::Polygon { mode, points }) if points.len() >= 3 => {
                        Some(SelectionRequest {
                            tool,
                            shape: SelectionShape::Polygon(points.into_iter().map(tuple).collect()),
                            mode,
                        })
                    }
                    _ => None,
                };
                if self.gesture.is_some() {
                    let (enter, backspace, escape) = ui.input_mut(|i| {
                        (
                            i.consume_key(Modifiers::NONE, Key::Enter),
                            i.consume_key(Modifiers::NONE, Key::Backspace),
                            i.consume_key(Modifiers::NONE, Key::Escape),
                        )
                    });
                    if escape {
                        self.gesture = None;
                        return None;
                    }
                    if enter {
                        return close(self.gesture.take());
                    }
                    if backspace {
                        if let Some(Gesture::Polygon { points, .. }) = &mut self.gesture {
                            points.pop();
                            if points.is_empty() {
                                self.gesture = None;
                            }
                        }
                    }
                }
                // egui reports a third quick click as a triple-click (it only
                // checks distance to the previous click), so accept both.
                let multi_click = response.double_clicked_by(PointerButton::Primary)
                    || response.triple_clicked_by(PointerButton::Primary);
                if multi_click && self.gesture.is_some() {
                    return close(self.gesture.take());
                }
                if response.clicked_by(PointerButton::Primary) {
                    let screen = pointer?;
                    let p = to_image(screen);
                    match &mut self.gesture {
                        Some(Gesture::Polygon { points, .. }) => {
                            let first = view.image_to_view(points[0], ppp) + rect.min.to_vec2();
                            // Clicking the first vertex closes the polygon; with
                            // fewer than three vertices, closing cancels it.
                            if first.distance(screen) <= POLYGON_CLOSE_DISTANCE {
                                return close(self.gesture.take());
                            }
                            points.push(p);
                        }
                        _ => {
                            self.gesture = Some(Gesture::Polygon {
                                mode: mode_now,
                                points: vec![p],
                            });
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
}

/// A small black-and-white crosshair centered on `p`.
fn cross(shapes: &mut Vec<Shape>, p: Pos2, half: f32) {
    for (stroke, extra) in [
        (Stroke::new(3.0, Color32::WHITE), 1.0),
        (Stroke::new(1.0, Color32::BLACK), 0.0),
    ] {
        let h = half + extra;
        shapes.push(Shape::line_segment(
            [p - Vec2::new(h, 0.0), p + Vec2::new(h, 0.0)],
            stroke,
        ));
        shapes.push(Shape::line_segment(
            [p - Vec2::new(0.0, h), p + Vec2::new(0.0, h)],
            stroke,
        ));
    }
}

/// Appends marching-ants shapes for a polyline: a white line with black
/// dashes on top, shifted by `phase` (screen-anchored so dashes line up).
fn ants(shapes: &mut Vec<Shape>, path: &[Pos2], phase: f32) {
    if path.len() < 2 {
        return;
    }
    shapes.push(Shape::line(path.to_vec(), Stroke::new(1.0, Color32::WHITE)));
    let anchor = path[0].x + path[0].y;
    let offset = (phase - anchor).rem_euclid(2.0 * ANT_DASH);
    Shape::dashed_line_many_with_offset(
        path,
        Stroke::new(1.0, Color32::BLACK),
        &[ANT_DASH],
        &[ANT_DASH],
        offset,
        shapes,
    );
}

/// Image pixels covered by the visible part of the canvas.
fn visible_pixel_rect(
    view: &ViewState,
    visible: Rect,
    canvas: Rect,
    image: [u32; 2],
    ppp: f32,
) -> PixelRect {
    let min = view.view_to_image(visible.min - canvas.min.to_vec2(), ppp);
    let max = view.view_to_image(visible.max - canvas.min.to_vec2(), ppp);
    let x0 = (min.x.floor().max(0.0) as u32).min(image[0]);
    let y0 = (min.y.floor().max(0.0) as u32).min(image[1]);
    let x1 = (max.x.ceil().max(0.0) as u32).min(image[0]);
    let y1 = (max.y.ceil().max(0.0) as u32).min(image[1]);
    PixelRect::new(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}

/// The 2×2 repeating checkerboard texture.
pub fn checker_texture(ctx: &egui::Context) -> TextureHandle {
    let light = Color32::from_gray(CHECKER_LIGHT);
    let dark = Color32::from_gray(CHECKER_DARK);
    let image = ColorImage::new([2, 2], vec![light, dark, dark, light]);
    ctx.load_texture(
        "canvas-checkerboard",
        image,
        TextureOptions {
            magnification: egui::TextureFilter::Nearest,
            minification: egui::TextureFilter::Nearest,
            wrap_mode: TextureWrapMode::Repeat,
            mipmap_mode: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn image_view_round_trip() {
        let view = ViewState {
            zoom: 3.0,
            offset: Vec2::new(10.0, -4.0),
        };
        for ppp in [1.0, 2.0] {
            let p = Pos2::new(12.5, 7.25);
            let back = view.view_to_image(view.image_to_view(p, ppp), ppp);
            assert!(approx(back.x, p.x) && approx(back.y, p.y));
        }
        // 100% at 2 points-per-pixel: one image pixel is half a point.
        assert!(approx(
            ViewState {
                zoom: 1.0,
                offset: Vec2::ZERO
            }
            .points_per_pixel(2.0),
            0.5
        ));
    }

    #[test]
    fn zoom_around_keeps_anchor_fixed_and_clamps() {
        let mut view = ViewState {
            zoom: 1.0,
            offset: Vec2::new(30.0, 20.0),
        };
        let anchor = Pos2::new(200.0, 150.0);
        let before = view.view_to_image(anchor, 2.0);
        view.zoom_around(anchor, 1.7, 2.0);
        assert!(approx(view.zoom, 1.7));
        let after = view.view_to_image(anchor, 2.0);
        assert!(approx(before.x, after.x) && approx(before.y, after.y));

        view.zoom_around(anchor, 1000.0, 2.0);
        assert_eq!(view.zoom, MAX_ZOOM);
        view.zoom_around(anchor, 1e-9, 2.0);
        assert_eq!(view.zoom, MIN_ZOOM);
        let still = view.view_to_image(anchor, 2.0);
        assert!(approx(before.x, still.x) && approx(before.y, still.y));
    }

    #[test]
    fn fit_shows_whole_image_centered() {
        let view_size = Vec2::new(800.0, 600.0);
        let view = ViewState::fit(view_size, [4000, 4000], 1.0);
        assert!(approx(view.zoom, 600.0 / 4000.0));
        let rect = view.image_rect([4000, 4000], 1.0);
        assert!(approx(rect.min.y, 0.0) && approx(rect.max.y, 600.0));
        assert!(approx(rect.center().x, 400.0));
        // Retina: zoom is in physical pixels.
        let retina = ViewState::fit(view_size, [4000, 4000], 2.0);
        assert!(approx(retina.zoom, 1200.0 / 4000.0));
        // Tiny images fit by magnifying, up to the clamp.
        assert_eq!(ViewState::fit(view_size, [1, 1], 1.0).zoom, MAX_ZOOM);
    }

    #[test]
    fn actual_size_and_initial() {
        let view_size = Vec2::new(400.0, 300.0);
        let actual = ViewState::actual_size(view_size, [100, 50], 2.0);
        assert_eq!(actual.zoom, 1.0);
        let rect = actual.image_rect([100, 50], 2.0);
        assert!(approx(rect.width(), 50.0) && approx(rect.center().x, 200.0));

        // Fits in 800x600 physical pixels → 100%.
        assert_eq!(ViewState::initial(view_size, [800, 600], 2.0).zoom, 1.0);
        // Larger than the view → fit.
        let big = ViewState::initial(view_size, [801, 10], 2.0);
        assert!(big.zoom < 1.0);
    }

    #[test]
    fn pixel_grid_threshold_is_strict() {
        let mut view = ViewState {
            zoom: 8.0,
            offset: Vec2::ZERO,
        };
        assert!(!view.shows_pixel_grid());
        view.zoom = 8.01;
        assert!(view.shows_pixel_grid());
    }

    #[test]
    fn tile_grid_covers_image() {
        let grid = TileGrid::new(600, 300);
        assert_eq!((grid.cols, grid.rows, grid.len()), (3, 2, 6));
        assert_eq!(grid.tile_rect(0), PixelRect::new(0, 0, 256, 256));
        assert_eq!(grid.tile_rect(2), PixelRect::new(512, 0, 88, 256));
        assert_eq!(grid.tile_rect(5), PixelRect::new(512, 256, 88, 44));
        let total: u32 = (0..grid.len())
            .map(|i| grid.tile_rect(i))
            .map(|r| r.width * r.height)
            .sum();
        assert_eq!(total, 600 * 300);
    }

    #[test]
    fn tiles_intersecting_rect() {
        let grid = TileGrid::new(600, 300);
        assert_eq!(
            grid.tiles_intersecting(PixelRect::new(10, 10, 5, 5)),
            vec![0]
        );
        assert_eq!(
            grid.tiles_intersecting(PixelRect::new(250, 250, 10, 10)),
            vec![0, 1, 3, 4]
        );
        assert_eq!(
            grid.tiles_intersecting(PixelRect::new(0, 0, 600, 300))
                .len(),
            6
        );
        assert_eq!(
            grid.tiles_intersecting(PixelRect::new(256, 0, 1, 1)),
            vec![1]
        );
        assert!(grid
            .tiles_intersecting(PixelRect::new(700, 0, 5, 5))
            .is_empty());
        assert!(grid
            .tiles_intersecting(PixelRect::new(5, 5, 0, 5))
            .is_empty());
    }

    #[test]
    fn mip_level_selection() {
        assert_eq!(mip_level_for_zoom(4.0, 5), 0);
        assert_eq!(mip_level_for_zoom(1.0, 5), 0);
        assert_eq!(mip_level_for_zoom(0.99, 5), 0);
        assert_eq!(mip_level_for_zoom(0.5, 5), 1);
        assert_eq!(mip_level_for_zoom(0.3, 5), 1);
        assert_eq!(mip_level_for_zoom(0.25, 5), 2);
        assert_eq!(mip_level_for_zoom(0.18, 5), 2);
        assert_eq!(mip_level_for_zoom(0.01, 5), 4);
        assert_eq!(mip_level_for_zoom(0.01, 1), 0);
    }

    #[test]
    fn level_sizes_and_rect_mapping() {
        assert_eq!(level_size(4000, 4000, 0), (4000, 4000));
        assert_eq!(level_size(4000, 4000, 3), (500, 500));
        assert_eq!(level_size(5, 3, 1), (3, 2));
        assert_eq!(level_size(5, 3, 4), (1, 1));
        assert_eq!(
            rect_at_level(PixelRect::new(3, 5, 2, 1), 1),
            PixelRect::new(1, 2, 2, 1)
        );
        assert_eq!(
            rect_at_level(PixelRect::new(0, 0, 1, 1), 3),
            PixelRect::new(0, 0, 1, 1)
        );
        let cache = TileCache::new(4000, 4000);
        assert_eq!(cache.level_count(), MAX_MIP_LEVELS);
        assert_eq!(cache.level_grid(4).width, 250);
        assert_eq!(TileCache::new(1, 1).level_count(), 1);
        assert_eq!(TileCache::new(3, 1).level_count(), 3);
    }

    #[test]
    fn downsample_averages_premultiplied() {
        let mut src = RgbaImage::new(3, 2);
        src.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        src.put_pixel(1, 0, Rgba([0, 255, 0, 0])); // hidden colour must not bleed
        src.put_pixel(0, 1, Rgba([255, 0, 0, 255]));
        src.put_pixel(1, 1, Rgba([0, 0, 255, 255]));
        src.put_pixel(2, 0, Rgba([10, 20, 30, 200]));
        let mut dst = RgbaImage::new(2, 1);
        downsample_into(&src, &mut dst, PixelRect::new(0, 0, 2, 1));
        let p = dst.get_pixel(0, 0).0;
        assert_eq!(p[3], 191); // (255*3 + 0) / 4
        assert_eq!(p[1], 0, "no green from the transparent pixel");
        assert_eq!(p[0], 170); // 2/3 red
        assert_eq!(p[2], 85); // 1/3 blue
                              // Edge pixel averages only the source pixels that exist.
        assert_eq!(dst.get_pixel(1, 0).0, [10, 20, 30, 100]);
    }

    #[test]
    fn pyramid_matches_rebuild_after_partial_refresh() {
        let ctx = egui::Context::default();
        let mut doc = Document::new_blank("d", 600, 300, 72.0).unwrap();
        let mut cache = TileCache::new(600, 300);
        cache.refresh(&ctx, &doc, "a");
        let level_uploads: Vec<u64> = (0..cache.level_count())
            .map(|k| cache.level_uploads(k))
            .collect();
        // Edit a small block, refresh only that region.
        for y in 100..110 {
            for x in 300..330 {
                doc.layer_mut(0)
                    .unwrap()
                    .pixels
                    .put_pixel(x, y, Rgba([200, 100, 50, 255]));
            }
        }
        cache.mark_dirty(PixelRect::new(300, 100, 30, 10));
        cache.refresh(&ctx, &doc, "a");
        // Exactly one tile per level re-uploaded.
        for (k, before) in level_uploads.iter().enumerate() {
            assert_eq!(cache.level_uploads(k), before + 1, "level {k}");
        }
        let mut fresh = TileCache::new(600, 300);
        fresh.refresh(&ctx, &doc, "b");
        for k in 0..cache.level_count() {
            assert_eq!(cache.level_image(k), fresh.level_image(k), "level {k}");
        }
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
    }

    #[test]
    fn tile_cache_tracks_dirty_tiles() {
        let mut cache = TileCache::new(600, 300);
        assert_eq!(cache.dirty_count(), 6);
        let ctx = egui::Context::default();
        let doc = Document::new_blank("d", 600, 300, 72.0).unwrap();
        cache.refresh(&ctx, &doc, "t");
        assert_eq!((cache.dirty_count(), cache.uploads()), (0, 6));
        cache.refresh(&ctx, &doc, "t");
        assert_eq!(cache.uploads(), 6);
        cache.mark_dirty(PixelRect::new(300, 10, 4, 4));
        assert_eq!(cache.dirty_count(), 1);
        cache.refresh(&ctx, &doc, "t");
        assert_eq!(cache.uploads(), 7);
        // Discard the texture deltas generated without a renderer.
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
    }
}
