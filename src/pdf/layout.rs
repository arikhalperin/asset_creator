//! A parts layout: the parts (or split pieces) and where each sits on the A4
//! sheets; composing sheets and exporting them as a printable PDF.

use std::path::{Path, PathBuf};

use image::{imageops, Rgba, RgbaImage};

use super::pack::{self, Placement};
use super::{mm_to_px, split};

/// One part or piece to lay out.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// "Part 3", "Part 3a", …
    pub label: String,
    /// True-size pixels at `DPI`, transparent around the part.
    pub image: RgbaImage,
}

impl Item {
    pub fn size(&self) -> (u32, u32) {
        self.image.dimensions()
    }

    /// Whether it fits on A4 in some 90° orientation.
    pub fn fits(&self) -> bool {
        let (w, h) = self.size();
        pack::fits(w, h)
    }

    pub fn size_mm(&self) -> (f32, f32) {
        let (w, h) = self.size();
        (super::px_to_mm(w as f32), super::px_to_mm(h as f32))
    }
}

/// Items and their placements; `None` = in the tray.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    pub items: Vec<Item>,
    pub placements: Vec<Option<Placement>>,
    pub sheets: usize,
}

impl Layout {
    pub fn new(items: Vec<Item>) -> Self {
        let mut l = Self {
            placements: vec![None; items.len()],
            items,
            sheets: 0,
        };
        l.repack();
        l
    }

    /// Packs every item that fits anew (manual moves are discarded).
    pub fn repack(&mut self) {
        let sizes: Vec<(u32, u32)> = self.items.iter().map(Item::size).collect();
        let (placed, sheets) = pack::pack(&sizes);
        self.placements = placed;
        self.sheets = sheets.max(1);
    }

