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
    /// Where it was in that document (for animations: the first pose).
    pub rect: PixelRect,
    /// Animation frames on a shared canvas (empty for a single asset;
    /// `image` is then the first frame).
    pub frames: Vec<RgbaImage>,
}

impl Asset {
    pub fn is_animation(&self) -> bool {
        !self.frames.is_empty()
    }

    /// Frames in order (one for a single asset).
    pub fn all_frames(&self) -> Vec<&RgbaImage> {
        if self.frames.is_empty() {
            vec![&self.image]
        } else {
            self.frames.iter().collect()
        }
    }
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
        let name = self.next_name(document, "");
        self.push(name, document, image, rect, Vec::new())
    }

    /// Adds an animation (frames already on a shared canvas), naming it
    /// `<stem> animation <n>`.
    pub fn add_animation(&mut self, source: &str, frames: Vec<RgbaImage>, rect: PixelRect) -> u64 {
        let name = self.next_name(source, "animation ");
        let first = frames.first().cloned().unwrap_or_default();
        self.push(name, source, first, rect, frames)
    }

    fn next_name(&mut self, source: &str, kind: &str) -> String {
        let stem = Path::new(source)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| source.to_owned());
        let n = self
            .counters
            .entry(format!("{stem}\u{0}{kind}"))
            .or_insert(0);
        *n += 1;
        format!("{stem} {kind}{n}")
    }

    fn push(
        &mut self,
        name: String,
        source: &str,
        image: RgbaImage,
        rect: PixelRect,
        frames: Vec<RgbaImage>,
    ) -> u64 {
        self.next_id += 1;
        self.assets.push(Asset {
            id: self.next_id,
            name,
            image,
            source: source.to_owned(),
            rect,
            frames,
        });
        self.next_id
    }

    pub fn remove(&mut self, ids: &[u64]) {
        self.assets.retain(|a| !ids.contains(&a.id));
    }

    /// Writes every asset into `dir` as `<name>.png` and every animation as
    /// `<name>_01.png`, … (` (2)`, ` (3)`, … added to the name when a file
    /// exists); returns the paths written.
    pub fn save_all(&self, dir: &Path) -> Result<Vec<PathBuf>, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let mut written = Vec::new();
        for asset in &self.assets {
            let path = free_entry_path(asset, &dir.join(format!("{}.png", asset.name)), &written);
            written.extend(save_entry(asset, &path)?);
        }
        Ok(written)
    }
}

/// The files an entry saved "as" `path` writes: `path` itself, or for an
/// animation `<stem>_01.png`, … next to it.
pub fn entry_paths(asset: &Asset, path: &Path) -> Vec<PathBuf> {
    if !asset.is_animation() {
        return vec![path.to_path_buf()];
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| asset.name.clone());
    let dir = path.parent().unwrap_or(Path::new("."));
    let digits = asset.frames.len().to_string().len().max(2);
    (1..=asset.frames.len())
        .map(|i| dir.join(format!("{stem}_{i:0digits$}.png")))
        .collect()
}

/// `path`, or `<stem> (2).png`, `<stem> (3).png`, … — the first whose files
/// (for an animation, its whole numbered sequence) neither exist nor are in
/// `taken`.
pub fn free_entry_path(asset: &Asset, path: &Path, taken: &[PathBuf]) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| asset.name.clone());
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut candidate = path.to_path_buf();
    let mut k = 2;
    while entry_paths(asset, &candidate)
        .iter()
        .any(|p| p.exists() || taken.contains(p))
    {
        candidate = dir.join(format!("{stem} ({k}).png"));
        k += 1;
    }
    candidate
}

/// Saves an entry as `path` (an animation as its numbered sequence).
pub fn save_entry(asset: &Asset, path: &Path) -> Result<Vec<PathBuf>, String> {
    let paths = entry_paths(asset, path);
    for (frame, p) in asset.all_frames().into_iter().zip(&paths) {
        save_png(frame, p)?;
    }
    Ok(paths)
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

    #[test]
    fn animations_are_named_and_saved_as_numbered_sequences() {
        let mut lib = AssetLibrary::default();
        let frames: Vec<RgbaImage> = (0..3)
            .map(|i| RgbaImage::from_pixel(4, 5, image::Rgba([i * 50, 0, 0, 255])))
            .collect();
        let id = lib.add_animation("walk.mp4", frames.clone(), PixelRect::new(0, 0, 4, 5));
        lib.add("walk.mp4", frames[0].clone(), PixelRect::new(0, 0, 4, 5));
        lib.add_animation("walk.mp4", frames.clone(), PixelRect::new(0, 0, 4, 5));
        let names: Vec<&str> = lib.assets().iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["walk animation 1", "walk 1", "walk animation 2"]
        );
        let anim = lib.get(id).unwrap();
        assert!(anim.is_animation() && anim.all_frames().len() == 3);
        let dir = tempfile::tempdir().unwrap();
        let paths = save_entry(anim, &dir.path().join("hero.png")).unwrap();
        let files: Vec<String> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files, vec!["hero_01.png", "hero_02.png", "hero_03.png"]);
        assert_eq!(image::open(&paths[2]).unwrap().into_rgba8(), frames[2]);
        std::fs::write(dir.path().join("walk animation 1_01.png"), b"x").unwrap();
        let all = lib.save_all(dir.path()).unwrap();
        let files: Vec<String> = all
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            files[0], "walk animation 1 (2)_01.png",
            "existing sequence not overwritten"
        );
        assert_eq!(files.len(), 7);
    }
}
