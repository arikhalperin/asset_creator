//! Prompts and the background segmentation worker that picks MobileSAM when
//! it is available and GrabCut otherwise.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use image::{GrayImage, RgbaImage};

use super::models::{self, Source};
use super::sam::{self, Embedding, SamModel};

/// What the user pointed at, in image coordinates.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Prompt {
    /// (x, y, include): include points (object) and exclude points.
    pub points: Vec<(f32, f32, bool)>,
    /// A box (two corners).
    pub rect: Option<((f32, f32), (f32, f32))>,
}

impl Prompt {
    pub fn click(p: (f32, f32)) -> Self {
        Self {
            points: vec![(p.0, p.1, true)],
            rect: None,
        }
    }

    pub fn boxed(a: (f32, f32), b: (f32, f32)) -> Self {
        Self {
            points: Vec::new(),
            rect: Some((a, b)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty() && self.rect.is_none()
    }

    /// Pixels the object must contain: include points, or the box center.
    pub fn seeds(&self, w: u32, h: u32) -> Vec<(u32, u32)> {
        let clamp = |x: f32, y: f32| {
            (
                (x.max(0.0) as u32).min(w.saturating_sub(1)),
                (y.max(0.0) as u32).min(h.saturating_sub(1)),
            )
        };
        let mut seeds: Vec<(u32, u32)> = self
            .points
            .iter()
            .filter(|p| p.2)
            .map(|&(x, y, _)| clamp(x, y))
            .collect();
        if seeds.is_empty() {
            if let Some((a, b)) = self.rect {
                seeds.push(clamp((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0));
            }
        }
        seeds
    }
}

/// Which engine produced a mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Sam,
    GrabCut,
}

/// Whether MobileSAM can be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// Runtime and models are loaded.
    Ready,
    /// The runtime works but the models are missing (or damaged: the reason).
    NeedsModels(Option<String>),
    /// No usable runtime (the reason).
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// ONNX Runtime library candidates (empty: no runtime).
    pub runtime_paths: Vec<PathBuf>,
    pub model_dir: PathBuf,
    pub source: Source,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            runtime_paths: sam::default_runtime_paths(),
            model_dir: models::default_dir(),
            source: Source::default(),
        }
    }
}

/// A segmentation job.
#[derive(Clone)]
pub struct Request {
    pub id: u64,
    /// Identifies the image content (cache key for the SAM embedding).
    pub key: u64,
    pub image: Arc<RgbaImage>,
    pub prompt: Prompt,
}

#[derive(Debug)]
pub enum Event {
    Availability(Availability),
    /// Encoding a new image with MobileSAM.
    Analyzing(u64),
    Mask {
        id: u64,
        mask: GrayImage,
        engine: Engine,
        /// Why GrabCut was used, when it was.
        note: Option<String>,
    },
}

#[derive(Default)]
struct Slot {
    request: Option<Request>,
    reprobe: bool,
    quit: bool,
}

type Shared = Arc<(Mutex<Slot>, Condvar)>;

/// The worker thread. Created on the first prompt, so nothing ML-related
/// loads before then.
pub struct Segmenter {
    shared: Shared,
    events: Receiver<Event>,
    handle: Option<JoinHandle<()>>,
}

impl Segmenter {
    pub fn new(config: Config, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        let shared: Shared = Arc::default();
        let (tx, events) = mpsc::channel();
        let s = shared.clone();
        let handle = std::thread::Builder::new()
            .name("segmenter".to_owned())
            .spawn(move || run(config, s, tx, notify))
            .expect("spawn segmenter");
        Self {
            shared,
            events,
            handle: Some(handle),
        }
    }

    /// Replaces any pending request.
    pub fn request(&self, request: Request) {
        let (lock, cvar) = &*self.shared;
        lock.lock().expect("segmenter lock").request = Some(request);
        cvar.notify_all();
    }

    /// Checks again for the runtime and models (after a download).
    pub fn reprobe(&self) {
        let (lock, cvar) = &*self.shared;
        lock.lock().expect("segmenter lock").reprobe = true;
        cvar.notify_all();
    }

    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

impl Drop for Segmenter {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.shared;
        if let Ok(mut slot) = lock.lock() {
            slot.quit = true;
        }
        cvar.notify_all();
        // Detach: an in-flight job finishes in the background.
        drop(self.handle.take());
    }
}

enum Sam {
    Unknown,
    Ready(Box<SamModel>),
    Missing(Availability),
}

fn probe(config: &Config) -> Sam {
    if config.runtime_paths.is_empty() {
        return Sam::Missing(Availability::Unavailable(
            "ONNX Runtime not found".to_owned(),
        ));
    }
    let env = match sam::runtime(&config.runtime_paths) {
        Ok(env) => env,
        Err(reason) => return Sam::Missing(Availability::Unavailable(reason)),
    };
    if !models::present(&config.model_dir) {
        return Sam::Missing(Availability::NeedsModels(None));
    }
    match SamModel::load(env, &config.model_dir) {
        Ok(model) => Sam::Ready(Box::new(model)),
        Err(reason) => Sam::Missing(Availability::NeedsModels(Some(reason))),
    }
}

fn availability(sam: &Sam) -> Availability {
    match sam {
        Sam::Ready(_) => Availability::Ready,
        Sam::Missing(a) => a.clone(),
        Sam::Unknown => Availability::Unavailable("not checked".to_owned()),
    }
}

fn run(config: Config, shared: Shared, tx: Sender<Event>, notify: Arc<dyn Fn() + Send + Sync>) {
    let send = |e: Event| {
        let _ = tx.send(e);
        notify();
    };
    let mut sam = Sam::Unknown;
    let mut cache: Option<(u64, Embedding)> = None;
    loop {
        let (request, reprobe) = {
            let (lock, cvar) = &*shared;
            let Ok(mut slot) = lock.lock() else { return };
            while !slot.quit && slot.request.is_none() && !slot.reprobe {
                slot = match cvar.wait(slot) {
                    Ok(s) => s,
                    Err(_) => return,
                };
            }
            if slot.quit {
                return;
            }
            (slot.request.take(), std::mem::take(&mut slot.reprobe))
        };
        if reprobe || matches!(sam, Sam::Unknown) {
            sam = probe(&config);
            cache = None;
            send(Event::Availability(availability(&sam)));
        }
        let Some(request) = request else { continue };
        let (w, h) = request.image.dimensions();
        if w == 0 || h == 0 || request.prompt.is_empty() {
            continue;
        }
        let mut note = None;
        if let Sam::Ready(model) = &mut sam {
            if cache.as_ref().is_none_or(|(k, _)| *k != request.key) {
                send(Event::Analyzing(request.id));
                cache = model.embed(&request.image).ok().map(|e| (request.key, e));
            }
            let result = match &cache {
                Some((_, embedding)) => model.decode(embedding, &request.prompt),
                None => Err("MobileSAM could not encode the image".to_owned()),
            };
            match result {
                Ok(mask) => {
                    send(Event::Mask {
                        id: request.id,
                        mask,
                        engine: Engine::Sam,
                        note: None,
                    });
                    continue;
                }
                Err(e) => note = Some(e),
            }
        }
        let mask = super::grabcut::segment(&request.image, &request.prompt);
        send(Event::Mask {
            id: request.id,
            mask,
            engine: Engine::GrabCut,
            note,
        });
    }
}
