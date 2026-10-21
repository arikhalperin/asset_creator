//! Sprite sheets: grid and max-rects packing, TexturePacker JSON Hash
//! atlases, animation strips.

use std::path::{Path, PathBuf};

use image::RgbaImage;
use serde_json::{json, Map, Value};

use crate::asset_library::{save_entry, Asset};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Fixed cells of the largest frame size; `None` columns = ⌈√n⌉.
    Grid { columns: Option<u32> },
    /// Max-rects, best-short-side-fit.
    MaxRects,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetOptions {
    pub layout: Layout,
    /// Space between frames and around the edge (px).
    pub padding: u32,
    pub power_of_two: bool,
    pub max_size: u32,
}

impl Default for SheetOptions {
    fn default() -> Self {
        Self {
            layout: Layout::MaxRects,
            padding: 2,
            power_of_two: false,
            max_size: 4096,
        }
    }
}

/// A frame's place in a sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub name: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// A packed sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub image: RgbaImage,
    pub frames: Vec<Placed>,
    /// Animation name → its frame names in order.
    pub animations: Vec<(String, Vec<String>)>,
}

/// Named frames to pack.
pub type Frames<'a> = Vec<(String, &'a RgbaImage)>;
/// Animation name → frame names in order.
pub type Animations = Vec<(String, Vec<String>)>;

/// The frames assets contribute: a single asset as itself, an animation as
/// `<name>_01`, … plus an animation entry.
pub fn frames_of<'a>(assets: &[&'a Asset]) -> (Frames<'a>, Animations) {
    // Names must be unique in an atlas: a repeated name (or a single asset
    // named like an animation frame) gets " (2)", " (3)", ….
    let mut used = std::collections::HashSet::new();
    let mut unique = |name: String| -> String {
        let mut candidate = name.clone();
        let mut k = 2;
        while !used.insert(candidate.clone()) {
            candidate = format!("{name} ({k})");
            k += 1;
        }
        candidate
    };
    let mut frames = Vec::new();
    let mut animations = Vec::new();
    for asset in assets {
        if asset.is_animation() {
            let digits = asset.frames.len().to_string().len().max(2);
            let base = unique(asset.name.clone());
            let names: Vec<String> = (1..=asset.frames.len())
                .map(|i| unique(format!("{base}_{i:0digits$}")))
                .collect();
            frames.extend(names.iter().cloned().zip(asset.frames.iter()));
            animations.push((base, names));
        } else {
            frames.push((unique(asset.name.clone()), &asset.image));
        }
    }
    (frames, animations)
}

fn pow2(v: u32) -> u32 {
    v.max(1).next_power_of_two()
}

