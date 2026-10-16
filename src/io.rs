//! Loading raster files into documents and exporting documents back out.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use image::codecs::bmp::BmpEncoder;
use image::codecs::jpeg::{JpegEncoder, PixelDensity, PixelDensityUnit};
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageReader, RgbImage, RgbaImage};
use thiserror::Error;

use crate::document::{Document, DocumentError, Layer, DEFAULT_DPI};

const METERS_PER_INCH: f64 = 0.0254;
const CM_PER_INCH: f64 = 2.54;
const JPEG_QUALITY: u8 = 95;

/// Raster formats the editor can open and export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Png,
    Jpeg,
    Bmp,
    WebP,
}

impl FileFormat {
    pub const ALL: [FileFormat; 4] = [Self::Png, Self::Jpeg, Self::Bmp, Self::WebP];

    /// Determines the format from a path's extension (case-insensitive).
    pub fn from_path(path: &Path) -> Result<Self, IoError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        match ext.as_str() {
            "png" => Ok(Self::Png),
            "jpg" | "jpeg" => Ok(Self::Jpeg),
            "bmp" => Ok(Self::Bmp),
            "webp" => Ok(Self::WebP),
            _ => Err(IoError::UnsupportedFormat(ext)),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Bmp => "BMP",
            Self::WebP => "WebP",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Png => &["png"],
            Self::Jpeg => &["jpg", "jpeg"],
            Self::Bmp => &["bmp"],
            Self::WebP => &["webp"],
        }
    }

    /// Whether exported files keep the alpha channel.
    pub fn supports_alpha(self) -> bool {
        matches!(self, Self::Png | Self::WebP)
    }
}

