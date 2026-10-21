//! The asset library: extracted assets and animations with their metadata,
//! categories, and tags, kept on disk (PNGs plus `library.json`) when opened
//! on a folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::document::PixelRect;

/// Categories a new library starts with.
pub const DEFAULT_CATEGORIES: [&str; 8] = [
    "chalice",
    "skeleton",
    "potion",
    "deathtrap",
    "wall",
    "prop",
    "pattern",
    "other",
];

/// Category of tiles saved by the pattern tool (never chosen by auto-tagging).
pub const PATTERN: &str = "pattern";

/// `library.json` format version (2 added the "pattern" category).
const MANIFEST_VERSION: u32 = 2;

/// The category that always exists (assets of removed categories move here).
pub const OTHER: &str = "other";

/// How an asset got its category.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CategorySource {
    /// Not tagged yet (or no tagging model).
    #[default]
    None,
    /// Set by auto-tagging.
    Auto,
    /// Set by the user; never changed by automatic tagging.
    Manual,
}

/// Where an asset came from and how it was extracted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Origin {
    /// The source file, when the document or video had one.
    pub file: Option<String>,
    /// Video frame (first appearance), for assets from videos.
    pub frame: Option<u64>,
    /// "auto_detect", "extract_object", "selection", "video", …
    pub method: String,
    /// The extraction's parameters.
    pub params: serde_json::Value,
}

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
    pub category: String,
    pub category_source: CategorySource,
    /// Auto-tagging confidence (0–1).
    pub confidence: Option<f32>,
    pub tags: Vec<String>,
    pub origin: Origin,
    /// Seconds since the Unix epoch.
    pub created: u64,
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

    /// Case-insensitive match of `query` against name, tags, category, and
    /// source file name (empty matches everything).
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let file = self
            .origin
            .file
            .as_deref()
            .and_then(|f| Path::new(f).file_name())
            .map(|f| f.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        self.name.to_lowercase().contains(&q)
            || self.category.to_lowercase().contains(&q)
            || self.source.to_lowercase().contains(&q)
            || file.contains(&q)
            || self.tags.iter().any(|t| t.to_lowercase().contains(&q))
    }
}

