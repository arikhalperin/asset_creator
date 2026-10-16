use std::path::{Path, PathBuf};

use game_asset_creator::document::{Document, Layer};
use game_asset_creator::io::{export_document, load_document, IoError};
use image::{Rgba, RgbaImage};

/// A small image with varied colours and alpha, including colour under alpha 0.
fn sample_rgba(width: u32, height: u32) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        Rgba([
            (x * 37 % 256) as u8,
            (y * 59 % 256) as u8,
            ((x + y) * 11 % 256) as u8,
            ((x * 7 + y * 13) % 256) as u8,
        ])
    })
}

fn opaque(image: &RgbaImage) -> RgbaImage {
    let mut out = image.clone();
    out.pixels_mut().for_each(|p| p[3] = 255);
    out
}

fn doc_from(image: RgbaImage, dpi: f64) -> Document {
    let mut doc = Document::new("sample", image.width(), image.height(), dpi).unwrap();
    doc.add_layer(Layer::new("Background", image)).unwrap();
    doc
}

fn tmp(dir: &tempfile::TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
}

fn max_channel_diff(a: &RgbaImage, b: &RgbaImage) -> u8 {
    a.as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

#[test]
fn png_round_trip_is_pixel_exact() {
    let dir = tempfile::tempdir().unwrap();
    let source = tmp(&dir, "source.png");
    let pixels = sample_rgba(31, 17);
    pixels.save(&source).unwrap();

    let doc = load_document(&source).unwrap();
    assert_eq!(doc.layer(0).unwrap().pixels, pixels);

    let exported = tmp(&dir, "exported.png");
    export_document(&doc, &exported).unwrap();
    let reloaded = image::open(&exported).unwrap().into_rgba8();
    assert_eq!(reloaded, pixels);
}

#[test]
fn loading_produces_single_rgba_layer_with_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let pixels = opaque(&sample_rgba(12, 9));
    for name in ["a.png", "b.jpg", "c.JPEG", "d.bmp", "e.webp"] {
        let path = tmp(&dir, name);
        if name.ends_with("webp") {
            export_document(&doc_from(pixels.clone(), 72.0), &path).unwrap();
        } else {
            image::DynamicImage::ImageRgba8(pixels.clone())
                .to_rgb8()
                .save(&path)
                .unwrap();
        }
        let doc = load_document(&path).unwrap();
        assert_eq!((doc.width(), doc.height()), (12, 9), "{name}");
        assert_eq!(doc.layer_count(), 1, "{name}");
        let layer = doc.layer(0).unwrap();
        assert_eq!((layer.width(), layer.height()), (12, 9), "{name}");
        assert_eq!(layer.name, "Background");
        assert_eq!(doc.name, name);
        assert_eq!(doc.source_path.as_deref(), Some(path.as_path()));
        assert_eq!(doc.dpi(), 72.0, "{name} has no stored DPI");
    }
}

#[test]
fn webp_round_trip_is_lossless_with_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let path = tmp(&dir, "x.webp");
    let pixels = sample_rgba(20, 20);
    export_document(&doc_from(pixels.clone(), 72.0), &path).unwrap();
    let loaded = load_document(&path).unwrap();
    // Lossless WebP may discard colour under fully transparent pixels.
    for (a, b) in loaded
        .layer(0)
        .unwrap()
        .pixels
        .pixels()
        .zip(pixels.pixels())
    {
        assert_eq!(a[3], b[3]);
        if b[3] != 0 {
            assert_eq!(a, b);
        }
    }
}

#[test]
fn bmp_round_trip_is_exact_for_opaque_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let path = tmp(&dir, "x.bmp");
    let pixels = opaque(&sample_rgba(13, 7));
    export_document(&doc_from(pixels.clone(), 72.0), &path).unwrap();
    assert_eq!(
        load_document(&path).unwrap().layer(0).unwrap().pixels,
        pixels
    );
}

#[test]
fn jpeg_round_trip_is_close() {
    let dir = tempfile::tempdir().unwrap();
    let path = tmp(&dir, "x.jpg");
    let pixels = RgbaImage::from_fn(32, 32, |x, y| {
        Rgba([(x * 8) as u8, (y * 8) as u8, 128, 255])
    });
    export_document(&doc_from(pixels.clone(), 72.0), &path).unwrap();
    let loaded = load_document(&path).unwrap();
    assert!(max_channel_diff(&loaded.layer(0).unwrap().pixels, &pixels) <= 12);
}