/// Packs `frames` (unique names) into one sheet.
pub fn pack(
    frames: &[(String, &RgbaImage)],
    animations: Vec<(String, Vec<String>)>,
    opts: &SheetOptions,
) -> Result<Sheet, String> {
    if frames.is_empty() {
        return Err("nothing to pack".to_owned());
    }
    let pad = opts.padding;
    let sizes: Vec<(u32, u32)> = frames.iter().map(|(_, i)| i.dimensions()).collect();
    let positions: Vec<(u32, u32)> = match opts.layout {
        Layout::Grid { columns } => {
            let n = frames.len() as u32;
            let cols = columns
                .unwrap_or_else(|| (n as f64).sqrt().ceil() as u32)
                .clamp(1, n);
            let cw = sizes.iter().map(|s| s.0).max().unwrap_or(1);
            let ch = sizes.iter().map(|s| s.1).max().unwrap_or(1);
            (0..n)
                .map(|i| (pad + (i % cols) * (cw + pad), pad + (i / cols) * (ch + pad)))
                .collect()
        }
        Layout::MaxRects => max_rects(&sizes, pad, opts)?,
    };
    let mut w = frames
        .iter()
        .zip(&positions)
        .map(|((_, i), p)| p.0 + i.width())
        .max()
        .unwrap_or(0)
        + pad;
    let mut h = frames
        .iter()
        .zip(&positions)
        .map(|((_, i), p)| p.1 + i.height())
        .max()
        .unwrap_or(0)
        + pad;
    if opts.power_of_two {
        w = pow2(w);
        h = pow2(h);
    }
    if w > opts.max_size || h > opts.max_size {
        return Err(format!(
            "the sprite sheet would be {w} × {h} px, larger than the maximum {} px",
            opts.max_size
        ));
    }
    let mut image = RgbaImage::new(w, h);
    let mut placed = Vec::new();
    for ((name, img), &(x, y)) in frames.iter().zip(&positions) {
        image::imageops::replace(&mut image, *img, x as i64, y as i64);
        placed.push(Placed {
            name: name.clone(),
            x,
            y,
            w: img.width(),
            h: img.height(),
        });
    }
    Ok(Sheet {
        image,
        frames: placed,
        animations,
    })
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl Rect {
    fn contains(&self, o: &Rect) -> bool {
        o.x >= self.x
            && o.y >= self.y
            && o.x + o.w <= self.x + self.w
            && o.y + o.h <= self.y + self.h
    }
    fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

/// Max-rects (best-short-side-fit) placement in the smallest bin that fits,
/// growing from the area bound; returns each frame's top-left.
fn max_rects(
    sizes: &[(u32, u32)],
    pad: u32,
    opts: &SheetOptions,
) -> Result<Vec<(u32, u32)>, String> {
    let area: u64 = sizes
        .iter()
        .map(|&(w, h)| (w + pad) as u64 * (h + pad) as u64)
        .sum();
    let widest = sizes.iter().map(|s| s.0 + pad).max().unwrap_or(1) + pad;
    let tallest = sizes.iter().map(|s| s.1 + pad).max().unwrap_or(1) + pad;
    let mut side = ((area as f64).sqrt().ceil() as u32 + pad)
        .max(widest)
        .max(tallest);
    if opts.power_of_two {
        side = pow2(side);
    }
    let (mut bw, mut bh) = (side, side);
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i].0 as u64 * sizes[i].1 as u64));
    loop {
        if let Some(pos) = try_pack(sizes, &order, pad, bw, bh) {
            return Ok(pos);
        }
        if bw > opts.max_size && bh > opts.max_size {
            return Err(format!(
                "the frames do not fit in {} × {} px",
                opts.max_size, opts.max_size
            ));
        }
        // Grow the shorter side.
        let grow = |v: u32| {
            if opts.power_of_two {
                v * 2
            } else {
                v + v / 4 + 1
            }
        };
        if bw <= bh {
            bw = grow(bw);
        } else {
            bh = grow(bh);
        }
    }
}

fn try_pack(
    sizes: &[(u32, u32)],
    order: &[usize],
    pad: u32,
    bw: u32,
    bh: u32,
) -> Option<Vec<(u32, u32)>> {
    // Each frame occupies (w + pad) × (h + pad) from (x, y), inside a bin
    // offset by `pad` from the sheet's top-left.
    let mut free = vec![Rect {
        x: pad,
        y: pad,
        w: bw.saturating_sub(pad),
        h: bh.saturating_sub(pad),
    }];
    let mut pos = vec![(0, 0); sizes.len()];
    for &i in order {
        let (w, h) = (sizes[i].0 + pad, sizes[i].1 + pad);
        let best = free
            .iter()
            .filter(|f| f.w >= w && f.h >= h)
            .map(|f| ((f.w - w).min(f.h - h), (f.w - w).max(f.h - h), f.y, f.x, *f))
            .min_by_key(|k| (k.0, k.1, k.2, k.3))?;
        let node = Rect {
            x: best.4.x,
            y: best.4.y,
            w,
            h,
        };
        pos[i] = (node.x, node.y);
        // Split every free rectangle the node overlaps.
        let mut next = Vec::new();
        for f in free {
            if !f.intersects(&node) {
                next.push(f);
                continue;
            }
            if node.x > f.x {
                next.push(Rect {
                    x: f.x,
                    y: f.y,
                    w: node.x - f.x,
                    h: f.h,
                });
            }
            if node.x + node.w < f.x + f.w {
                next.push(Rect {
                    x: node.x + node.w,
                    y: f.y,
                    w: f.x + f.w - node.x - node.w,
                    h: f.h,
                });
            }
            if node.y > f.y {
                next.push(Rect {
                    x: f.x,
                    y: f.y,
                    w: f.w,
                    h: node.y - f.y,
                });
            }
            if node.y + node.h < f.y + f.h {
                next.push(Rect {
                    x: f.x,
                    y: node.y + node.h,
                    w: f.w,
                    h: f.y + f.h - node.y - node.h,
                });
            }
        }
        // Drop free rectangles contained in others.
        let mut pruned: Vec<Rect> = Vec::new();
        for (a, ra) in next.iter().enumerate() {
            let inside = next
                .iter()
                .enumerate()
                .any(|(b, rb)| a != b && rb.contains(ra) && !(ra.contains(rb) && b > a));
            if !inside {
                pruned.push(*ra);
            }
        }
        free = pruned;
    }
    Some(pos)
}