/// `library.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    categories: Vec<String>,
    next_id: u64,
    #[serde(default)]
    counters: HashMap<String, u32>,
    assets: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    id: u64,
    name: String,
    kind: String,
    files: Vec<String>,
    width: u32,
    height: u32,
    category: String,
    category_source: CategorySource,
    confidence: Option<f32>,
    tags: Vec<String>,
    source: EntrySource,
    extraction: Extraction,
    created: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct EntrySource {
    document: String,
    file: Option<String>,
    frame: Option<u64>,
    rect: [u32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Extraction {
    method: String,
    params: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssetLibrary {
    assets: Vec<Asset>,
    next_id: u64,
    /// Assets made per document stem (for `<stem> <n>` names).
    counters: HashMap<String, u32>,
    categories: Vec<String>,
    /// The folder it is kept in (`None`: in memory only).
    dir: Option<PathBuf>,
    /// Problems found while loading or saving, for the panel to show.
    notices: Vec<String>,
    /// Entries that could not be loaded: kept (metadata and files untouched)
    /// so they survive later saves and can be repaired.
    broken: Vec<Entry>,
}

impl Default for AssetLibrary {
    fn default() -> Self {
        Self {
            assets: Vec::new(),
            next_id: 0,
            counters: HashMap::new(),
            categories: DEFAULT_CATEGORIES.iter().map(|c| c.to_string()).collect(),
            dir: None,
            notices: Vec::new(),
            broken: Vec::new(),
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Writes `bytes` to `path` through a temporary file and a rename.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn save_png_atomic(image: &RgbaImage, path: &Path) -> Result<(), String> {
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| e.to_string())?;
    write_atomic(path, &bytes).map_err(|e| format!("could not save {}: {e}", path.display()))
}

impl AssetLibrary {
    /// The default library folder: `<app data>/Game Asset Creator/library`
    /// (`GAC_LIBRARY_DIR` overrides).
    pub fn default_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("GAC_LIBRARY_DIR") {
            return PathBuf::from(dir);
        }
        dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Game Asset Creator")
            .join("library")
    }

    /// Opens (or creates) the library kept in `dir`. Entries whose files are
    /// missing or unreadable are skipped; a damaged `library.json` is kept
    /// aside as `library.json.bad` and an empty library starts. Problems are
    /// listed in [`Self::notices`].
    pub fn open(dir: &Path) -> Self {
        let mut lib = Self {
            dir: Some(dir.to_path_buf()),
            ..Self::default()
        };
        if let Err(e) = std::fs::create_dir_all(dir.join("assets")) {
            lib.notices.push(format!(
                "cannot use the library folder {}: {e}",
                dir.display()
            ));
            lib.dir = None;
            return lib;
        }
        let path = dir.join("library.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return lib;
        };
        let manifest: Manifest = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                let _ = std::fs::rename(&path, dir.join("library.json.bad"));
                lib.notices.push(format!(
                    "library.json was damaged ({e}); it was kept as library.json.bad"
                ));
                return lib;
            }
        };
        if !manifest.categories.is_empty() {
            lib.categories = manifest.categories;
        }
        if !lib.categories.iter().any(|c| c == OTHER) {
            lib.categories.push(OTHER.to_owned());
        }
        // Libraries from before patterns get the category once (a later
        // removal by the user sticks).
        if manifest.version < 2 && !lib.categories.iter().any(|c| c == PATTERN) {
            let other = lib.categories.iter().position(|c| c == OTHER).unwrap();
            lib.categories.insert(other, PATTERN.to_owned());
        }
        lib.next_id = manifest.next_id;
        lib.counters = manifest.counters;
        for entry in manifest.assets {
            match load_entry(dir, &entry) {
                Ok(asset) => {
                    lib.next_id = lib.next_id.max(asset.id);
                    lib.assets.push(asset);
                }
                Err(e) => {
                    lib.notices.push(format!("skipped \"{}\": {e}", entry.name));
                    lib.next_id = lib.next_id.max(entry.id);
                    lib.broken.push(entry);
                }
            }
        }
        lib
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    pub fn notices(&self) -> &[String] {
        &self.notices
    }

    pub fn clear_notices(&mut self) {
        self.notices.clear();
    }

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

    pub fn categories(&self) -> &[String] {
        &self.categories
    }

    /// Adds an asset from `document` (its name), naming it `<stem> <n>`.
    pub fn add(&mut self, document: &str, image: RgbaImage, rect: PixelRect) -> u64 {
        self.add_with(document, image, rect, Origin::default())
    }

    /// Adds an asset with its origin.
    pub fn add_with(
        &mut self,
        document: &str,
        image: RgbaImage,
        rect: PixelRect,
        origin: Origin,
    ) -> u64 {
        let name = self.next_name(document, "");
        self.push(name, document, image, rect, Vec::new(), origin)
    }

    /// Adds an animation (frames already on a shared canvas), naming it
    /// `<stem> animation <n>`.
    pub fn add_animation(&mut self, source: &str, frames: Vec<RgbaImage>, rect: PixelRect) -> u64 {
        self.add_animation_with(source, frames, rect, Origin::default())
    }

    pub fn add_animation_with(
        &mut self,
        source: &str,
        frames: Vec<RgbaImage>,
        rect: PixelRect,
        origin: Origin,
    ) -> u64 {
        let name = self.next_name(source, "animation ");
        let first = frames.first().cloned().unwrap_or_default();
        self.push(name, source, first, rect, frames, origin)
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
        origin: Origin,
    ) -> u64 {
        self.next_id += 1;
        let asset = Asset {
            id: self.next_id,
            name,
            image,
            source: source.to_owned(),
            rect,
            frames,
            category: OTHER.to_owned(),
            category_source: CategorySource::None,
            confidence: None,
            tags: Vec::new(),
            origin,
            created: now(),
        };
        if let Some(dir) = &self.dir {
            for (file, frame) in entry_files(&asset).iter().zip(asset.all_frames()) {
                if let Err(e) = save_png_atomic(frame, &dir.join("assets").join(file)) {
                    self.notices.push(e);
                }
            }
        }
        self.assets.push(asset);
        self.persist();
        self.next_id
    }

    /// Removes assets (and their files).
    pub fn remove(&mut self, ids: &[u64]) {
        let (gone, kept): (Vec<Asset>, Vec<Asset>) = std::mem::take(&mut self.assets)
            .into_iter()
            .partition(|a| ids.contains(&a.id));
        self.assets = kept;
        self.persist();
        if let Some(dir) = &self.dir {
            for asset in &gone {
                for file in entry_files(asset) {
                    let _ = std::fs::remove_file(dir.join("assets").join(file));
                }
            }
        }
    }

    fn asset_mut(&mut self, id: u64) -> Option<&mut Asset> {
        self.assets.iter_mut().find(|a| a.id == id)
    }

    pub fn rename(&mut self, id: u64, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let Some(asset) = self.asset_mut(id) else {
            return false;
        };
        asset.name = name.to_owned();
        self.persist();
        true
    }

    /// Sets the category by hand (marked manual).
    pub fn set_category(&mut self, ids: &[u64], category: &str) {
        if !self.categories.iter().any(|c| c == category) {
            return;
        }
        for a in self.assets.iter_mut().filter(|a| ids.contains(&a.id)) {
            a.category = category.to_owned();
            a.category_source = CategorySource::Manual;
            a.confidence = None;
        }
        self.persist();
    }

    /// Applies an automatic tag unless the category was set by hand (or
    /// `force`, for an explicit Auto-Tag).
    pub fn apply_auto_tag(
        &mut self,
        id: u64,
        category: &str,
        confidence: f32,
        force: bool,
    ) -> bool {
        if !self.categories.iter().any(|c| c == category) {
            return false;
        }
        let Some(a) = self.asset_mut(id) else {
            return false;
        };
        if a.category_source == CategorySource::Manual && !force {
            return false;
        }
        a.category = category.to_owned();
        a.category_source = CategorySource::Auto;
        a.confidence = Some(confidence);
        self.persist();
        true
    }

    pub fn set_tags(&mut self, id: u64, tags: Vec<String>) {
        let mut tags: Vec<String> = tags
            .into_iter()
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .collect();
        tags.dedup();
        if let Some(a) = self.asset_mut(id) {
            a.tags = tags;
        }
        self.persist();
    }

    /// Adds a category (ignored if it exists or is blank).
    pub fn add_category(&mut self, name: &str) -> bool {
        let name = name.trim().to_lowercase();
        if name.is_empty() || self.categories.contains(&name) {
            return false;
        }
        let other = self
            .categories
            .iter()
            .position(|c| c == OTHER)
            .unwrap_or(self.categories.len());
        self.categories.insert(other, name);
        self.persist();
        true
    }

    /// Renames a category (and its assets'); "other" cannot be renamed.
    pub fn rename_category(&mut self, from: &str, to: &str) -> bool {
        let to = to.trim().to_lowercase();
        if from == OTHER || to.is_empty() || self.categories.contains(&to) {
            return false;
        }
        let Some(i) = self.categories.iter().position(|c| c == from) else {
            return false;
        };
        self.categories[i] = to.clone();
        for a in self.assets.iter_mut().filter(|a| a.category == from) {
            a.category = to.clone();
        }
        self.persist();
        true
    }

    /// Removes a category; its assets become "other". "other" stays.
    pub fn remove_category(&mut self, name: &str) -> bool {
        if name == OTHER {
            return false;
        }
        let before = self.categories.len();
        self.categories.retain(|c| c != name);
        if self.categories.len() == before {
            return false;
        }
        for a in self.assets.iter_mut().filter(|a| a.category == name) {
            a.category = OTHER.to_owned();
            a.category_source = CategorySource::None;
            a.confidence = None;
        }
        self.persist();
        true
    }

    /// Writes `library.json` (no-op for an in-memory library).
    fn persist(&mut self) {
        let Some(dir) = &self.dir else { return };
        let manifest = Manifest {
            version: MANIFEST_VERSION,
            categories: self.categories.clone(),
            next_id: self.next_id,
            counters: self.counters.clone(),
            assets: self
                .assets
                .iter()
                .map(entry_of)
                .chain(self.broken.iter().cloned())
                .collect(),
        };
        let text = serde_json::to_string_pretty(&manifest).expect("manifest serializes");
        if let Err(e) = write_atomic(&dir.join("library.json"), text.as_bytes()) {
            self.notices
                .push(format!("could not save the library: {e}"));
        }
    }

    /// Writes every asset into `dir` as `<name>.png` and every animation as
    /// `<name>_01.png`, … (` (2)`, ` (3)`, … added to the name when a file
    /// exists); returns the paths written.
    pub fn save_all(&self, dir: &Path) -> Result<Vec<PathBuf>, String> {
        save_assets(self.assets.iter(), dir)
    }
}

/// Saves `assets` into `dir` (unique names; animations as sequences).
pub fn save_assets<'a>(
    assets: impl Iterator<Item = &'a Asset>,
    dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut written = Vec::new();
    for asset in assets {
        let path = free_entry_path(asset, &dir.join(format!("{}.png", asset.name)), &written);
        written.extend(save_entry(asset, &path)?);
    }
    Ok(written)
}

/// File names of an asset inside the library's `assets/` folder.
fn entry_files(asset: &Asset) -> Vec<String> {
    if asset.is_animation() {
        (1..=asset.frames.len())
            .map(|i| format!("{}_{i:02}.png", asset.id))
            .collect()
    } else {
        vec![format!("{}.png", asset.id)]
    }
}

fn entry_of(a: &Asset) -> Entry {
    Entry {
        id: a.id,
        name: a.name.clone(),
        kind: if a.is_animation() {
            "animation"
        } else {
            "single"
        }
        .to_owned(),
        files: entry_files(a),
        width: a.image.width(),
        height: a.image.height(),
        category: a.category.clone(),
        category_source: a.category_source,
        confidence: a.confidence,
        tags: a.tags.clone(),
        source: EntrySource {
            document: a.source.clone(),
            file: a.origin.file.clone(),
            frame: a.origin.frame,
            rect: [a.rect.x, a.rect.y, a.rect.width, a.rect.height],
        },
        extraction: Extraction {
            method: a.origin.method.clone(),
            params: a.origin.params.clone(),
        },
        created: a.created,
    }
}

fn load_entry(dir: &Path, e: &Entry) -> Result<Asset, String> {
    if e.files.is_empty() {
        return Err("no image files".to_owned());
    }
    let mut images = Vec::new();
    for file in &e.files {
        let path = dir.join("assets").join(file);
        let img = image::open(&path)
            .map_err(|err| format!("{file}: {err}"))?
            .into_rgba8();
        images.push(img);
    }
    let animation = e.kind == "animation";
    let image = images[0].clone();
    Ok(Asset {
        id: e.id,
        name: e.name.clone(),
        image,
        source: e.source.document.clone(),
        rect: PixelRect::new(
            e.source.rect[0],
            e.source.rect[1],
            e.source.rect[2],
            e.source.rect[3],
        ),
        frames: if animation { images } else { Vec::new() },
        category: e.category.clone(),
        category_source: e.category_source,
        confidence: e.confidence,
        tags: e.tags.clone(),
        origin: Origin {
            file: e.source.file.clone(),
            frame: e.source.frame,
            method: e.extraction.method.clone(),
            params: e.extraction.params.clone(),
        },
        created: e.created,
    })
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