#[derive(Debug, Error)]
pub enum IoError {
    #[error("unsupported file format: {}", if .0.is_empty() { "(no extension)" } else { .0.as_str() })]
    UnsupportedFormat(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("could not decode image: {0}")]
    Decode(#[source] image::ImageError),
    #[error("could not encode image: {0}")]
    Encode(String),
    #[error(transparent)]
    Document(#[from] DocumentError),
}

/// Loads an image file into a new single-layer document.
pub fn load_document(path: &Path) -> Result<Document, IoError> {
    let format = FileFormat::from_path(path)?;
    let pixels = ImageReader::open(path)?
        .with_guessed_format()?
        .decode()
        .map_err(IoError::Decode)?
        .into_rgba8();
    let dpi = read_dpi(path, format).unwrap_or(DEFAULT_DPI);

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_owned());
    let mut doc = Document::new(name, pixels.width(), pixels.height(), dpi)?;
    doc.add_layer(Layer::new("Background", pixels))?;
    doc.source_path = Some(path.to_path_buf());
    Ok(doc)
}

/// Composites the document's visible layers and writes them in the format
/// given by the path's extension.
pub fn export_document(doc: &Document, path: &Path) -> Result<(), IoError> {
    let format = FileFormat::from_path(path)?;
    let image = doc.composite();
    let writer = BufWriter::new(File::create(path)?);
    match format {
        FileFormat::Png => write_png(writer, &image, doc.dpi()),
        FileFormat::WebP => WebPEncoder::new_lossless(writer)
            .encode(
                image.as_raw(),
                image.width(),
                image.height(),
                ExtendedColorType::Rgba8,
            )
            .map_err(encode_err),
        FileFormat::Jpeg => {
            let rgb = flatten_on_white(&image);
            let mut encoder = JpegEncoder::new_with_quality(writer, JPEG_QUALITY);
            encoder.set_pixel_density(PixelDensity {
                density: (dpi_u16(doc.dpi()), dpi_u16(doc.dpi())),
                unit: PixelDensityUnit::Inches,
            });
            encoder
                .encode(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    ExtendedColorType::Rgb8,
                )
                .map_err(encode_err)
        }
        FileFormat::Bmp => {
            let rgb = flatten_on_white(&image);
            let mut writer = writer;
            BmpEncoder::new(&mut writer)
                .encode(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    ExtendedColorType::Rgb8,
                )
                .map_err(encode_err)?;
            writer.flush()?;
            Ok(())
        }
    }
}

/// Composites RGBA pixels over opaque white, dropping alpha.
pub fn flatten_on_white(image: &RgbaImage) -> RgbImage {
    RgbImage::from_fn(image.width(), image.height(), |x, y| {
        let [r, g, b, a] = image.get_pixel(x, y).0;
        let alpha = a as f32 / 255.0;
        let mix = |c: u8| (c as f32 * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        image::Rgb([mix(r), mix(g), mix(b)])
    })
}

fn write_png(writer: impl Write, image: &RgbaImage, dpi: f64) -> Result<(), IoError> {
    let mut encoder = png::Encoder::new(writer, image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let ppm = (dpi / METERS_PER_INCH).round() as u32;
    encoder.set_pixel_dims(Some(png::PixelDimensions {
        xppu: ppm,
        yppu: ppm,
        unit: png::Unit::Meter,
    }));
    let mut png_writer = encoder.write_header().map_err(encode_err)?;
    png_writer
        .write_image_data(image.as_raw())
        .map_err(encode_err)?;
    png_writer.finish().map_err(encode_err)
}

fn encode_err(err: impl std::fmt::Display) -> IoError {
    IoError::Encode(err.to_string())
}

fn dpi_u16(dpi: f64) -> u16 {
    dpi.round().clamp(1.0, u16::MAX as f64) as u16
}

/// Reads the file's resolution, if it records one in a supported way.
fn read_dpi(path: &Path, format: FileFormat) -> Option<f64> {
    match format {
        FileFormat::Png => read_png_dpi(path),
        FileFormat::Jpeg => read_jfif_dpi(path),
        FileFormat::Bmp | FileFormat::WebP => None,
    }
}

fn read_png_dpi(path: &Path) -> Option<f64> {
    let reader = png::Decoder::new(BufReader::new(File::open(path).ok()?))
        .read_info()
        .ok()?;
    let dims = reader.info().pixel_dims?;
    if dims.unit != png::Unit::Meter || dims.xppu == 0 {
        return None;
    }
    Some(tidy_dpi(dims.xppu as f64 * METERS_PER_INCH))
}

/// Reads the density from a JPEG's JFIF APP0 segment.
fn read_jfif_dpi(path: &Path) -> Option<f64> {
    let mut header = [0u8; 20];
    let mut file = File::open(path).ok()?;
    file.read_exact(&mut header).ok()?;
    // SOI, APP0 marker, length, "JFIF\0", version (2), units, Xdensity, Ydensity
    if header[0..4] != [0xFF, 0xD8, 0xFF, 0xE0] || &header[6..11] != b"JFIF\0" {
        return None;
    }
    let units = header[13];
    let x_density = u16::from_be_bytes([header[14], header[15]]) as f64;
    if x_density == 0.0 {
        return None;
    }
    match units {
        1 => Some(x_density),
        2 => Some(tidy_dpi(x_density * CM_PER_INCH)),
        _ => None,
    }
}

/// Snaps values that only differ from a whole number by unit-conversion
/// rounding (e.g. 2835 px/m → 72.009 DPI) back to that whole number.
fn tidy_dpi(dpi: f64) -> f64 {
    if (dpi - dpi.round()).abs() < 0.05 {
        dpi.round()
    } else {
        dpi
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_from_extension_is_case_insensitive() {
        assert_eq!(
            FileFormat::from_path(Path::new("a.PNG")).unwrap(),
            FileFormat::Png
        );
        assert_eq!(
            FileFormat::from_path(Path::new("a.Jpeg")).unwrap(),
            FileFormat::Jpeg
        );
        assert_eq!(
            FileFormat::from_path(Path::new("a.jpg")).unwrap(),
            FileFormat::Jpeg
        );
        assert_eq!(
            FileFormat::from_path(Path::new("a.bmp")).unwrap(),
            FileFormat::Bmp
        );
        assert_eq!(
            FileFormat::from_path(Path::new("a.WebP")).unwrap(),
            FileFormat::WebP
        );
        assert!(matches!(
            FileFormat::from_path(Path::new("a.gif")),
            Err(IoError::UnsupportedFormat(e)) if e == "gif"
        ));
        assert!(FileFormat::from_path(Path::new("noext")).is_err());
    }

    #[test]
    fn flatten_on_white_mixes_alpha() {
        let mut img = RgbaImage::new(3, 1);
        img.put_pixel(0, 0, image::Rgba([10, 20, 30, 0]));
        img.put_pixel(1, 0, image::Rgba([0, 0, 0, 255]));
        img.put_pixel(2, 0, image::Rgba([0, 0, 0, 128]));
        let rgb = flatten_on_white(&img);
        assert_eq!(rgb.get_pixel(0, 0).0, [255, 255, 255]);
        assert_eq!(rgb.get_pixel(1, 0).0, [0, 0, 0]);
        assert_eq!(rgb.get_pixel(2, 0).0, [127, 127, 127]);
    }

    #[test]
    fn tidy_dpi_snaps_conversion_noise() {
        assert_eq!(tidy_dpi(72.009), 72.0);
        assert_eq!(tidy_dpi(299.999), 300.0);
        assert_eq!(tidy_dpi(96.5), 96.5);
    }
}
