//! `game-asset-creator --self-test [video]`: a headless check of the bundled
//! native libraries, run before any window is created.
//!
//! It decodes the first frame of the bundled sample video (or the given file)
//! through the app's video module and loads ONNX Runtime through the app's own
//! loader and search order. It reads nothing but the video and the libraries
//! and writes nothing (in particular, nothing in the user's data folders).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ffmpeg_next as ff;

use crate::extract::sam;
use crate::video::VideoSource;

/// The command-line flag that selects the self-test.
pub const FLAG: &str = "--self-test";

/// If `args` (without the program name) ask for the self-test, runs it and
/// returns the process exit code.
pub fn from_args(args: &[OsString]) -> Option<i32> {
    if args.first().map(|a| a.as_os_str()) != Some(FLAG.as_ref()) {
        return None;
    }
    let video = args.get(1).map(PathBuf::from);
    Some(match run(video.as_deref()) {
        Ok(()) => {
            println!("self-test: OK");
            0
        }
        Err(e) => {
            eprintln!("self-test: FAILED: {e}");
            1
        }
    })
}

/// The bundled sample video: `<exe>/../Resources/sample.mp4` inside the app
/// bundle.
pub fn bundled_sample() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.parent()?.join("Resources").join("sample.mp4");
    path.is_file().then_some(path)
}

/// Runs every check, printing what it finds.
pub fn run(video: Option<&Path>) -> Result<(), String> {
    let video = match video {
        Some(p) => p.to_path_buf(),
        None => bundled_sample().ok_or(
            "no video given and no bundled sample (Contents/Resources/sample.mp4) next to the executable",
        )?,
    };

    // Video: open (which validates by decoding frame 0) and decode frame 0.
    let mut source = VideoSource::open(&video)
        .map_err(|e| format!("could not open {}: {e}", video.display()))?;
    let frame = source.frame(0).map_err(|e| {
        format!(
            "could not decode the first frame of {}: {e}",
            video.display()
        )
    })?;
    let info = source.info();
    println!(
        "video: {} ({}, {} frames) first frame {}x{}",
        video.display(),
        info.codec,
        info.frame_count,
        frame.image.width(),
        frame.image.height()
    );
    if frame.image.width() == 0 || frame.image.height() == 0 {
        return Err("the decoded frame is empty".into());
    }

    // ffmpeg build: version, licence and configuration of the loaded libavcodec.
    let v = ff::codec::version();
    println!(
        "ffmpeg: libavcodec {}.{}.{} license={}",
        v >> 16,
        (v >> 8) & 0xff,
        v & 0xff,
        ff::codec::license()
    );
    println!("ffmpeg configuration: {}", ff::codec::configuration());

    // ONNX Runtime through the app's loader and search order.
    let paths = sam::default_runtime_paths();
    let candidate = paths
        .iter()
        .find(|p| !p.is_absolute() || p.exists())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(none found)".into());
    sam::runtime(&paths).map_err(|e| format!("ONNX Runtime: {e}"))?;
    println!("onnxruntime: loaded from {candidate}");
    println!("onnxruntime: {}", ort::info());
    Ok(())
}
