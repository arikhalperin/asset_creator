//! The session's extracted assets (a stub library until tagging and export
//! arrive).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use image::RgbaImage;

use crate::document::PixelRect;

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub id: u64,
    pub name: String,
    /// Tightly cropped RGBA.
    pub image: RgbaImage,
    /// Name of the document it came from.
    pub source: String,
    /// Where it was in that document.
    pub rect: PixelRect,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetLibrary {
    assets: Vec<Asset>,
    next_id: u64,
    /// Assets made per document stem (for `<stem> <n>` names).
    counters: HashMap<String, u32>,
}

impl AssetLibrary {
    pub fn assets(&self) -> &[Asset] {
        &self.assets
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    pub fn get(&self, id: u64) -> Option<&Asset> {
        self.assets.iter().find(|a| a.id == id)
    }

    /// Adds an asset from `document` (its name), naming it `<stem> <n>`.
    pub fn add(&mut self, document: &str, image: RgbaImage, rect: PixelRect) -> u64 {
        let stem = Path::new(document)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| document.to_owned());
        let n = self.counters.entry(stem.clone()).or_insert(0);
        *n += 1;
        self.next_id += 1;
        self.assets.push(Asset {
            id: self.next_id,
            name: format!("{stem} {n}"),
            image,
            source: document.to_owned(),
            rect,
        });
        self.next_id
    }

    pub fn remove(&mut self, ids: &[u64]) {
        self.assets.retain(|a| !ids.contains(&a.id));
    }

    /// Writes every asset into `dir` as `<name>.png` (`<name> (2).png`, … for
    /// duplicates); returns the paths written.
    pub fn save_all(&self, dir: &Path) -> Result<Vec<PathBuf>, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let mut written = Vec::new();
        for asset in &self.assets {
            let mut path = dir.join(format!("{}.png", asset.name));
            let mut k = 2;
            while path.exists() || written.contains(&path) {
                path = dir.join(format!("{} ({k}).png", asset.name));
                k += 1;
            }
            save_png(&asset.image, &path)?;
            written.push(path);
        }
        Ok(written)
    }
}

/// Saves RGBA as PNG.
pub fn save_png(image: &RgbaImage, path: &Path) -> Result<(), String> {
    image
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| format!("could not save {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_count_per_document_and_save_all_dedupes() {
        let mut lib = AssetLibrary::default();
        let img = RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 128]));
        let a = lib.add("sheet.png", img.clone(), PixelRect::new(0, 0, 3, 2));
        lib.add("sheet.png", img.clone(), PixelRect::new(5, 0, 3, 2));
        lib.add("photo", img.clone(), PixelRect::new(0, 0, 3, 2));
        let names: Vec<&str> = lib.assets().iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["sheet 1", "sheet 2", "photo 1"]);
        lib.remove(&[a]);
        lib.add("sheet.png", img.clone(), PixelRect::new(0, 0, 3, 2));
        lib.add("x/sheet 3.png", img, PixelRect::new(0, 0, 3, 2)); // stem "sheet 3"
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("sheet 2.png"), b"existing").unwrap();
        let paths = lib.save_all(dir.path()).unwrap();
        let files: Vec<String> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            files,
            vec![
                "sheet 2 (2).png",
                "photo 1.png",
                "sheet 3.png",
                "sheet 3 1.png"
            ],
            "existing files are not overwritten"
        );
        let back = image::open(&paths[0]).unwrap().into_rgba8();
        assert_eq!(&back, &lib.assets()[0].image);
    }
}