/// A horizontal strip of an animation's frames.
pub fn strip(asset: &Asset, padding: u32) -> Result<Sheet, String> {
    let (frames, animations) = frames_of(&[asset]);
    pack(
        &frames,
        animations,
        &SheetOptions {
            layout: Layout::Grid {
                columns: Some(frames.len() as u32),
            },
            padding,
            power_of_two: false,
            max_size: u32::MAX,
        },
    )
}

/// TexturePacker JSON Hash for `sheet` whose image is saved as `image_file`.
pub fn atlas_json(sheet: &Sheet, image_file: &str) -> Value {
    let mut frames = Map::new();
    for f in &sheet.frames {
        frames.insert(
            f.name.clone(),
            json!({
                "frame": {"x": f.x, "y": f.y, "w": f.w, "h": f.h},
                "rotated": false,
                "trimmed": false,
                "spriteSourceSize": {"x": 0, "y": 0, "w": f.w, "h": f.h},
                "sourceSize": {"w": f.w, "h": f.h},
            }),
        );
    }
    let mut animations = Map::new();
    for (name, list) in &sheet.animations {
        animations.insert(name.clone(), json!(list));
    }
    json!({
        "frames": frames,
        "animations": animations,
        "meta": {
            "app": "Game Asset Creator",
            "version": env!("CARGO_PKG_VERSION"),
            "image": image_file,
            "format": "RGBA8888",
            "size": {"w": sheet.image.width(), "h": sheet.image.height()},
            "scale": "1",
        },
    })
}

/// Writes `sheet` as `<path>` (PNG) and `<path stem>.json`; returns both.
pub fn write_sheet(sheet: &Sheet, path: &Path) -> Result<Vec<PathBuf>, String> {
    let png = path.with_extension("png");
    let json_path = path.with_extension("json");
    sheet
        .image
        .save_with_format(&png, image::ImageFormat::Png)
        .map_err(|e| format!("could not save {}: {e}", png.display()))?;
    let file_name = png
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let text =
        serde_json::to_string_pretty(&atlas_json(sheet, &file_name)).expect("atlas serializes");
    std::fs::write(&json_path, text)
        .map_err(|e| format!("could not save {}: {e}", json_path.display()))?;
    Ok(vec![png, json_path])
}

/// How assets are exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// One PNG per asset (animations as numbered sequences) into a folder.
    Pngs,
    /// One packed sprite sheet with its atlas.
    Sheet(SheetOptions),
    /// Each animation as a horizontal strip with its atlas (single assets
    /// as PNGs).
    Strips { padding: u32 },
    /// One PNG per asset (frames numbered) into a folder, each image the
    /// asset repeated `columns` × `rows` times (1 × 1: the tile itself).
    Tileable { columns: u32, rows: u32 },
}

