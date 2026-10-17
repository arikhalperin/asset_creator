//! The native `.gac` project format: a ZIP archive with a JSON manifest,
//! one RGBA PNG per layer, grayscale PNGs for masks, and the selection.

use std::fs::File;
use std::io::{Cursor, Read, Seek, Write};
use std::path::Path;

use image::{GrayImage, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::document::{BlendMode, Document, Layer};

pub const FORMAT_NAME: &str = "game-asset-creator";
pub const FORMAT_VERSION: u32 = 1;
pub const EXTENSION: &str = "gac";

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not a Game Asset Creator project: {0}")]
    Archive(String),
    #[error("invalid project manifest: {0}")]
    Manifest(String),
    #[error("this project was saved by a newer version (format version {0})")]
    UnsupportedVersion(u32),
    #[error("could not read image {path}: {message}")]
    Image { path: String, message: String },
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub dpi: f64,
    pub active_layer: usize,
    pub layers: Vec<LayerEntry>,
    pub selection: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct LayerEntry {
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub blend_mode: String,
    pub image: String,
    pub mask: Option<String>,
    pub mask_enabled: bool,
}

fn png_bytes(
    encode: impl FnOnce(&mut Cursor<Vec<u8>>) -> image::ImageResult<()>,
) -> Result<Vec<u8>, ProjectError> {
    let mut out = Cursor::new(Vec::new());
    encode(&mut out).map_err(|e| ProjectError::Io(std::io::Error::other(e.to_string())))?;
    Ok(out.into_inner())
}

/// Writes `doc` as a `.gac` archive to `writer`.
pub fn write_project<W: Write + Seek>(doc: &Document, writer: W) -> Result<W, ProjectError> {
    let mut zip = zip::ZipWriter::new(writer);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let add = |zip: &mut zip::ZipWriter<W>, name: &str, bytes: &[u8]| -> Result<(), ProjectError> {
        zip.start_file(name, options)
            .map_err(|e| ProjectError::Archive(e.to_string()))?;
        zip.write_all(bytes)?;
        Ok(())
    };
    let mut entries = Vec::new();
    for (i, layer) in doc.layers().iter().enumerate() {
        let image = format!("layers/{i}.png");
        add(
            &mut zip,
            &image,
            &png_bytes(|c| layer.pixels.write_to(c, ImageFormat::Png))?,
        )?;
        let mask = match layer.mask() {
            Some(m) => {
                let path = format!("masks/{i}.png");
                add(
                    &mut zip,
                    &path,
                    &png_bytes(|c| m.write_to(c, ImageFormat::Png))?,
                )?;
                Some(path)
            }
            None => None,
        };
        entries.push(LayerEntry {
            name: layer.name.clone(),
            visible: layer.visible,
            opacity: layer.opacity(),
            blend_mode: layer.blend_mode.key().to_owned(),
            image,
            mask,
            mask_enabled: layer.mask_enabled,
        });
    }
    let selection = match doc.selection() {
        Some(sel) => {
            add(
                &mut zip,
                "selection.png",
                &png_bytes(|c| sel.write_to(c, ImageFormat::Png))?,
            )?;
            Some("selection.png".to_owned())
        }
        None => None,
    };
    let manifest = Manifest {
        format: FORMAT_NAME.to_owned(),
        version: FORMAT_VERSION,
        width: doc.width(),
        height: doc.height(),
        dpi: doc.dpi(),
        active_layer: doc.active_layer(),
        layers: entries,
        selection,
    };
    let json =
        serde_json::to_vec_pretty(&manifest).map_err(|e| ProjectError::Manifest(e.to_string()))?;
    add(&mut zip, "manifest.json", &json)?;
    zip.finish()
        .map_err(|e| ProjectError::Archive(e.to_string()))
}

/// Saves atomically: writes a temporary file next to `path`, then renames it.
pub fn save_project(doc: &Document, path: &Path) -> Result<(), ProjectError> {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project.gac".to_owned());
    let temp = path.with_file_name(format!(".{file_name}.tmp"));
    let result = (|| {
        let file = File::create(&temp)?;
        let file = write_project(doc, file)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn read_entry<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, ProjectError> {
    let mut file = zip
        .by_name(name)
        .map_err(|_| ProjectError::Manifest(format!("missing entry {name}")))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn decode(bytes: &[u8], path: &str) -> Result<image::DynamicImage, ProjectError> {
    image::load_from_memory_with_format(bytes, ImageFormat::Png).map_err(|e| ProjectError::Image {
        path: path.to_owned(),
        message: e.to_string(),
    })
}

fn check_size(path: &str, (w, h): (u32, u32), manifest: &Manifest) -> Result<(), ProjectError> {
    if (w, h) != (manifest.width, manifest.height) {
        return Err(ProjectError::Image {
            path: path.to_owned(),
            message: format!(
                "is {w}x{h}, expected {}x{}",
                manifest.width, manifest.height
            ),
        });
    }
    Ok(())
}

/// Reads a `.gac` archive.
pub fn read_project<R: Read + Seek>(reader: R, name: &str) -> Result<Document, ProjectError> {
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| ProjectError::Archive(e.to_string()))?;
    let manifest: Manifest = serde_json::from_slice(&read_entry(&mut zip, "manifest.json")?)
        .map_err(|e| ProjectError::Manifest(e.to_string()))?;
    if manifest.format != FORMAT_NAME {
        return Err(ProjectError::Manifest(format!(
            "unknown format \"{}\"",
            manifest.format
        )));
    }
    if manifest.version != FORMAT_VERSION {
        return Err(ProjectError::UnsupportedVersion(manifest.version));
    }
    if manifest.layers.is_empty() {
        return Err(ProjectError::Manifest(
            "the project has no layers".to_owned(),
        ));
    }
    let mut doc = Document::new(name, manifest.width, manifest.height, manifest.dpi)
        .map_err(|e| ProjectError::Manifest(e.to_string()))?;
    for entry in &manifest.layers {
        let pixels: RgbaImage =
            decode(&read_entry(&mut zip, &entry.image)?, &entry.image)?.into_rgba8();
        check_size(&entry.image, pixels.dimensions(), &manifest)?;
        let mut layer = Layer::new(entry.name.clone(), pixels);
        layer.visible = entry.visible;
        layer.set_opacity(entry.opacity);
        layer.blend_mode = BlendMode::from_key(&entry.blend_mode).ok_or_else(|| {
            ProjectError::Manifest(format!("unknown blend mode \"{}\"", entry.blend_mode))
        })?;
        if let Some(path) = &entry.mask {
            let mask: GrayImage = decode(&read_entry(&mut zip, path)?, path)?.into_luma8();
            check_size(path, mask.dimensions(), &manifest)?;
            layer
                .set_mask(Some(mask))
                .map_err(|e| ProjectError::Manifest(e.to_string()))?;
        }
        layer.mask_enabled = entry.mask_enabled;
        doc.add_layer(layer)
            .map_err(|e| ProjectError::Manifest(e.to_string()))?;
    }
    if let Some(path) = &manifest.selection {
        let sel = decode(&read_entry(&mut zip, path)?, path)?.into_luma8();
        check_size(path, sel.dimensions(), &manifest)?;
        doc.set_selection(Some(sel))
            .map_err(|e| ProjectError::Manifest(e.to_string()))?;
    }
    if manifest.active_layer >= doc.layer_count() {
        return Err(ProjectError::Manifest(
            "active layer out of range".to_owned(),
        ));
    }
    doc.set_active_layer(manifest.active_layer);
    Ok(doc)
}

/// Loads a `.gac` file into a document named after the file.
pub fn load_project(path: &Path) -> Result<Document, ProjectError> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_owned());
    let mut doc = read_project(File::open(path)?, &name)?;
    doc.source_path = Some(path.to_path_buf());
    Ok(doc)
}

/// Whether `path` names a project file.
pub fn is_project_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Luma, Rgba};

    fn sample_document() -> Document {
        let mut doc = Document::new("art.gac", 7, 5, 144.0).unwrap();
        let a = RgbaImage::from_fn(7, 5, |x, y| {
            Rgba([(x * 30) as u8, (y * 40) as u8, 9, ((x + y) * 20) as u8])
        });
        doc.add_layer(Layer::new("Base", a)).unwrap();
        let mut b = Layer::new(
            "Shadow",
            RgbaImage::from_pixel(7, 5, Rgba([10, 20, 30, 128])),
        );
        b.blend_mode = BlendMode::Multiply;
        b.set_opacity(0.37);
        b.visible = false;
        let mask = GrayImage::from_fn(7, 5, |x, _| Luma([(x * 36) as u8]));
        b.set_mask(Some(mask)).unwrap();
        b.mask_enabled = false;
        doc.add_layer(b).unwrap();
        let mut c = Layer::new("Glow ✨", RgbaImage::new(7, 5));
        c.blend_mode = BlendMode::Screen;
        doc.add_layer(c).unwrap();
        doc.set_selection(Some(crate::selection::rect_mask(
            7,
            5,
            (1.0, 1.0),
            (4.0, 3.0),
        )))
        .unwrap();
        doc.set_active_layer(1);
        doc
    }

    fn round_trip(doc: &Document) -> Document {
        let bytes = write_project(doc, Cursor::new(Vec::new()))
            .unwrap()
            .into_inner();
        read_project(Cursor::new(bytes), &doc.name).unwrap()
    }

    #[test]
    fn multi_layer_document_round_trips_exactly() {
        let doc = sample_document();
        let back = round_trip(&doc);
        assert_eq!(back, doc);
        // Every blend mode survives.
        for mode in BlendMode::ALL {
            let mut d = Document::new_blank("m", 2, 2, 72.0).unwrap();
            d.layer_mut(0).unwrap().blend_mode = mode;
            assert_eq!(round_trip(&d), d);
        }
    }

    #[test]
    fn archive_layout_and_manifest() {
        let doc = sample_document();
        let bytes = write_project(&doc, Cursor::new(Vec::new()))
            .unwrap()
            .into_inner();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "layers/0.png",
                "layers/1.png",
                "layers/2.png",
                "manifest.json",
                "masks/1.png",
                "selection.png"
            ]
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&read_entry(&mut zip, "manifest.json").unwrap()).unwrap();
        assert_eq!(manifest["format"], "game-asset-creator");
        assert_eq!(manifest["version"], 1);
        assert_eq!(
            (manifest["width"].as_u64(), manifest["height"].as_u64()),
            (Some(7), Some(5))
        );
        assert_eq!(manifest["dpi"], 144.0);
        assert_eq!(manifest["active_layer"], 1);
        assert_eq!(manifest["selection"], "selection.png");
        let layers = manifest["layers"].as_array().unwrap();
        assert_eq!(layers[0]["name"], "Base");
        assert_eq!(layers[0]["mask"], serde_json::Value::Null);
        assert_eq!(layers[1]["blend_mode"], "multiply");
        assert_eq!(layers[1]["visible"], false);
        assert_eq!(layers[1]["mask"], "masks/1.png");
        assert_eq!(layers[1]["mask_enabled"], false);
        assert_eq!(layers[2]["image"], "layers/2.png");
        // Layer images are RGBA PNGs, masks grayscale.
        let layer_png = decode(&read_entry(&mut zip, "layers/0.png").unwrap(), "l").unwrap();
        assert!(matches!(layer_png, image::DynamicImage::ImageRgba8(_)));
        let mask_png = decode(&read_entry(&mut zip, "masks/1.png").unwrap(), "m").unwrap();
        assert!(matches!(mask_png, image::DynamicImage::ImageLuma8(_)));
    }

    #[test]
    fn save_and_load_files_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Scene.GAC");
        let doc = sample_document();
        save_project(&doc, &path).unwrap();
        assert!(is_project_path(&path));
        let files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(files.len(), 1, "no temporary file left behind: {files:?}");
        let loaded = load_project(&path).unwrap();
        assert_eq!(loaded.name, "Scene.GAC");
        assert_eq!(loaded.source_path.as_deref(), Some(path.as_path()));
        assert_eq!(loaded.layers(), doc.layers());
        // Saving over an existing file replaces it.
        let mut changed = doc.clone();
        changed.layer_mut(0).unwrap().name = "Renamed".into();
        save_project(&changed, &path).unwrap();
        assert_eq!(
            load_project(&path).unwrap().layer(0).unwrap().name,
            "Renamed"
        );
    }

    fn rewrite(
        doc: &Document,
        edit: impl FnOnce(&mut serde_json::Value),
        drop: Option<&str>,
    ) -> Vec<u8> {
        let mut edit = Some(edit);
        let bytes = write_project(doc, Cursor::new(Vec::new()))
            .unwrap()
            .into_inner();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
        for name in names {
            if Some(name.as_str()) == drop {
                continue;
            }
            let mut data = read_entry(&mut zip, &name).unwrap();
            if name == "manifest.json" {
                let mut v: serde_json::Value = serde_json::from_slice(&data).unwrap();
                if let Some(edit) = edit.take() {
                    edit(&mut v);
                }
                data = serde_json::to_vec(&v).unwrap();
            }
            out.start_file(name, opts).unwrap();
            out.write_all(&data).unwrap();
        }
        out.finish().unwrap().into_inner()
    }

    #[test]
    fn malformed_projects_are_rejected() {
        let doc = sample_document();
        let load = |bytes: Vec<u8>| read_project(Cursor::new(bytes), "x");
        assert!(matches!(
            load(b"not a zip".to_vec()),
            Err(ProjectError::Archive(_))
        ));
        assert!(matches!(
            load(rewrite(&doc, |_| {}, Some("manifest.json"))),
            Err(ProjectError::Manifest(_))
        ));
        assert!(matches!(
            load(rewrite(&doc, |m| m["format"] = "other".into(), None)),
            Err(ProjectError::Manifest(_))
        ));
        assert!(matches!(
            load(rewrite(&doc, |m| m["version"] = 2.into(), None)),
            Err(ProjectError::UnsupportedVersion(2))
        ));
        assert!(matches!(
            load(rewrite(&doc, |_| {}, Some("layers/1.png"))),
            Err(ProjectError::Manifest(_))
        ));
        assert!(matches!(
            load(rewrite(&doc, |m| m["width"] = 8.into(), None)),
            Err(ProjectError::Image { .. })
        ));
        assert!(matches!(
            load(rewrite(
                &doc,
                |m| m["layers"][0]["blend_mode"] = "dissolve".into(),
                None
            )),
            Err(ProjectError::Manifest(_))
        ));
        assert!(matches!(
            load(rewrite(&doc, |m| m["active_layer"] = 9.into(), None)),
            Err(ProjectError::Manifest(_))
        ));
        assert!(matches!(
            load(rewrite(&doc, |m| m["layers"] = serde_json::json!([]), None)),
            Err(ProjectError::Manifest(_))
        ));
    }
}
