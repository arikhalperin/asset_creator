//! Pages from a folder of scanned images: listing, the DPI stored in a file,
//! and loading an image as a true-scale page at `DPI`.

use std::path::{Path, PathBuf};

use image::{imageops, Rgba, RgbaImage};

use super::render::Page;
use super::{mm_to_px, px_to_mm, DPI};

/// File extensions loaded from a folder (lower case).
pub const EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "bmp", "webp"];

/// A3 in millimetres (portrait).
pub const A3_MM: (f32, f32) = (297.0, 420.0);

/// Where an image page's size came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeSource {
    /// The resolution stored in the file (dots per inch).
    FileDpi(f32),
    /// No resolution in the file: taken as a full A3 sheet.
    AssumedA3,
    /// A resolution the user entered.
    Override(f32),
    /// The file's resolution would make the page larger than A0; taken as
    /// A3 instead.
    IgnoredDpi(f32),
}

/// Longest side a page may have (A0).
pub const MAX_PAGE_MM: f32 = 1189.0;

impl SizeSource {
    pub fn describe(&self) -> String {
        match self {
            SizeSource::FileDpi(d) => format!("from file DPI ({d:.0} dpi)"),
            SizeSource::AssumedA3 => "assumed A3".to_owned(),
            SizeSource::Override(d) => format!("{d:.0} dpi set by you"),
            SizeSource::IgnoredDpi(d) => {
                format!("assumed A3; the file's {d:.0} dpi would make it larger than A0")
            }
        }
    }
}

/// Splits a name into text and number runs so "page2" sorts before "page10".
fn natural_key(name: &str) -> Vec<(String, u128)> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut digits = String::new();
    for c in name.chars() {
        if c.is_ascii_digit() {
            if !text.is_empty() {
                out.push((std::mem::take(&mut text), 0));
            }
            digits.push(c);
        } else {
            if !digits.is_empty() {
                out.push((String::new(), digits.parse().unwrap_or(u128::MAX)));
                digits.clear();
            }
            text.extend(c.to_lowercase());
        }
    }
    if !text.is_empty() {
        out.push((text, 0));
    }
    if !digits.is_empty() {
        out.push((String::new(), digits.parse().unwrap_or(u128::MAX)));
    }
    out
}

/// The folder's image files in natural name order (others ignored).
pub fn list_images(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        // Hidden files (e.g. macOS "._name" resource forks) are not pages.
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        })
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        })
        .collect();
    files.sort_by_cached_key(|p| {
        natural_key(
            &p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )
    });
    Ok(files)
}

