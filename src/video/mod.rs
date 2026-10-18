//! Video decoding through the linked ffmpeg libraries: opening, exact and
//! keyframe seeking, sequential decoding, and conversion to RGBA.

pub mod extract;
pub mod player;
pub mod scene;

use std::path::{Path, PathBuf};
use std::sync::Once;

use ffmpeg_next as ff;
use image::RgbaImage;

/// File extensions opened as video.
pub const EXTENSIONS: [&str; 5] = ["mp4", "mkv", "avi", "webm", "mov"];

/// Whether `path` has one of the video extensions.
pub fn is_video_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|v| v.eq_ignore_ascii_case(e)))
}

#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("{0}")]
    Ffmpeg(#[from] ff::Error),
    #[error("the file has no video stream")]
    NoVideoStream,
    #[error("no frames could be decoded")]
    NoFrames,
}

/// Clockwise rotation applied to decoded frames so they display upright.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    None,
    Cw90,
    Cw180,
    Cw270,
}

impl Rotation {
    /// From a display matrix's counterclockwise angle in degrees, the way
    /// the ffmpeg CLI's autorotation reads it.
    pub fn from_display_degrees(ccw: f64) -> Self {
        let theta = -ccw.round();
        let theta = theta - 360.0 * (theta / 360.0 + 0.9 / 360.0).floor();
        match theta.round() as i64 {
            90 => Rotation::Cw90,
            180 => Rotation::Cw180,
            270 => Rotation::Cw270,
            _ => Rotation::None,
        }
    }

    fn swaps_sides(self) -> bool {
        matches!(self, Rotation::Cw90 | Rotation::Cw270)
    }

    /// The filters the ffmpeg CLI inserts for this rotation.
    fn filters(self) -> Option<&'static str> {
        match self {
            Rotation::None => None,
            Rotation::Cw90 => Some("transpose=clock"),
            Rotation::Cw180 => Some("hflip,vflip"),
            Rotation::Cw270 => Some("transpose=cclock"),
        }
    }
}

/// Rotates decoded frames before color conversion with the same filters as
/// the ffmpeg CLI, so chroma is resampled identically.
struct Rotator {
    graph: ff::filter::Graph,
    key: (ff::format::Pixel, u32, u32),
}

// SAFETY: like `Scaler`, used only by the owning thread.
unsafe impl Send for Rotator {}

impl Rotator {
    fn new(frame: &ff::frame::Video, filters: &str) -> Result<Self, ff::Error> {
        let mut graph = ff::filter::Graph::new();
        let pix_fmt: ff::ffi::AVPixelFormat = frame.format().into();
        let args = format!(
            "video_size={}x{}:pix_fmt={}:time_base=1/30:pixel_aspect=1/1",
            frame.width(),
            frame.height(),
            pix_fmt as i32
        );
        let buffer = ff::filter::find("buffer").ok_or(ff::Error::FilterNotFound)?;
        let sink = ff::filter::find("buffersink").ok_or(ff::Error::FilterNotFound)?;
        graph.add(&buffer, "in", &args)?;
        graph.add(&sink, "out", "")?;
        graph.output("in", 0)?.input("out", 0)?.parse(filters)?;
        graph.validate()?;
        Ok(Self {
            graph,
            key: (frame.format(), frame.width(), frame.height()),
        })
    }

    fn rotate(&mut self, frame: &ff::frame::Video) -> Result<ff::frame::Video, ff::Error> {
        self.graph
            .get("in")
            .ok_or(ff::Error::FilterNotFound)?
            .source()
            .add(frame)?;
        let mut out = ff::frame::Video::empty();
        self.graph
            .get("out")
            .ok_or(ff::Error::FilterNotFound)?
            .sink()
            .frame(&mut out)?;
        Ok(out)
    }
}

/// What the UI and the extraction jobs know about a video.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    pub path: PathBuf,
    /// File name.
    pub name: String,
    /// Display size (after rotation).
    pub width: u32,
    pub height: u32,
    /// Average frame rate as a fraction.
    pub frame_rate: (i32, i32),
    pub frame_count: u64,
    pub rotation: Rotation,
    pub codec: String,
}

impl VideoInfo {
    pub fn fps(&self) -> f64 {
        self.frame_rate.0 as f64 / self.frame_rate.1 as f64
    }

    /// Time at which frame `index` is shown, in seconds.
    pub fn timestamp(&self, index: u64) -> f64 {
        index as f64 / self.fps()
    }

    pub fn duration(&self) -> f64 {
        self.timestamp(self.frame_count)
    }

    pub fn last_frame(&self) -> u64 {
        self.frame_count.saturating_sub(1)
    }

    /// The file name without its extension.
    pub fn stem(&self) -> String {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.name.clone())
    }
}