#[test]
fn jpeg_and_bmp_composite_transparency_over_white() {
    let dir = tempfile::tempdir().unwrap();
    let mut pixels = RgbaImage::from_pixel(16, 16, Rgba([0, 0, 0, 0]));
    pixels.put_pixel(0, 0, Rgba([0, 0, 0, 255]));
    let doc = doc_from(pixels, 72.0);
    for name in ["t.jpg", "t.bmp"] {
        let path = tmp(&dir, name);
        export_document(&doc, &path).unwrap();
        let rgba = image::open(&path).unwrap().into_rgba8();
        let corner = rgba.get_pixel(15, 15).0;
        assert!(corner.iter().all(|c| *c >= 250), "{name}: {corner:?}");
        assert_eq!(corner[3], 255);
    }
}

#[test]
fn png_and_webp_keep_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let pixels = RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 77]));
    let doc = doc_from(pixels, 72.0);
    for name in ["a.png", "a.webp"] {
        let path = tmp(&dir, name);
        export_document(&doc, &path).unwrap();
        let rgba = image::open(&path).unwrap().into_rgba8();
        assert!(rgba.pixels().all(|p| p.0 == [10, 20, 30, 77]), "{name}");
    }
}

#[test]
fn export_composites_visible_layers() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = Document::new("layers", 2, 1, 72.0).unwrap();
    doc.add_layer(Layer::new(
        "red",
        RgbaImage::from_pixel(2, 1, Rgba([255, 0, 0, 255])),
    ))
    .unwrap();
    let mut top = RgbaImage::new(2, 1);
    top.put_pixel(1, 0, Rgba([0, 255, 0, 255]));
    doc.add_layer(Layer::new("green-right", top)).unwrap();
    let mut hidden = Layer::new(
        "hidden",
        RgbaImage::from_pixel(2, 1, Rgba([0, 0, 255, 255])),
    );
    hidden.visible = false;
    doc.add_layer(hidden).unwrap();

    let path = tmp(&dir, "c.png");
    export_document(&doc, &path).unwrap();
    let out = image::open(&path).unwrap().into_rgba8();
    assert_eq!(out.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(out.get_pixel(1, 0).0, [0, 255, 0, 255]);
}

#[test]
fn dpi_round_trips_through_png_and_jpeg() {
    let dir = tempfile::tempdir().unwrap();
    for (name, dpi) in [("d.png", 300.0), ("d.jpg", 150.0), ("e.png", 72.0)] {
        let path = tmp(&dir, name);
        export_document(&doc_from(opaque(&sample_rgba(8, 8)), dpi), &path).unwrap();
        assert_eq!(load_document(&path).unwrap().dpi(), dpi, "{name}");
    }
}

#[test]
fn dpi_defaults_to_72_for_bmp_and_webp() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["d.bmp", "d.webp"] {
        let path = tmp(&dir, name);
        export_document(&doc_from(opaque(&sample_rgba(4, 4)), 300.0), &path).unwrap();
        assert_eq!(load_document(&path).unwrap().dpi(), 72.0, "{name}");
    }
}

#[test]
fn jpeg_density_in_dots_per_cm_is_converted() {
    let dir = tempfile::tempdir().unwrap();
    let path = tmp(&dir, "cm.jpg");
    let file = std::fs::File::create(&path).unwrap();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, 90);
    encoder.set_pixel_density(image::codecs::jpeg::PixelDensity {
        density: (118, 118),
        unit: image::codecs::jpeg::PixelDensityUnit::Centimeters,
    });
    encoder
        .encode(&[0u8; 4 * 4 * 3], 4, 4, image::ExtendedColorType::Rgb8)
        .unwrap();
    // 118 dots/cm × 2.54 = 299.72 DPI
    assert!((load_document(&path).unwrap().dpi() - 299.72).abs() < 0.01);
}

#[test]
fn unsupported_and_corrupt_files_fail() {
    let dir = tempfile::tempdir().unwrap();
    let gif = tmp(&dir, "x.gif");
    std::fs::write(&gif, b"GIF89a").unwrap();
    assert!(matches!(
        load_document(&gif),
        Err(IoError::UnsupportedFormat(_))
    ));

    let corrupt = tmp(&dir, "bad.png");
    std::fs::write(&corrupt, b"not a png at all").unwrap();
    assert!(load_document(&corrupt).is_err());

    assert!(load_document(Path::new("/nonexistent/file.png")).is_err());

    let doc = doc_from(sample_rgba(2, 2), 72.0);
    assert!(matches!(
        export_document(&doc, &tmp(&dir, "out.tiff")),
        Err(IoError::UnsupportedFormat(_))
    ));
}
