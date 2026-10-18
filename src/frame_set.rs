//! The in-session Frame Set: frames extracted from videos, kept for the
//! asset-extraction pipeline. Frames are stored losslessly compressed.

use std::path::PathBuf;
use std::sync::Arc;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ImageEncoder, RgbaImage};

/// Height of Frame Set thumbnails in pixels.
pub const THUMBNAIL_HEIGHT: u32 = 72;

#[derive(Debug, Clone, PartialEq)]
pub struct FrameSetEntry {
    /// Unique within the session (assigned when added to a set).
    pub id: u64,
    pub source: PathBuf,
    pub video_name: String,
    pub frame: u64,
    /// Seconds from the start of the video.
    pub timestamp: f64,
    pub width: u32,
    pub height: u32,
    png: Arc<Vec<u8>>,
    pub thumbnail: RgbaImage,
}

impl FrameSetEntry {
    pub fn new(
        source: PathBuf,
        video_name: String,
        frame: u64,
        timestamp: f64,
        image: &RgbaImage,
    ) -> Self {
        let mut png = Vec::new();
        PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Sub)
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                image::ExtendedColorType::Rgba8,
            )
            .expect("encoding to memory");
        let (w, h) = image.dimensions();
        let tw = ((w as f32 * THUMBNAIL_HEIGHT as f32 / h as f32).round() as u32).max(1);
        Self {
            id: 0,
            source,
            video_name,
            frame,
            timestamp,
            width: w,
            height: h,
            png: Arc::new(png),
            thumbnail: image::imageops::thumbnail(image, tw, THUMBNAIL_HEIGHT),
        }
    }

    /// The full-resolution frame.
    pub fn image(&self) -> RgbaImage {
        image::load_from_memory_with_format(&self.png, image::ImageFormat::Png)
            .expect("frame stored as PNG")
            .into_rgba8()
    }

    /// Name for a document opened from this frame.
    pub fn document_name(&self) -> String {
        let stem = self
            .source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.video_name.clone());
        frame_document_name(&stem, self.frame)
    }
}

/// `<video stem> frame <N>`.
pub fn frame_document_name(stem: &str, frame: u64) -> String {
    format!("{stem} frame {frame}")
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameSet {
    entries: Vec<FrameSetEntry>,
    next_id: u64,
}

impl FrameSet {
    pub fn entries(&self) -> &[FrameSetEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, id: u64) -> Option<&FrameSetEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Appends frames in order, assigning their ids.
    pub fn extend(&mut self, frames: impl IntoIterator<Item = FrameSetEntry>) {
        for mut entry in frames {
            self.next_id += 1;
            entry.id = self.next_id;
            self.entries.push(entry);
        }
    }

    /// Removes the frames with these ids.
    pub fn remove(&mut self, ids: &[u64]) {
        self.entries.retain(|e| !ids.contains(&e.id));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_round_trip_and_set_operations() {
        let img = RgbaImage::from_fn(30, 20, |x, y| {
            image::Rgba([x as u8 * 8, y as u8 * 12, 7, 200])
        });
        let e = FrameSetEntry::new("v/clip.mp4".into(), "clip.mp4".into(), 12, 0.4, &img);
        assert_eq!(e.image(), img);
        assert_eq!(e.thumbnail.height(), THUMBNAIL_HEIGHT);
        assert_eq!(e.document_name(), "clip frame 12");
        let mut set = FrameSet::default();
        set.extend([e.clone(), e.clone(), e]);
        let ids: Vec<u64> = set.entries().iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        set.remove(&[2]);
        assert_eq!(set.len(), 2);
        set.clear();
        assert!(set.is_empty());
        let img2 = RgbaImage::new(4, 4);
        set.extend([FrameSetEntry::new(
            "a.mkv".into(),
            "a.mkv".into(),
            0,
            0.0,
            &img2,
        )]);
        assert_eq!(set.entries()[0].id, 4, "ids are never reused");
    }
}
