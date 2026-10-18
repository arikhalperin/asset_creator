//! Batch frame extraction (every Nth frame, or scene changes) in the
//! background, producing Frame Set entries.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::JoinHandle;

use super::player::Notify;
use super::scene::SceneDetector;
use super::{VideoError, VideoSource};
use crate::frame_set::FrameSetEntry;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExtractMode {
    EveryNth(u64),
    /// Threshold 0…1 and minimum scene length in frames.
    SceneChanges {
        threshold: f32,
        min_scene: u64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractSpec {
    pub path: PathBuf,
    pub mode: ExtractMode,
    /// Inclusive frame range.
    pub from: u64,
    pub to: u64,
}

impl ExtractSpec {
    /// Frames Every-Nth selects (From, From+N, … ≤ To).
    pub fn nth_targets(from: u64, to: u64, n: u64) -> Vec<u64> {
        (from..=to).step_by(n.max(1) as usize).collect()
    }
}

/// A running extraction job.
pub struct ExtractJob {
    progress: Arc<AtomicU64>,
    total: u64,
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<Vec<FrameSetEntry>, String>>,
    handle: Option<JoinHandle<()>>,
}

impl ExtractJob {
    pub fn start(spec: ExtractSpec, notify: Notify) -> Self {
        let total = match spec.mode {
            ExtractMode::EveryNth(n) => {
                ExtractSpec::nth_targets(spec.from, spec.to, n).len() as u64
            }
            ExtractMode::SceneChanges { .. } => spec.to.saturating_sub(spec.from) + 1,
        };
        let progress = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, result) = mpsc::channel();
        let (p, c) = (progress.clone(), cancel.clone());
        let handle = std::thread::Builder::new()
            .name("video-extract".to_owned())
            .spawn(move || {
                let outcome = run(&spec, &p, &c, &notify).map_err(|e| e.to_string());
                if !c.load(Ordering::Relaxed) {
                    let _ = tx.send(outcome);
                }
                notify();
            })
            .expect("spawn extraction worker");
        Self {
            progress,
            total,
            cancel,
            result,
            handle: Some(handle),
        }
    }

    /// Fraction done, 0…1.
    pub fn progress(&self) -> f32 {
        if self.total == 0 {
            return 1.0;
        }
        (self.progress.load(Ordering::Relaxed) as f32 / self.total as f32).min(1.0)
    }

    /// The result once finished (frames in order, or an error message).
    pub fn try_result(&self) -> Option<Result<Vec<FrameSetEntry>, String>> {
        self.result.try_recv().ok()
    }

    /// Stops the job; nothing is delivered.
    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ExtractJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        drop(self.handle.take());
    }
}

/// Runs an extraction synchronously (the job thread's body; also used by tests).
pub fn run(
    spec: &ExtractSpec,
    progress: &AtomicU64,
    cancel: &AtomicBool,
    notify: &Notify,
) -> Result<Vec<FrameSetEntry>, VideoError> {
    let mut source = VideoSource::open(&spec.path)?;
    let info = source.info().clone();
    let to = spec.to.min(info.last_frame());
    let from = spec.from.min(to);
    let mut out = Vec::new();
    let entry = |index: u64, image: &image::RgbaImage| {
        FrameSetEntry::new(
            info.path.clone(),
            info.name.clone(),
            index,
            info.timestamp(index),
            image,
        )
    };
    match spec.mode {
        ExtractMode::EveryNth(n) => {
            for (done, target) in ExtractSpec::nth_targets(from, to, n)
                .into_iter()
                .enumerate()
            {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(Vec::new());
                }
                let frame = source.frame(target)?;
                if frame.index == target {
                    out.push(entry(target, &frame.image));
                }
                progress.store(done as u64 + 1, Ordering::Relaxed);
                notify();
            }
        }
        ExtractMode::SceneChanges {
            threshold,
            min_scene,
        } => {
            let mut detector = SceneDetector::new(threshold, min_scene);
            let first = source.frame(from)?;
            let mut current = Some(first);
            while let Some(frame) = current {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(Vec::new());
                }
                if frame.index > to {
                    break;
                }
                if detector.push(frame.index, &frame.image) {
                    out.push(entry(frame.index, &frame.image));
                }
                progress.store(frame.index - from + 1, Ordering::Relaxed);
                if frame.index % 8 == 0 {
                    notify();
                }
                current = source.next_frame()?;
            }
        }
    }
    Ok(out)
}