    /// Items that do not fit on A4 in either orientation.
    pub fn oversized(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| !self.items[i].fits())
            .collect()
    }

    fn others(&self, except: usize) -> Vec<(Placement, (u32, u32))> {
        self.placements
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != except)
            .filter_map(|(i, p)| p.map(|p| (p, self.items[i].size())))
            .collect()
    }

    /// Moves item `i` to `place` if it is a legal spot (inside the printable
    /// area, clear of other parts); returns whether it moved.
    pub fn move_item(&mut self, i: usize, place: Placement) -> bool {
        if i >= self.items.len() || place.sheet >= self.sheets {
            return false;
        }
        if !pack::valid(place, self.items[i].size(), &self.others(i)) {
            return false;
        }
        self.placements[i] = Some(place);
        true
    }

    /// Moves item `i` to `place`, or — if that spot is illegal — to the
    /// nearest legal spot within `radius` pixels (positions are first clamped
    /// to the printable area). Returns whether it moved.
    pub fn move_item_near(&mut self, i: usize, place: Placement, radius: u32) -> bool {
        let Some(item) = self.items.get(i) else {
            return false;
        };
        let (w, h) = if place.rotated {
            (item.size().1, item.size().0)
        } else {
            item.size()
        };
        let (pw, ph) = pack::printable_px();
        if w > pw || h > ph {
            return false;
        }
        let x = place.x.min(pw - w);
        let y = place.y.min(ph - h);
        let others = self.others(i);
        let ok = |x: u32, y: u32| {
            place.sheet < self.sheets
                && pack::valid(Placement { x, y, ..place }, item.size(), &others)
        };
        let mut best: Option<(u64, u32, u32)> = None;
        let step = 4i64;
        let r = radius as i64;
        let mut dy = -r;
        while dy <= r {
            let mut dx = -r;
            while dx <= r {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if nx >= 0 && ny >= 0 && nx as u32 + w <= pw && ny as u32 + h <= ph {
                    let d = (dx * dx + dy * dy) as u64;
                    if best.is_none_or(|b| d < b.0) && ok(nx as u32, ny as u32) {
                        best = Some((d, nx as u32, ny as u32));
                    }
                }
                dx += step;
            }
            dy += step;
        }
        match best {
            Some((_, x, y)) => {
                self.placements[i] = Some(Placement { x, y, ..place });
                true
            }
            None => false,
        }
    }

    /// Turns a placed item 90° about its top-left, if the result is legal.
    pub fn rotate(&mut self, i: usize) -> bool {
        let Some(p) = self.placements.get(i).copied().flatten() else {
            return false;
        };
        self.move_item(
            i,
            Placement {
                rotated: !p.rotated,
                ..p
            },
        )
    }

    /// Puts item `i` back in the tray.
    pub fn unplace(&mut self, i: usize) {
        if let Some(p) = self.placements.get_mut(i) {
            *p = None;
        }
    }

    pub fn add_sheet(&mut self) {
        self.sheets += 1;
    }

    /// Removes sheet `s` if it is empty (later sheets move up).
    pub fn remove_sheet(&mut self, s: usize) -> bool {
        if self.sheets <= 1 || self.placements.iter().flatten().any(|p| p.sheet == s) {
            return false;
        }
        for p in self.placements.iter_mut().flatten() {
            if p.sheet > s {
                p.sheet -= 1;
            }
        }
        self.sheets -= 1;
        true
    }

    /// Splits item `i` along a line (item pixels) into two pieces (labels
    /// "<label>a" / "<label>b") placed in the tray. Returns whether it split.
    pub fn split(
        &mut self,
        i: usize,
        a: (f32, f32),
        b: (f32, f32),
        glue_mm: f32,
        on: split::GlueOn,
    ) -> bool {
        let Some(item) = self.items.get(i) else {
            return false;
        };
        let Some(pieces) = split::split(&item.image, a, b, glue_mm, on) else {
            return false;
        };
        let label = item.label.clone();
        let [p, q] = pieces;
        self.items[i] = Item {
            label: format!("{label}a"),
            image: p.image,
        };
        self.placements[i] = None;
        self.items.push(Item {
            label: format!("{label}b"),
            image: q.image,
        });
        self.placements.push(None);
        true
    }

    /// Places tray items that fit into free space (existing placements stay).
    pub fn place_tray(&mut self) {
        for i in 0..self.items.len() {
            if self.placements[i].is_some() || !self.items[i].fits() {
                continue;
            }
            let size = self.items[i].size();
            let step = mm_to_px(2.0) as u32;
            let (pw, ph) = pack::printable_px();
            let mut done = false;
            'sheets: for sheet in 0..self.sheets + 1 {
                if sheet == self.sheets {
                    self.add_sheet();
                }
                for rotated in [false, true] {
                    let (w, h) = if rotated { (size.1, size.0) } else { size };
                    if w > pw || h > ph {
                        continue;
                    }
                    for y in (0..=ph - h).step_by(step as usize) {
                        for x in (0..=pw - w).step_by(step as usize) {
                            let place = Placement {
                                sheet,
                                x,
                                y,
                                rotated,
                            };
                            if self.move_item(i, place) {
                                done = true;
                                break 'sheets;
                            }
                        }
                    }
                }
            }
            debug_assert!(done);
        }
    }

    /// Sheet `s` as a white A4 image at `DPI` with its parts.
    pub fn compose(&self, s: usize) -> RgbaImage {
        let (w, h) = sheet_px();
        let mut sheet = RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]));
        let margin = mm_to_px(pack::MARGIN_MM).round() as i64;
        for (item, place) in self.items.iter().zip(&self.placements) {
            let Some(p) = place.filter(|p| p.sheet == s) else {
                continue;
            };
            let img = if p.rotated {
                imageops::rotate90(&item.image)
            } else {
                item.image.clone()
            };
            imageops::overlay(&mut sheet, &img, margin + p.x as i64, margin + p.y as i64);
        }
        sheet
    }

    /// The packed sheets as A4 images.
    pub fn compose_all(&self) -> Vec<RgbaImage> {
        (0..self.sheets).map(|s| self.compose(s)).collect()
    }

    /// Writes all sheets as one A4 PDF (and, with `pngs`, `<stem>_<n>.png`
    /// next to it). Returns the files written.
    pub fn export(&self, path: &Path, pngs: bool) -> Result<Vec<PathBuf>, String> {
        export_sheets(&self.compose_all(), path, pngs)
    }
}

