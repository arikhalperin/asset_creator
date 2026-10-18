//! Video decoding against the fixtures in tests/fixtures/video (regenerate
//! them with its generate.sh), plus the release scrubbing benchmark.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

use game_asset_creator::video::extract::{self, ExtractMode, ExtractSpec};
use game_asset_creator::video::{Rotation, VideoSource};
use image::RgbaImage;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/video")
        .join(name)
}

fn reference(clip: &str, n: u64) -> RgbaImage {
    let name = if clip == "lossless.mkv" {
        format!("lossless_f{n}.png")
    } else {
        format!("{}_f{n}.png", clip.replace('.', "_"))
    };
    image::open(fixture(&name)).unwrap().into_rgba8()
}

const CLIPS: [&str; 8] = [
    "bt709.mp4",
    "clip.mp4",
    "clip.webm",
    "clip.mkv",
    "clip.avi",
    "clip.mov",
    "rotated.mov",
    "lossless.mkv",
];
const REFERENCE_FRAMES: [u64; 4] = [0, 17, 45, 89];

#[test]
fn every_container_opens_with_its_info() {
    for clip in CLIPS {
        let source = VideoSource::open(&fixture(clip)).unwrap_or_else(|e| panic!("{clip}: {e}"));
        let info = source.info();
        assert_eq!(info.frame_count, 90, "{clip}");
        assert_eq!(info.fps(), 30.0, "{clip}");
        assert_eq!(info.name, clip);
        if clip == "rotated.mov" {
            // `-display_rotation 90` is counterclockwise: frames turn 270° clockwise.
            assert_eq!(info.rotation, Rotation::Cw270);
            assert_eq!((info.width, info.height), (120, 160));
        } else {
            assert_eq!((info.width, info.height), (160, 120), "{clip}");
        }
    }
}

#[test]
fn unreadable_files_are_errors() {
    assert!(VideoSource::open(&fixture("corrupt.mp4")).is_err());
    assert!(VideoSource::open(&fixture("missing.mp4")).is_err());
    assert!(VideoSource::open(&fixture("scenes.json")).is_err());
}

#[test]
fn exact_frames_match_ffmpeg_in_any_access_order() {
    for clip in CLIPS {
        let path = fixture(clip);
        // In order from a fresh open.
        let mut source = VideoSource::open(&path).unwrap();
        for n in REFERENCE_FRAMES {
            let frame = source.frame(n).unwrap();
            assert_eq!(frame.index, n);
            assert!(
                frame.image == reference(clip, n),
                "{clip} frame {n} (in order)"
            );
        }
        // Backwards, after previews, and by stepping.
        let mut source = VideoSource::open(&path).unwrap();
        for n in REFERENCE_FRAMES.into_iter().rev() {
            let _ = source.keyframe_at(n).unwrap();
            assert!(
                source.frame(n).unwrap().image == reference(clip, n),
                "{clip} frame {n} (after preview)"
            );
        }
        let mut source = VideoSource::open(&path).unwrap();
        let _ = source.frame(80).unwrap();
        let _ = source.frame(44).unwrap();
        assert!(
            source.frame(45).unwrap().image == reference(clip, 45),
            "{clip} step forward"
        );
        let _ = source.frame(18).unwrap();
        assert!(
            source.frame(17).unwrap().image == reference(clip, 17),
            "{clip} step back"
        );
        // Sequential decoding numbers frames 0, 1, 2, ….
        let mut source = VideoSource::open(&path).unwrap();
        for i in 0..=17 {
            let frame = source.next_frame().unwrap().unwrap();
            assert_eq!(frame.index, i);
            if i == 17 {
                assert!(frame.image == reference(clip, 17), "{clip} sequential");
            }
        }
    }
}

#[test]
fn keyframe_preview_is_at_or_before_the_target() {
    let mut source = VideoSource::open(&fixture("clip.mp4")).unwrap();
    // Keyframes every 30 frames.
    for (target, key) in [(0, 0), (17, 0), (45, 30), (89, 60)] {
        let frame = source.keyframe_at(target).unwrap();
        assert_eq!(frame.index, key, "target {target}");
    }
    assert_eq!(
        source.frame(10_000).unwrap().index,
        89,
        "past the end clamps"
    );
}