/// The resolution stored in a PNG (pHYs) or JPEG (JFIF density), if any.
pub fn file_dpi(path: &Path) -> Option<f32> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => {
            let file = std::fs::File::open(path).ok()?;
            let reader = png::Decoder::new(std::io::BufReader::new(file))
                .read_info()
                .ok()?;
            let dims = reader.info().pixel_dims?;
            (dims.unit == png::Unit::Meter && dims.xppu > 0).then_some(dims.xppu as f32 * 0.0254)
        }
        "jpg" | "jpeg" => {
            let mut head = vec![0u8; 64];
            use std::io::Read;
            let n = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
            let head = &head[..n];
            // SOI, APP0 marker, length, "JFIF\0", version (2), units, Xdensity.
            if head.len() < 18
                || head[0..4] != [0xFF, 0xD8, 0xFF, 0xE0]
                || &head[6..11] != b"JFIF\0"
            {
                return None;
            }
            let units = head[13];
            let x = u16::from_be_bytes([head[14], head[15]]) as f32;
            match units {
                1 if x > 0.0 => Some(x),
                2 if x > 0.0 => Some(x * 2.54),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Loads an image as a page at `DPI`: at `dpi` if given, else the file's
/// DPI, else as a full A3 sheet (portrait or landscape by its shape, fitted
/// inside). Transparency is flattened onto white.
pub fn load_page(path: &Path, dpi: Option<f32>) -> Result<(Page, SizeSource), String> {
    let img = image::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .into_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return Err(format!("{}: empty image", path.display()));
    }
    let longest_mm = |d: f32| w.max(h) as f32 / d * 25.4;
    let a3 = |source: SizeSource| {
        let (aw, ah) = if w > h { (A3_MM.1, A3_MM.0) } else { A3_MM };
        let s = (mm_to_px(aw) / w as f32).min(mm_to_px(ah) / h as f32);
        (s, source)
    };
    let (scale, source) = match dpi.filter(|d| *d > 0.0) {
        Some(d) if longest_mm(d) > MAX_PAGE_MM => {
            return Err(format!(
                "at {d:.0} dpi the page would be {:.0} mm long (at most {MAX_PAGE_MM:.0} mm)",
                longest_mm(d)
            ));
        }
        Some(d) => (DPI / d, SizeSource::Override(d)),
        None => match file_dpi(path).filter(|d| (20.0..=4800.0).contains(d)) {
            Some(d) if longest_mm(d) > MAX_PAGE_MM => a3(SizeSource::IgnoredDpi(d)),
            Some(d) => (DPI / d, SizeSource::FileDpi(d)),
            None => a3(SizeSource::AssumedA3),
        },
    };
    let nw = ((w as f32 * scale).round() as u32).max(1);
    let nh = ((h as f32 * scale).round() as u32).max(1);
    let scaled = if (nw, nh) == (w, h) {
        img
    } else {
        imageops::resize(&img, nw, nh, imageops::FilterType::Triangle)
    };
    let mut page = RgbaImage::from_pixel(nw, nh, Rgba([255, 255, 255, 255]));
    imageops::overlay(&mut page, &scaled, 0, 0);
    Ok((
        Page {
            image: page,
            width_mm: px_to_mm(nw as f32),
            height_mm: px_to_mm(nh as f32),
        },
        source,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_and_only_images() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "page10.png",
            "page2.PNG",
            "page1.jpg",
            "notes.txt",
            "Page3.webp",
            "._page1.png",
            ".DS_Store",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let names: Vec<String> = list_images(dir.path())
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["page1.jpg", "page2.PNG", "Page3.webp", "page10.png"]
        );
    }

    fn save_png_with_dpi(path: &Path, img: &RgbaImage, dpi: Option<f32>) {
        let file = std::fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width(), img.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        if let Some(d) = dpi {
            enc.set_pixel_dims(Some(png::PixelDimensions {
                xppu: (d / 0.0254).round() as u32,
                yppu: (d / 0.0254).round() as u32,
                unit: png::Unit::Meter,
            }));
        }
        enc.write_header()
            .unwrap()
            .write_image_data(img.as_raw())
            .unwrap();
    }

    #[test]
    fn sizes_come_from_the_file_or_a3() {
        let dir = tempfile::tempdir().unwrap();
        let img = RgbaImage::from_pixel(1200, 1697, Rgba([200, 10, 10, 255]));
        // 150 dpi: 203.2 × 287.4 mm, resampled to 300 dpi.
        let p = dir.path().join("scan.png");
        save_png_with_dpi(&p, &img, Some(150.0));
        assert!((file_dpi(&p).unwrap() - 150.0).abs() < 0.1);
        let (page, src) = load_page(&p, None).unwrap();
        assert_eq!(src, SizeSource::FileDpi(file_dpi(&p).unwrap()));
        assert_eq!(page.image.dimensions(), (2400, 3394));
        assert!((page.width_mm - 203.2).abs() < 0.2);
        // No DPI: A3 portrait, fitted.
        let q = dir.path().join("nodpi.png");
        save_png_with_dpi(&q, &img, None);
        assert_eq!(file_dpi(&q), None);
        let (page, src) = load_page(&q, None).unwrap();
        assert_eq!(src, SizeSource::AssumedA3);
        assert!((page.height_mm - 420.0).abs() < 0.5 && page.width_mm <= 297.5);
        // Landscape without DPI: A3 landscape.
        let r = dir.path().join("wide.png");
        save_png_with_dpi(&r, &RgbaImage::new(1697, 1200), None);
        let (page, _) = load_page(&r, None).unwrap();
        assert!((page.width_mm - 420.0).abs() < 0.5);
        assert_eq!(
            page.image.get_pixel(5, 5).0,
            [255, 255, 255, 255],
            "transparent → white"
        );
        // Override.
        let (page, src) = load_page(&p, Some(300.0)).unwrap();
        assert_eq!(src, SizeSource::Override(300.0));
        assert_eq!(page.image.dimensions(), (1200, 1697));
        // A file DPI that would make the page larger than A0: A3 instead.
        let t = dir.path().join("tiny_dpi.png");
        save_png_with_dpi(&t, &img, Some(30.0));
        let (page, src) = load_page(&t, None).unwrap();
        assert!(matches!(src, SizeSource::IgnoredDpi(d) if (d - 30.0).abs() < 0.1));
        assert!((page.height_mm - 420.0).abs() < 0.5);
        // An override that large is refused.
        assert!(load_page(&p, Some(30.0)).is_err());
    }

    #[test]
    fn jpeg_density_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.jpg");
        image::DynamicImage::ImageRgb8(image::RgbImage::new(40, 30))
            .save(&p)
            .unwrap();
        // Patch the JFIF header: units = dpi, density 200 × 200.
        let mut bytes = std::fs::read(&p).unwrap();
        assert_eq!(&bytes[6..11], b"JFIF\0");
        bytes[13] = 1;
        bytes[14..16].copy_from_slice(&200u16.to_be_bytes());
        bytes[16..18].copy_from_slice(&200u16.to_be_bytes());
        std::fs::write(&p, &bytes).unwrap();
        assert_eq!(file_dpi(&p), Some(200.0));
    }
}