/// Writes `sheets` (A4 images) as one PDF at `path`, and with `pngs` also
/// `<stem>_<nn>.png` next to it. Returns the files written.
pub fn export_sheets(
    sheets: &[RgbaImage],
    path: &Path,
    pngs: bool,
) -> Result<Vec<PathBuf>, String> {
    let never = std::sync::atomic::AtomicBool::new(false);
    write_export(
        sheets.len(),
        |i| sheets[i].clone(),
        path,
        pngs,
        &|_, _| {},
        &never,
    )
}

/// Writes `count` A4 sheets, made one at a time by `sheet(i)`, as one PDF at
/// `path` (written to a temporary file, then renamed), and with `pngs` also
/// `<stem>_<nn>.png`. Calls `progress(done, count)` after each sheet; when
/// `cancel` is set it stops, removes what it wrote, and returns
/// `Err("cancelled")`.
pub fn write_export(
    count: usize,
    mut sheet: impl FnMut(usize) -> RgbaImage,
    path: &Path,
    pngs: bool,
    progress: &dyn Fn(usize, usize),
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<PathBuf>, String> {
    use std::sync::atomic::Ordering;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "sheet".to_owned());
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut pngs_written = Vec::new();
    let cleanup = |files: &[PathBuf]| {
        for f in files {
            let _ = std::fs::remove_file(f);
        }
    };
    let mut pdf = PdfBuilder::new();
    for i in 0..count {
        if cancel.load(Ordering::Relaxed) {
            cleanup(&pngs_written);
            return Err("cancelled".to_owned());
        }
        let img = sheet(i);
        pdf.add(&img);
        if pngs {
            // Under a temporary name until the export completes, so a
            // cancel never touches files from an earlier export.
            let p = dir.join(format!(".{stem}_{:02}.png.part", i + 1));
            let saved = std::fs::File::create(&p)
                .map_err(|e| e.to_string())
                .and_then(|f| {
                    img.write_to(&mut std::io::BufWriter::new(f), image::ImageFormat::Png)
                        .map_err(|e| e.to_string())
                });
            if let Err(e) = saved {
                let _ = std::fs::remove_file(&p);
                cleanup(&pngs_written);
                return Err(format!("{}: {e}", p.display()));
            }
            pngs_written.push(p);
        }
        progress(i + 1, count);
    }
    if cancel.load(Ordering::Relaxed) {
        cleanup(&pngs_written);
        return Err("cancelled".to_owned());
    }
    let tmp = dir.join(format!(".{stem}.pdf.part"));
    std::fs::write(&tmp, pdf.finish()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut written = vec![path.to_path_buf()];
    for (i, part) in pngs_written.iter().enumerate() {
        let p = dir.join(format!("{stem}_{:02}.png", i + 1));
        std::fs::rename(part, &p).map_err(|e| format!("{}: {e}", p.display()))?;
        written.push(p);
    }
    Ok(written)
}

/// The A4 sheet for a manual frame: the page area under the frame (a full
/// A4 sheet, `rotated` = turned 90°; top-left at `x`, `y` page pixels,
/// outside the page white) at true scale, turned onto the portrait sheet and
/// filling it edge to edge.
pub fn compose_frame(page: &RgbaImage, x: i32, y: i32, rotated: bool) -> RgbaImage {
    let (pw, ph) = sheet_px();
    let (fw, fh) = if rotated { (ph, pw) } else { (pw, ph) };
    let mut area = RgbaImage::from_pixel(fw, fh, Rgba([255, 255, 255, 255]));
    imageops::overlay(&mut area, page, -(x as i64), -(y as i64));
    if rotated {
        imageops::rotate90(&area)
    } else {
        area
    }
}

/// A4 at `DPI`, whole pixels (2480 × 3508).
pub fn sheet_px() -> (u32, u32) {
    (
        mm_to_px(pack::SHEET_MM.0).round() as u32,
        mm_to_px(pack::SHEET_MM.1).round() as u32,
    )
}

/// Builds a PDF of A4 pages, one image each, drawn at exactly `DPI` from
/// the page's top-left (pages are added one at a time).
pub struct PdfBuilder {
    pdf: pdf_writer::Pdf,
    pages: Vec<pdf_writer::Ref>,
    next: i32,
}

impl Default for PdfBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfBuilder {
    const CATALOG: i32 = 1;
    const TREE: i32 = 2;

    pub fn new() -> Self {
        Self {
            pdf: pdf_writer::Pdf::new(),
            pages: Vec::new(),
            next: 3,
        }
    }

    fn alloc(&mut self) -> pdf_writer::Ref {
        let r = pdf_writer::Ref::new(self.next);
        self.next += 1;
        r
    }

    /// Adds one A4 page showing `sheet`.
    pub fn add(&mut self, sheet: &RgbaImage) {
        use pdf_writer::{Content, Filter, Finish, Name, Rect, Ref};
        use std::io::Write;
        let page_w = pack::SHEET_MM.0 / 25.4 * 72.0;
        let page_h = pack::SHEET_MM.1 / 25.4 * 72.0;
        let (page_id, content_id, image_id) = (self.alloc(), self.alloc(), self.alloc());
        self.pages.push(page_id);
        let (w, h) = sheet.dimensions();
        let mut page = self.pdf.page(page_id);
        page.media_box(Rect::new(0.0, 0.0, page_w, page_h));
        page.parent(Ref::new(Self::TREE));
        page.contents(content_id);
        page.resources().x_objects().pair(Name(b"Im0"), image_id);
        page.finish();
        let rgb: Vec<u8> = sheet.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&rgb).expect("in-memory write");
        let data = z.finish().expect("in-memory write");
        let mut image = self.pdf.image_xobject(image_id, &data);
        image.filter(Filter::FlateDecode);
        image.width(w as i32);
        image.height(h as i32);
        image.color_space().device_rgb();
        image.bits_per_component(8);
        image.finish();
        let (iw, ih) = (w as f32 / super::DPI * 72.0, h as f32 / super::DPI * 72.0);
        let mut content = Content::new();
        content.save_state();
        content.transform([iw, 0.0, 0.0, ih, 0.0, page_h - ih]);
        content.x_object(Name(b"Im0"));
        content.restore_state();
        self.pdf.stream(content_id, &content.finish());
    }

    /// The finished PDF bytes.
    pub fn finish(mut self) -> Vec<u8> {
        use pdf_writer::Ref;
        let tree = Ref::new(Self::TREE);
        self.pdf.catalog(Ref::new(Self::CATALOG)).pages(tree);
        self.pdf
            .pages(tree)
            .kids(self.pages.iter().copied())
            .count(self.pages.len() as i32);
        self.pdf.finish()
    }
}