#[derive(serde::Deserialize)]
struct SceneFixture {
    frames: u64,
    cuts: Vec<u64>,
}

fn run(spec: ExtractSpec) -> Vec<game_asset_creator::frame_set::FrameSetEntry> {
    let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    extract::run(&spec, &AtomicU64::new(0), &AtomicBool::new(false), &notify).unwrap()
}

#[test]
fn scene_detection_finds_the_fixture_cuts() {
    let expected: SceneFixture =
        serde_json::from_str(&std::fs::read_to_string(fixture("scenes.json")).unwrap()).unwrap();
    let path = fixture("scenes.mp4");
    assert_eq!(
        VideoSource::open(&path).unwrap().info().frame_count,
        expected.frames
    );
    let frames = run(ExtractSpec {
        path,
        mode: ExtractMode::SceneChanges {
            threshold: 0.30,
            min_scene: 10,
        },
        from: 0,
        to: expected.frames - 1,
    });
    let found: Vec<u64> = frames.iter().map(|e| e.frame).collect();
    let mut wanted = vec![0];
    wanted.extend(&expected.cuts);
    assert_eq!(found, wanted);
}

#[test]
fn every_nth_extracts_exact_frames_in_range() {
    let frames = run(ExtractSpec {
        path: fixture("lossless.mkv"),
        mode: ExtractMode::EveryNth(28),
        from: 17,
        to: 89,
    });
    let numbers: Vec<u64> = frames.iter().map(|e| e.frame).collect();
    assert_eq!(numbers, vec![17, 45, 73]);
    assert!(frames[0].image() == reference("lossless.mkv", 17));
    assert!(frames[1].image() == reference("lossless.mkv", 45));
    assert_eq!(frames[1].timestamp, 1.5);
    assert_eq!(frames[1].document_name(), "lossless frame 45");
}

fn bench_clip() -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let path = dir.join("bench_1080p.mp4");
    if !path.exists() {
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=1920x1080:rate=30:duration=20",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-g",
                "60",
                "-preset",
                "medium",
            ])
            .arg(&path)
            .status()
            .ok()?;
        if !status.success() {
            return None;
        }
    }
    Some(path)
}

/// Release benchmark: `cargo test --release --test video -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_scrub_1080p() {
    let path = bench_clip().expect("ffmpeg CLI with libx264 to generate the clip");
    let mut source = VideoSource::open(&path).unwrap();
    let count = source.info().frame_count;
    assert_eq!((source.info().width, source.info().height), (1920, 1080));
    let time = |f: &mut dyn FnMut()| {
        let start = std::time::Instant::now();
        f();
        start.elapsed().as_secs_f64() * 1000.0
    };
    // Scrubbing previews at scattered positions.
    let positions: Vec<u64> = (0..40).map(|i| (i * 7919) % count).collect();
    let mut preview_worst = 0.0f64;
    for &p in &positions {
        preview_worst = preview_worst.max(time(&mut || {
            source.keyframe_at(p).unwrap();
        }));
    }
    // Exact frames after moving elsewhere, including the worst case just
    // before a keyframe (59 frames to decode).
    let mut exact_worst = 0.0f64;
    for &p in positions.iter().chain(&[59, 119, 599, count - 2]) {
        source.keyframe_at((p + 300) % count).unwrap();
        exact_worst = exact_worst.max(time(&mut || {
            source.frame(p).unwrap();
        }));
    }
    // Sequential decoding.
    source.frame(0).unwrap();
    let n = 300;
    let sequential = time(&mut || {
        for _ in 0..n {
            source.next_frame().unwrap().unwrap();
        }
    }) / n as f64;
    println!(
        "preview worst {preview_worst:.1} ms, exact worst {exact_worst:.1} ms, sequential {sequential:.2} ms/frame"
    );
    if !cfg!(debug_assertions) {
        assert!(preview_worst < 60.0, "preview {preview_worst:.1} ms");
        assert!(exact_worst < 500.0, "exact {exact_worst:.1} ms");
        assert!(sequential < 33.0, "sequential {sequential:.2} ms");
    }
}