/// `hh:mm:ss.mmm`.
pub fn format_timecode(seconds: f64) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// A decoded frame and its number.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub index: u64,
    pub image: RgbaImage,
}

fn init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = ff::init();
        ff::util::log::set_level(ff::util::log::Level::Fatal);
    });
}

/// Where the decoder is in the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cursor {
    /// Freshly opened: the next frame is frame 0.
    Start,
    /// The last frame decoded was this one.
    After(u64),
    /// Just after a seek, before any frame is decoded.
    Unknown,
}

/// RGBA conversion matching `ffmpeg -pix_fmt rgba` (the frame's color matrix
/// and range are honoured).
struct Scaler {
    context: ff::software::scaling::Context,
    key: (ff::format::Pixel, u32, u32, i32, bool),
}

// SAFETY: the scaling context is only used by the thread that owns the
// `VideoSource`; it holds no thread-affine state.
unsafe impl Send for Scaler {}

/// A video file opened for decoding. Not shared between threads: each worker
/// opens its own.
pub struct VideoSource {
    input: ff::format::context::Input,
    stream: usize,
    decoder: ff::decoder::Video,
    scaler: Option<Scaler>,
    rotator: Option<Rotator>,
    info: VideoInfo,
    /// Seconds per stream time-base unit.
    time_base: f64,
    start_pts: i64,
    cursor: Cursor,
    eof: bool,
}

/// Frames decoded forward instead of seeking when the target is at most this
/// far ahead.
const FORWARD_LIMIT: u64 = 60;

