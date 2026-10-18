//! The background decoding worker behind a video tab: scrub previews, exact
//! frames, and real-time playback. The newest request replaces pending ones.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{Frame, VideoSource};

/// Called after the worker sends something (e.g. to request a repaint).
pub type Notify = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Nearest keyframe at or before the frame (while scrubbing).
    Preview(u64),
    /// Exactly this frame.
    Exact(u64),
    /// Play from this frame at the clip's frame rate.
    Play(u64),
}

#[derive(Debug)]
pub enum Event {
    Frame {
        frame: Frame,
        /// Whether this is the exact frame (not a keyframe preview).
        exact: bool,
        /// Sent during playback.
        playing: bool,
        /// The request it answers (playback frames answer `Play`).
        request: Request,
    },
    /// Playback reached the last frame (its number).
    Ended(u64),
    /// The stream turned out shorter than estimated.
    FrameCount(u64),
    Error(String),
}

#[derive(Default)]
struct Slot {
    request: Option<Request>,
    quit: bool,
}

type Shared = Arc<(Mutex<Slot>, Condvar)>;

pub struct Player {
    shared: Shared,
    events: Receiver<Event>,
    handle: Option<JoinHandle<()>>,
}

impl Player {
    /// Starts the worker on `source`.
    pub fn new(source: VideoSource, notify: Notify) -> Self {
        let shared: Shared = Arc::default();
        let (tx, events) = mpsc::channel();
        let worker_shared = shared.clone();
        let handle = std::thread::Builder::new()
            .name("video-player".to_owned())
            .spawn(move || run(source, worker_shared, tx, notify))
            .expect("spawn video worker");
        Self {
            shared,
            events,
            handle: Some(handle),
        }
    }

    /// Replaces any pending request (and stops playback).
    pub fn request(&self, request: Request) {
        let (lock, cvar) = &*self.shared;
        lock.lock().expect("player lock").request = Some(request);
        cvar.notify_all();
    }

    /// Events received since the last call.
    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    /// Waits up to `timeout` for the next event.
    pub fn wait(&self, timeout: Duration) -> Option<Event> {
        self.events.recv_timeout(timeout).ok()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let (lock, cvar) = &*self.shared;
        if let Ok(mut slot) = lock.lock() {
            slot.quit = true;
        }
        cvar.notify_all();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Takes the pending request, waiting up to `timeout` (forever with `None`).
/// Returns `Err(())` when the player is shutting down.
fn take(shared: &Shared, timeout: Option<Duration>) -> Result<Option<Request>, ()> {
    let (lock, cvar) = &**shared;
    let mut slot = lock.lock().map_err(|_| ())?;
    let deadline = timeout.map(|t| Instant::now() + t);
    loop {
        if slot.quit {
            return Err(());
        }
        if let Some(request) = slot.request.take() {
            return Ok(Some(request));
        }
        match deadline {
            None => slot = cvar.wait(slot).map_err(|_| ())?,
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    return Ok(None);
                }
                slot = cvar.wait_timeout(slot, deadline - now).map_err(|_| ())?.0;
            }
        }
    }
}

fn run(mut source: VideoSource, shared: Shared, tx: Sender<Event>, notify: Notify) {
    let send = |event: Event| {
        let ok = tx.send(event).is_ok();
        notify();
        ok
    };
    let mut known_count = source.info().frame_count;
    let mut next: Option<Request> = None;
    loop {
        let request = match next.take() {
            Some(r) => r,
            None => match take(&shared, None) {
                Ok(Some(r)) => r,
                Ok(None) => continue,
                Err(()) => return,
            },
        };
        let result = match request {
            Request::Preview(i) => source.keyframe_at(i).map(|frame| {
                send(Event::Frame {
                    frame,
                    exact: false,
                    playing: false,
                    request,
                });
            }),
            Request::Exact(i) => source.frame(i).map(|frame| {
                send(Event::Frame {
                    frame,
                    exact: true,
                    playing: false,
                    request,
                });
            }),
            Request::Play(from) => match play(&mut source, &shared, &send, from) {
                Ok(PlayEnd::Interrupted(r)) => {
                    next = Some(r);
                    Ok(())
                }
                Ok(PlayEnd::Finished) => Ok(()),
                Ok(PlayEnd::Quit) => return,
                Err(e) => Err(e),
            },
        };
        if let Err(e) = result {
            send(Event::Error(e.to_string()));
        }
        if source.info().frame_count != known_count {
            known_count = source.info().frame_count;
            send(Event::FrameCount(known_count));
        }
    }
}