/// Exports `assets` to `target` (a folder for PNGs and strips; the sheet
/// file path for a sprite sheet); returns the files written.
pub fn export(
    assets: &[&Asset],
    format: ExportFormat,
    target: &Path,
) -> Result<Vec<PathBuf>, String> {
    match format {
        ExportFormat::Pngs => crate::asset_library::save_assets(assets.iter().copied(), target),
        ExportFormat::Tileable { columns, rows } => {
            let (columns, rows) = (columns.clamp(1, 16), rows.clamp(1, 16));
            let tiled: Vec<Asset> = assets
                .iter()
                .map(|a| {
                    let mut t = (*a).clone();
                    t.image = crate::pattern::repeat(&a.image, columns, rows);
                    t.frames = a
                        .frames
                        .iter()
                        .map(|f| crate::pattern::repeat(f, columns, rows))
                        .collect();
                    t
                })
                .collect();
            crate::asset_library::save_assets(tiled.iter(), target)
        }
        ExportFormat::Sheet(opts) => {
            let (frames, animations) = frames_of(assets);
            write_sheet(&pack(&frames, animations, &opts)?, target)
        }
        ExportFormat::Strips { padding } => {
            std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
            let mut written = Vec::new();
            for asset in assets {
                // Never overwrite: an existing (or just written) name gets
                // " (2)", " (3)", ….
                let taken = |p: &Path, written: &[PathBuf]| {
                    p.exists() || written.contains(&p.to_path_buf())
                };
                let mut base = asset.name.clone();
                let mut k = 2;
                loop {
                    let png = target.join(format!("{base}.png"));
                    let clash = if asset.is_animation() {
                        taken(&png, &written) || taken(&png.with_extension("json"), &written)
                    } else {
                        crate::asset_library::entry_paths(asset, &png)
                            .iter()
                            .any(|p| taken(p, &written))
                    };
                    if !clash {
                        break;
                    }
                    base = format!("{} ({k})", asset.name);
                    k += 1;
                }
                let path = target.join(format!("{base}.png"));
                if asset.is_animation() {
                    written.extend(write_sheet(&strip(asset, padding)?, &path)?);
                } else {
                    written.extend(save_entry(asset, &path)?);
                }
            }
            Ok(written)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn sprite(w: u32, h: u32, seed: u8) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            Rgba([seed, (x * 7) as u8, (y * 11) as u8, 255])
        })
    }

    fn check(sheet: &Sheet, frames: &[(String, &RgbaImage)], pad: u32) {
        let (w, h) = sheet.image.dimensions();
        for (i, a) in sheet.frames.iter().enumerate() {
            assert!(
                a.x >= pad && a.y >= pad && a.x + a.w + pad <= w && a.y + a.h + pad <= h,
                "{a:?} in {w}×{h}"
            );
            for b in &sheet.frames[i + 1..] {
                let apart = a.x + a.w + pad <= b.x
                    || b.x + b.w + pad <= a.x
                    || a.y + a.h + pad <= b.y
                    || b.y + b.h + pad <= a.y;
                assert!(apart, "{a:?} and {b:?} overlap (padding {pad})");
            }
            let src = frames.iter().find(|(n, _)| *n == a.name).unwrap().1;
            let cut = image::imageops::crop_imm(&sheet.image, a.x, a.y, a.w, a.h).to_image();
            assert_eq!(&cut, src, "{} pixels", a.name);
        }
    }

    #[test]
    fn grid_and_maxrects_place_every_frame_exactly() {
        let imgs: Vec<RgbaImage> = (0..23)
            .map(|i| sprite(5 + (i * 7) % 29, 4 + (i * 13) % 31, i as u8))
            .collect();
        let frames: Vec<(String, &RgbaImage)> = imgs
            .iter()
            .enumerate()
            .map(|(i, im)| (format!("s{i}"), im))
            .collect();
        for layout in [
            Layout::Grid { columns: None },
            Layout::Grid { columns: Some(4) },
            Layout::MaxRects,
        ] {
            for pad in [0, 2, 5] {
                for pot in [false, true] {
                    let opts = SheetOptions {
                        layout,
                        padding: pad,
                        power_of_two: pot,
                        max_size: 4096,
                    };
                    let sheet = pack(&frames, Vec::new(), &opts).unwrap();
                    check(&sheet, &frames, pad);
                    if pot {
                        assert!(
                            sheet.image.width().is_power_of_two()
                                && sheet.image.height().is_power_of_two()
                        );
                    }
                }
            }
        }
        // Max-rects is denser than the grid here.
        let g = pack(
            &frames,
            Vec::new(),
            &SheetOptions {
                layout: Layout::Grid { columns: None },
                ..Default::default()
            },
        )
        .unwrap();
        let m = pack(&frames, Vec::new(), &SheetOptions::default()).unwrap();
        assert!((m.image.width() * m.image.height()) < (g.image.width() * g.image.height()));
        let too_small = SheetOptions {
            max_size: 32,
            ..Default::default()
        };
        assert!(pack(&frames, Vec::new(), &too_small).is_err());
    }

    #[test]
    fn atlas_json_is_texturepacker_hash() {
        let a = sprite(6, 4, 1);
        let b = sprite(3, 9, 2);
        let frames = vec![("a".to_owned(), &a), ("b".to_owned(), &b)];
        let sheet = pack(
            &frames,
            vec![("walk".to_owned(), vec!["a".to_owned(), "b".to_owned()])],
            &SheetOptions::default(),
        )
        .unwrap();
        let j = atlas_json(&sheet, "sheet.png");
        for f in &sheet.frames {
            let e = &j["frames"][&f.name];
            assert_eq!(e["frame"]["x"], f.x);
            assert_eq!(e["frame"]["w"], f.w);
            assert_eq!(e["sourceSize"]["h"], f.h);
            assert_eq!(e["rotated"], false);
        }
        assert_eq!(j["animations"]["walk"], json!(["a", "b"]));
        assert_eq!(j["meta"]["image"], "sheet.png");
        assert_eq!(j["meta"]["size"]["w"], sheet.image.width());
    }
}