impl VideoSource {
    /// Opens `path` and checks that its first frame decodes.
    pub fn open(path: &Path) -> Result<Self, VideoError> {
        init();
        let input = ff::format::input(&path)?;
        let stream = input
            .streams()
            .best(ff::media::Type::Video)
            .ok_or(VideoError::NoVideoStream)?;
        let index = stream.index();
        let mut context = ff::codec::context::Context::from_parameters(stream.parameters())?;
        context.set_threading(ff::codec::threading::Config {
            kind: ff::codec::threading::Type::Frame,
            count: 0,
        });
        let decoder = context.decoder().video()?;
        let tb = stream.time_base();
        let time_base = tb.numerator() as f64 / tb.denominator() as f64;
        let mut rate = stream.avg_frame_rate();
        if rate.numerator() <= 0 || rate.denominator() <= 0 {
            rate = stream.rate();
        }
        if rate.numerator() <= 0 || rate.denominator() <= 0 {
            rate = ff::Rational::new(25, 1);
        }
        let fps = rate.numerator() as f64 / rate.denominator() as f64;
        let start_pts = match stream.start_time() {
            t if t == ff::ffi::AV_NOPTS_VALUE => 0,
            t => t,
        };
        let frame_count = if stream.frames() > 0 {
            stream.frames() as u64
        } else {
            let seconds = if stream.duration() > 0 {
                stream.duration() as f64 * time_base
            } else {
                input.duration().max(0) as f64 / ff::ffi::AV_TIME_BASE as f64
            };
            (seconds * fps).round().max(1.0) as u64
        };
        let rotation = Rotation::from_display_degrees(display_rotation(&stream));
        let codec = decoder
            .codec()
            .map(|c| c.name().to_owned())
            .unwrap_or_default();
        let (w, h) = (decoder.width(), decoder.height());
        let (width, height) = if rotation.swaps_sides() {
            (h, w)
        } else {
            (w, h)
        };
        let info = VideoInfo {
            path: path.to_path_buf(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            width,
            height,
            frame_rate: (rate.numerator(), rate.denominator()),
            frame_count,
            rotation,
            codec,
        };
        let mut source = Self {
            input,
            stream: index,
            decoder,
            scaler: None,
            rotator: None,
            info,
            time_base,
            start_pts,
            cursor: Cursor::Start,
            eof: false,
        };
        // Validate by decoding the first frame, then rewind.
        if source.decode_next()?.is_none() {
            return Err(VideoError::NoFrames);
        }
        source.rewind()?;
        Ok(source)
    }

    pub fn info(&self) -> &VideoInfo {
        &self.info
    }

    fn rewind(&mut self) -> Result<(), VideoError> {
        self.seek_to(0)?;
        self.cursor = Cursor::Start;
        Ok(())
    }

    /// Seeks to the keyframe at or before frame `index`.
    fn seek_to(&mut self, index: u64) -> Result<(), VideoError> {
        let seconds = self.start_pts as f64 * self.time_base + self.info.timestamp(index);
        let ts = (seconds * ff::ffi::AV_TIME_BASE as f64).round() as i64;
        // Keyframe at or before `ts` (the range end is inclusive in ffmpeg).
        self.input.seek(ts, i64::MIN..ts)?;
        self.decoder.flush();
        self.cursor = Cursor::Unknown;
        self.eof = false;
        Ok(())
    }

    fn index_of(&self, frame: &ff::frame::Video) -> u64 {
        match frame.timestamp().or_else(|| frame.pts()) {
            Some(pts) => {
                let seconds = (pts - self.start_pts) as f64 * self.time_base;
                (seconds * self.info.fps()).round().max(0.0) as u64
            }
            None => match self.cursor {
                Cursor::After(i) => i + 1,
                _ => 0,
            },
        }
    }

    /// Decodes the next frame in presentation order (`None` at the end).
    fn decode_next(&mut self) -> Result<Option<(u64, ff::frame::Video)>, VideoError> {
        loop {
            let mut frame = ff::frame::Video::empty();
            if self.decoder.receive_frame(&mut frame).is_ok() {
                let mut index = self.index_of(&frame);
                if let Cursor::After(last) = self.cursor {
                    // Keep numbers increasing even with odd timestamps.
                    index = index.max(last + 1);
                }
                self.cursor = Cursor::After(index);
                return Ok(Some((index, frame)));
            }
            if self.eof {
                return Ok(None);
            }
            match self.input.packets().next() {
                Some((stream, packet)) => {
                    if stream.index() == self.stream {
                        // Damaged packets are skipped, as the CLI does.
                        let _ = self.decoder.send_packet(&packet);
                    }
                }
                None => {
                    let _ = self.decoder.send_eof();
                    self.eof = true;
                }
            }
        }
    }

    /// Converts a decoded frame to upright RGBA.
    fn rgba_of(&mut self, frame: &ff::frame::Video) -> Result<RgbaImage, VideoError> {
        let rotated;
        let frame = match self.info.rotation.filters() {
            None => frame,
            Some(filters) => {
                let key = (frame.format(), frame.width(), frame.height());
                if self.rotator.as_ref().is_none_or(|r| r.key != key) {
                    self.rotator = Some(Rotator::new(frame, filters)?);
                }
                rotated = self
                    .rotator
                    .as_mut()
                    .expect("rotator just set")
                    .rotate(frame)?;
                &rotated
            }
        };
        // SAFETY: reading plain fields of a valid decoded frame.
        let (colorspace, full_range) = unsafe {
            let raw = frame.as_ptr();
            (
                (*raw).colorspace as i32,
                (*raw).color_range == ff::ffi::AVColorRange::AVCOL_RANGE_JPEG,
            )
        };
        let key = (
            frame.format(),
            frame.width(),
            frame.height(),
            colorspace,
            full_range,
        );
        if self.scaler.as_ref().is_none_or(|s| s.key != key) {
            let mut context = ff::software::scaling::Context::get(
                frame.format(),
                frame.width(),
                frame.height(),
                ff::format::Pixel::RGBA,
                frame.width(),
                frame.height(),
                ff::software::scaling::Flags::BICUBIC,
            )?;
            // SAFETY: valid context; coefficient tables are static.
            unsafe {
                use ff::ffi::*;
                // Unspecified (2) and RGB (0) fall back to the default matrix.
                let matrix = if colorspace == 0 || colorspace == 2 {
                    SWS_CS_DEFAULT
                } else {
                    colorspace
                };
                sws_setColorspaceDetails(
                    context.as_mut_ptr(),
                    sws_getCoefficients(matrix),
                    full_range as i32,
                    sws_getCoefficients(SWS_CS_DEFAULT),
                    1,
                    0,
                    1 << 16,
                    1 << 16,
                );
            }
            self.scaler = Some(Scaler { context, key });
        }
        let scaler = self.scaler.as_mut().expect("scaler just set");
        let mut rgba = ff::frame::Video::empty();
        scaler.context.run(frame, &mut rgba)?;
        let (w, h) = (rgba.width() as usize, rgba.height() as usize);
        let stride = rgba.stride(0);
        let data = rgba.data(0);
        let mut pixels = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            pixels.extend_from_slice(&data[y * stride..y * stride + w * 4]);
        }
        Ok(RgbaImage::from_raw(w as u32, h as u32, pixels).expect("sized buffer"))
    }

    /// The nearest keyframe at or before frame `index` (fast; for scrubbing
    /// previews and thumbnails).
    pub fn keyframe_at(&mut self, index: u64) -> Result<Frame, VideoError> {
        self.seek_to(index.min(self.info.last_frame()))?;
        match self.decode_next()? {
            Some((i, frame)) => Ok(Frame {
                index: i,
                image: self.rgba_of(&frame)?,
            }),
            None => self.frame(index),
        }
    }