/// A PDF with one A4 page per image (see [`PdfBuilder`]).
pub fn pdf_of(sheets: &[RgbaImage]) -> Vec<u8> {
    let mut b = PdfBuilder::new();
    for s in sheets {
        b.add(s);
    }
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, w: u32, h: u32) -> Item {
        Item {
            label: label.to_owned(),
            image: RgbaImage::from_pixel(w, h, Rgba([10, 20, 30, 255])),
        }
    }

    #[test]
    fn moves_rotations_and_sheets_are_checked() {
        let mut l = Layout::new(vec![item("1", 600, 400), item("2", 600, 400)]);
        assert_eq!(l.sheets, 1);
        let p0 = l.placements[0].unwrap();
        // Onto the other part: refused.
        let p1 = l.placements[1].unwrap();
        assert!(!l.move_item(0, Placement { x: p1.x + 5, ..p1 }));
        // Outside the printable area: refused.
        assert!(!l.move_item(0, Placement { x: 2300, ..p0 }));
        // A new sheet and a move there.
        l.add_sheet();
        assert!(l.move_item(
            0,
            Placement {
                sheet: 1,
                x: 0,
                y: 0,
                rotated: false
            }
        ));
        assert!(l.rotate(0));
        assert!(!l.remove_sheet(1), "not empty");
        l.unplace(0);
        assert!(l.remove_sheet(1));
        l.place_tray();
        assert!(l.placements.iter().all(Option::is_some));
    }

    #[test]
    fn frame_sheets_hold_exactly_the_framed_area() {
        let page = RgbaImage::from_fn(4000, 4000, |x, y| {
            Rgba([(x % 251) as u8, (y % 241) as u8, 7, 255])
        });
        let (pw, ph) = sheet_px();
        let s = compose_frame(&page, 100, 50, false);
        assert_eq!(s.dimensions(), sheet_px());
        // Edge to edge: the sheet's corners are the frame's corners.
        assert_eq!(s.get_pixel(0, 0), page.get_pixel(100, 50));
        assert_eq!(
            s.get_pixel(pw - 1, ph - 1),
            page.get_pixel(100 + pw - 1, 50 + ph - 1)
        );
        // Turned: the frame is ph wide and pw tall on the page.
        let r = compose_frame(&page, 200, 300, true);
        assert_eq!(r.dimensions(), sheet_px());
        // rotate90 (clockwise): page (x, y) in the area → sheet (fh-1-y, x).
        let (ax, ay) = (10u32, 20u32);
        assert_eq!(
            r.get_pixel(pw - 1 - ay, ax),
            page.get_pixel(200 + ax, 300 + ay)
        );
        // Past the page edge: white.
        let e = compose_frame(&page, 3000, 0, false);
        assert_eq!(e.get_pixel(pw - 1, 0).0, [255, 255, 255, 255]);
        // Two frames tile an A3 page (297 × 420 mm) exactly: landscape A4s.
        let a3 = (
            mm_to_px(297.0).round() as u32,
            mm_to_px(420.0).round() as u32,
        );
        assert!((a3.0 as i32 - ph as i32).abs() <= 1 && (a3.1 as i32 - 2 * pw as i32).abs() <= 1);
    }

    #[test]
    fn drops_snap_to_free_space_nearby() {
        let mut l = Layout::new(vec![item("1", 600, 400), item("2", 600, 400)]);
        let p1 = l.placements[1].unwrap();
        let p0 = l.placements[0].unwrap();
        let target = Placement {
            x: p0.x,
            y: p0.y + 30,
            ..p0
        };
        assert!(l.move_item_near(0, target, 40));
        let got = l.placements[0].unwrap();
        assert!((got.y as i64 - target.y as i64).abs() <= 40);
        // Past the printable edge: clamped inside.
        assert!(l.move_item_near(
            0,
            Placement {
                x: 99_999,
                y: 2000,
                ..p0
            },
            40
        ));
        let (pw, _) = pack::printable_px();
        assert_eq!(l.placements[0].unwrap().x, pw - 600);
        // Far into another part: refused.
        assert!(!l.move_item_near(0, p1, 40));
    }

    #[test]
    fn composed_sheets_are_a4_and_the_pdf_has_one_page_each() {
        let l = Layout::new(vec![item("1", 2000, 3000), item("2", 2000, 3000)]);
        assert_eq!(l.sheets, 2);
        let s = l.compose(0);
        assert_eq!(s.dimensions(), (2480, 3508));
        let m = mm_to_px(pack::MARGIN_MM).round() as u32;
        assert_eq!(s.get_pixel(m, m).0, [10, 20, 30, 255]);
        assert_eq!(s.get_pixel(m - 1, m).0, [255, 255, 255, 255]);
        let pdf = pdf_of(&[s.clone(), s]);
        let text = String::from_utf8_lossy(&pdf);
        assert_eq!(
            text.matches("/Type /Page\n").count() + text.matches("/Type /Page ").count(),
            2
        );
        assert!(text.contains("/Count 2"));
    }

    #[test]
    fn splitting_replaces_an_item_by_two_tray_pieces() {
        let mut l = Layout::new(vec![item("wing", 4000, 800)]);
        assert_eq!(l.oversized(), vec![0]);
        assert!(l.split(
            0,
            (2000.0, -1.0),
            (2000.0, 801.0),
            8.0,
            split::GlueOn::Smaller
        ));
        assert_eq!(l.items.len(), 2);
        assert_eq!(
            (l.items[0].label.as_str(), l.items[1].label.as_str()),
            ("winga", "wingb")
        );
        assert!(l.oversized().is_empty());
        l.place_tray();
        assert!(l.placements.iter().all(Option::is_some));
    }
}