enum PlayEnd {
    Finished,
    Interrupted(Request),
    Quit,
}

fn play(
    source: &mut VideoSource,
    shared: &Shared,
    send: &dyn Fn(Event) -> bool,
    from: u64,
) -> Result<PlayEnd, super::VideoError> {
    let fps = source.info().fps();
    let first = source.frame(from)?;
    let mut last = first.index;
    let start = Instant::now();
    send(Event::Frame {
        frame: first,
        exact: true,
        playing: true,
        request: Request::Play(from),
    });
    loop {
        // Any request (pause, seek) ends playback.
        match take(shared, Some(Duration::ZERO)) {
            Ok(Some(r)) => return Ok(PlayEnd::Interrupted(r)),
            Ok(None) => {}
            Err(()) => return Ok(PlayEnd::Quit),
        }
        let Some((index, raw)) = source.next_raw()? else {
            send(Event::Ended(last));
            return Ok(PlayEnd::Finished);
        };
        last = index;
        let due = start + Duration::from_secs_f64((index - from) as f64 / fps);
        let now = Instant::now();
        if now > due + Duration::from_secs_f64(1.0 / fps) {
            // Behind schedule: skip this frame.
            continue;
        }
        if due > now {
            match take(shared, Some(due - now)) {
                Ok(Some(r)) => return Ok(PlayEnd::Interrupted(r)),
                Ok(None) => {}
                Err(()) => return Ok(PlayEnd::Quit),
            }
        }
        let image = source.convert(&raw)?;
        send(Event::Frame {
            frame: Frame { index, image },
            exact: true,
            playing: true,
            request: Request::Play(from),
        });
    }
}

/// Decodes evenly spaced thumbnails in the background.
pub struct Thumbnails {
    cancel: Arc<std::sync::atomic::AtomicBool>,
    events: Receiver<(usize, Frame)>,
    handle: Option<JoinHandle<()>>,
}

impl Thumbnails {
    /// Starts decoding keyframe thumbnails near `targets`, scaled to fit
    /// `height` pixels tall. Results arrive as (slot, frame).
    pub fn new(path: std::path::PathBuf, targets: Vec<u64>, height: u32, notify: Notify) -> Self {
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (tx, events) = mpsc::channel();
        let flag = cancel.clone();
        let handle = std::thread::Builder::new()
            .name("video-thumbnails".to_owned())
            .spawn(move || {
                let Ok(mut source) = VideoSource::open(&path) else {
                    return;
                };
                for (slot, target) in targets.into_iter().enumerate() {
                    if flag.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    let Ok(frame) = source.keyframe_at(target) else {
                        continue;
                    };
                    let (w, h) = frame.image.dimensions();
                    let tw = ((w as f32 * height as f32 / h as f32).round() as u32).max(1);
                    let image = image::imageops::thumbnail(&frame.image, tw, height);
                    if tx
                        .send((
                            slot,
                            Frame {
                                index: frame.index,
                                image,
                            },
                        ))
                        .is_err()
                    {
                        return;
                    }
                    notify();
                }
            })
            .expect("spawn thumbnail worker");
        Self {
            cancel,
            events,
            handle: Some(handle),
        }
    }

    pub fn poll(&self) -> Vec<(usize, Frame)> {
        self.events.try_iter().collect()
    }
}

impl Drop for Thumbnails {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        // Detach: the worker stops after its current thumbnail.
        drop(self.handle.take());
    }
}