    /// Exactly frame `index` (the frame shown at its timestamp); past the end,
    /// the last frame.
    pub fn frame(&mut self, index: u64) -> Result<Frame, VideoError> {
        let forward = match self.cursor {
            Cursor::Start => index <= FORWARD_LIMIT,
            Cursor::After(last) => last < index && index - last <= FORWARD_LIMIT && !self.eof,
            Cursor::Unknown => false,
        };
        if !forward {
            self.seek_to(index)?;
        }
        let mut back_off = 1.0;
        let mut fresh_seek = !forward;
        let mut previous: Option<(u64, ff::frame::Video)> = None;
        loop {
            match self.decode_next()? {
                Some((i, frame)) if i == index => {
                    return Ok(Frame {
                        index,
                        image: self.rgba_of(&frame)?,
                    });
                }
                Some((i, frame)) if i > index => {
                    if let Some((_, prev)) = previous {
                        // A gap in timestamps: the earlier frame is still showing.
                        return Ok(Frame {
                            index,
                            image: self.rgba_of(&prev)?,
                        });
                    }
                    if !fresh_seek || index == 0 || back_off > 64.0 {
                        return Ok(Frame {
                            index: i,
                            image: self.rgba_of(&frame)?,
                        });
                    }
                    // The seek landed past the target: try from further back.
                    let back = (self.info.fps() * back_off).ceil() as u64;
                    self.seek_to(index.saturating_sub(back))?;
                    back_off *= 2.0;
                    fresh_seek = true;
                }
                Some((i, frame)) => {
                    fresh_seek = false;
                    previous = Some((i, frame));
                }
                None => {
                    return match previous {
                        Some((i, frame)) => {
                            self.note_last_frame(i);
                            Ok(Frame {
                                index: i,
                                image: self.rgba_of(&frame)?,
                            })
                        }
                        None if index > 0 => {
                            // At the end with nothing buffered: step back a little.
                            self.seek_to(index.saturating_sub(self.info.fps().ceil() as u64))?;
                            let last = self.info.last_frame().min(index - 1);
                            self.note_last_frame(last);
                            self.frame(last)
                        }
                        None => Err(VideoError::NoFrames),
                    };
                }
            }
        }
    }

    /// The frame count was an estimate: the stream really ends at `last`.
    fn note_last_frame(&mut self, last: u64) {
        if last + 1 < self.info.frame_count {
            self.info.frame_count = last + 1;
        }
    }

    /// The next frame after the last one decoded (frame 0 after opening).
    pub fn next_frame(&mut self) -> Result<Option<Frame>, VideoError> {
        match self.next_raw()? {
            Some((index, frame)) => Ok(Some(Frame {
                index,
                image: self.rgba_of(&frame.0)?,
            })),
            None => Ok(None),
        }
    }

    /// Like [`Self::next_frame`] but leaves conversion to the caller (so
    /// frames can be dropped cheaply during playback).
    pub fn next_raw(&mut self) -> Result<Option<(u64, RawFrame)>, VideoError> {
        match self.decode_next()? {
            Some((index, frame)) => Ok(Some((index, RawFrame(frame)))),
            None => {
                if let Cursor::After(last) = self.cursor {
                    self.note_last_frame(last);
                }
                Ok(None)
            }
        }
    }

    pub fn convert(&mut self, frame: &RawFrame) -> Result<RgbaImage, VideoError> {
        self.rgba_of(&frame.0)
    }
}

/// A decoded frame not yet converted to RGBA.
pub struct RawFrame(ff::frame::Video);

/// Counterclockwise display rotation in degrees from the stream's display
/// matrix (0 without one).
fn display_rotation(stream: &ff::format::stream::Stream) -> f64 {
    // SAFETY: reads the codec parameters' side data of a live stream.
    unsafe {
        use ff::ffi::*;
        let par = (*stream.as_ptr()).codecpar;
        if par.is_null() {
            return 0.0;
        }
        let side = av_packet_side_data_get(
            (*par).coded_side_data,
            (*par).nb_coded_side_data,
            AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX,
        );
        if side.is_null() || (*side).size < 36 {
            return 0.0;
        }
        let angle = av_display_rotation_get((*side).data as *const i32);
        if angle.is_nan() {
            0.0
        } else {
            angle
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_follows_cli_autorotate() {
        assert_eq!(Rotation::from_display_degrees(0.0), Rotation::None);
        assert_eq!(Rotation::from_display_degrees(-90.0), Rotation::Cw90);
        assert_eq!(Rotation::from_display_degrees(90.0), Rotation::Cw270);
        assert_eq!(Rotation::from_display_degrees(180.0), Rotation::Cw180);
        assert_eq!(Rotation::from_display_degrees(-180.0), Rotation::Cw180);
    }

    #[test]
    fn timecodes_and_extensions() {
        assert_eq!(format_timecode(0.0), "00:00:00.000");
        assert_eq!(format_timecode(3723.4567), "01:02:03.457");
        assert!(is_video_path(Path::new("a/b.MP4")));
        assert!(is_video_path(Path::new("x.webm")));
        assert!(!is_video_path(Path::new("x.png")));
    }
}
