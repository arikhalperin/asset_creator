//! Video tabs, Grab Frame, Extract Frames, and the Frame Set panel through
//! the app (fixtures in tests/fixtures/video).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Key, Pos2};
use game_asset_creator::ui::Panel;
use game_asset_creator::video::VideoSource;
use game_asset_creator::video_ui::{Displayed, ExtractChoice, FrameSetAction, VideoId};
use game_asset_creator::DocId;
use image::RgbaImage;

mod common;
use common::Harness;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/video")
        .join(name)
}

fn reference(name: &str) -> RgbaImage {
    image::open(fixture(name)).unwrap().into_rgba8()
}

/// Runs frames until `done` holds (fails after 10 s).
fn wait_until(h: &mut Harness, what: &str, mut done: impl FnMut(&Harness) -> bool) {
    let start = Instant::now();
    while !done(h) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(2));
        h.frame(vec![]);
    }
}

fn shows(h: &Harness, id: VideoId, index: u64) -> bool {
    let tab = h.app.video(id).unwrap();
    tab.displayed() == Some(Displayed { index, exact: true }) && !tab.is_busy()
}

fn open(h: &mut Harness, clip: &str) -> VideoId {
    assert_eq!(
        h.app.open_path(&fixture(clip)),
        None,
        "videos open in a video tab"
    );
    let id = *h.app.video_tabs().last().expect("video tab added");
    wait_until(h, "frame 0", |h| shows(h, id, 0));
    h.frame(vec![]);
    id
}

fn has_text(output: &egui::FullOutput, needle: &str) -> bool {
    output.shapes.iter().any(|s| match &s.shape {
        egui::Shape::Text(t) => t.galley.text().contains(needle),
        _ => false,
    })
}

fn text_pos(output: &egui::FullOutput, needle: &str) -> Option<Pos2> {
    output.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text().contains(needle) => Some(t.pos),
        _ => None,
    })
}

fn layer_pixels(h: &Harness, id: DocId) -> RgbaImage {
    h.app
        .state
        .document(id)
        .unwrap()
        .document
        .layer(0)
        .unwrap()
        .pixels
        .clone()
}

fn newest_document(h: &Harness) -> DocId {
    *h.app.document_tabs().last().expect("a document")
}

#[test]
fn opening_a_video_shows_frame_zero_and_its_info() {
    let mut h = Harness::new();
    let id = open(&mut h, "clip.mp4");
    let tab = h.app.video(id).unwrap();
    assert!(tab.displayed_image().unwrap() == &reference("clip_mp4_f0.png"));
    let out = h.frame(vec![]);
    assert!(has_text(&out, "clip.mp4"), "tab title");
    assert!(has_text(&out, "160 × 120"));
    assert!(has_text(&out, "30.000 fps"));
    assert!(has_text(&out, "frame 0 / 90"));
    assert!(has_text(&out, "00:00:03.000"), "duration");
    assert!(h.app.document_tabs().is_empty(), "no image document");
    // Every container opens.
    for clip in ["clip.webm", "clip.mkv", "clip.avi", "clip.mov"] {
        open(&mut h, clip);
    }
    assert_eq!(h.app.video_tabs().len(), 5);
}

#[test]
fn unreadable_video_shows_an_error_and_adds_no_tab() {
    let mut h = Harness::new();
    assert_eq!(h.app.open_path(&fixture("corrupt.mp4")), None);
    assert!(h.app.video_tabs().is_empty());
    assert!(h.app.error_message().unwrap().contains("corrupt.mp4"));
}

#[test]
fn rotated_video_displays_and_grabs_upright() {
    let mut h = Harness::new();
    let id = open(&mut h, "rotated.mov");
    let out = h.frame(vec![]);
    assert!(has_text(&out, "120 × 160"));
    let rect = h.app.video(id).unwrap().layout().frame;
    assert!(rect.height() > rect.width(), "portrait view {rect:?}");
    h.app.video_mut(id).unwrap().seek(17);
    wait_until(&mut h, "frame 17", |h| shows(h, id, 17));
    assert!(h.app.grab_frame(id));
    wait_until(&mut h, "grabbed document", |h| {
        !h.app.document_tabs().is_empty()
    });
    assert!(layer_pixels(&h, newest_document(&h)) == reference("rotated_mov_f17.png"));
}

#[test]
fn scrubbing_previews_keyframes_then_shows_the_exact_frame() {
    let mut h = Harness::new();
    let id = open(&mut h, "clip.mp4");
    let timeline = h.app.video(id).unwrap().layout().timeline;
    let from = Pos2::new(timeline.left() + 4.0, timeline.center().y);
    let to = Pos2::new(timeline.center().x, timeline.center().y);
    h.pointer = from;
    h.frame(vec![]);
    let down = h.press(egui::PointerButton::Primary, true);
    h.frame(vec![down]);
    for i in 1..=8 {
        h.pointer = from + (to - from) * (i as f32 / 8.0);
        h.frame(vec![]);
    }
    // While held: a keyframe preview at or before the pointer position.
    wait_until(&mut h, "preview", |h| {
        let tab = h.app.video(id).unwrap();
        tab.displayed().is_some_and(|d| !d.exact) && !tab.is_busy()
    });
    let tab = h.app.video(id).unwrap();
    let position = tab.position();
    let preview = tab.displayed().unwrap().index;
    assert!((40..=50).contains(&position), "pointer at {position}");
    assert!(
        preview <= position && preview.is_multiple_of(30),
        "keyframe {preview} for {position}"
    );
    let out = h.frame(vec![]);
    assert!(has_text(&out, "keyframe preview"));
    assert!(
        has_text(&out, &format!("frame {preview} / 90")),
        "counter follows the display"
    );
    // Release: the exact frame.
    let up = h.press(egui::PointerButton::Primary, false);
    h.frame(vec![up]);
    wait_until(&mut h, "exact frame", |h| shows(h, id, position));
    let mut source = VideoSource::open(&fixture("clip.mp4")).unwrap();
    assert!(
        h.app.video(id).unwrap().displayed_image().unwrap()
            == &source.frame(position).unwrap().image
    );
}

#[test]
fn transport_buttons_and_shortcuts() {
    let mut h = Harness::new();
    let id = open(&mut h, "clip.mp4");
    h.tap_key(Key::End);
    wait_until(&mut h, "last frame", |h| shows(h, id, 89));
    assert!(h.app.video(id).unwrap().displayed_image().unwrap() == &reference("clip_mp4_f89.png"));
    h.tap_key(Key::Home);
    wait_until(&mut h, "first frame", |h| shows(h, id, 0));
    h.app.video_mut(id).unwrap().seek(16);
    wait_until(&mut h, "frame 16", |h| shows(h, id, 16));
    h.tap_key(Key::ArrowRight);
    wait_until(&mut h, "frame 17", |h| shows(h, id, 17));
    assert!(h.app.video(id).unwrap().displayed_image().unwrap() == &reference("clip_mp4_f17.png"));
    h.app.video_mut(id).unwrap().seek(46);
    wait_until(&mut h, "frame 46", |h| shows(h, id, 46));
    h.tap_key(Key::ArrowLeft);
    wait_until(&mut h, "frame 45", |h| shows(h, id, 45));
    assert!(h.app.video(id).unwrap().displayed_image().unwrap() == &reference("clip_mp4_f45.png"));

    // Space plays in real time and pauses on an exact frame.
    let start = Instant::now();
    h.tap_key(Key::Space);
    assert!(h.app.video(id).unwrap().is_playing());
    wait_until(&mut h, "playback to advance", |h| {
        h.app.video(id).unwrap().displayed().unwrap().index >= 60
    });
    let elapsed = start.elapsed().as_secs_f64();
    assert!(
        elapsed > 0.4,
        "plays at about 30 fps, not as fast as possible ({elapsed:.2}s for 15 frames)"
    );
    h.tap_key(Key::Space);
    assert!(!h.app.video(id).unwrap().is_playing());
    wait_until(&mut h, "paused exact frame", |h| {
        let tab = h.app.video(id).unwrap();
        !tab.is_busy() && tab.displayed().unwrap().exact
    });

    // Playing to the end stops on the last frame.
    h.app.video_mut(id).unwrap().seek(80);
    wait_until(&mut h, "frame 80", |h| shows(h, id, 80));
    let out = h.frame(vec![]);
    let play = text_pos(&out, "Play").expect("Play button");
    h.click_at(play + egui::vec2(4.0, 4.0));
    assert!(h.app.video(id).unwrap().is_playing(), "Play button");
    wait_until(&mut h, "end of playback", |h| {
        !h.app.video(id).unwrap().is_playing()
    });
    assert!(shows(&h, id, 89));
}

#[test]
fn thumbnail_strip_fills_in_and_seeks_on_click() {
    let mut h = Harness::new();
    let id = open(&mut h, "clip.mp4");
    wait_until(&mut h, "thumbnails", |h| {
        let thumbs = h.app.video(id).unwrap().thumbnails();
        thumbs.len() > 2 && thumbs.iter().all(Option::is_some)
    });
    let thumbs: Vec<u64> = h
        .app
        .video(id)
        .unwrap()
        .thumbnails()
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(thumbs[0], 0);
    assert!(thumbs.windows(2).all(|w| w[0] <= w[1]), "{thumbs:?}");
    assert_eq!(*thumbs.last().unwrap(), 60, "keyframe for the last slot");
    let tab = h.app.video(id).unwrap();
    assert_eq!(
        tab.current_slot(0),
        Some(0),
        "first of equal thumbnails is highlighted"
    );
    let last_slot = thumbs.len() - 1;
    let first_60 = thumbs.iter().position(|&f| f == 60).unwrap();
    assert_eq!(tab.current_slot(75), Some(first_60));
    assert!(first_60 <= last_slot);
    let rects = h.app.video(id).unwrap().layout().thumbnails.clone();
    let last = rects.len() - 1;
    h.click_at(rects[last].center());
    wait_until(&mut h, "seek to thumbnail", |h| shows(h, id, 60));
}

#[test]
fn grab_frame_opens_the_exact_frame_as_a_document() {
    let mut h = Harness::new();
    let id = open(&mut h, "lossless.mkv");
    h.app.video_mut(id).unwrap().seek(45);
    wait_until(&mut h, "frame 45", |h| shows(h, id, 45));
    let out = h.frame(vec![]);
    let grab = text_pos(&out, "Grab Frame").unwrap();
    h.click_at(grab + egui::vec2(4.0, 4.0));
    wait_until(&mut h, "grabbed document", |h| {
        !h.app.document_tabs().is_empty()
    });
    let doc = newest_document(&h);
    let open = h.app.state.document(doc).unwrap();
    assert_eq!(open.document.name, "lossless frame 45");
    assert_eq!(open.document.layers().len(), 1);
    assert_eq!(open.document.layer(0).unwrap().name, "Background");
    assert_eq!(open.document.dpi(), 72.0);
    assert!(open.document.source_path.is_none());
    assert!(layer_pixels(&h, doc) == reference("lossless_f45.png"));

    // Grabbing while a keyframe preview shows decodes the exact frame first.
    h.app.video_mut(id).unwrap().seek_preview(89);
    h.app.grab_frame(id);
    wait_until(&mut h, "second grab", |h| h.app.document_tabs().len() == 2);
    let doc = newest_document(&h);
    assert_eq!(
        h.app.state.document(doc).unwrap().document.name,
        "lossless frame 89"
    );
    assert!(layer_pixels(&h, doc) == reference("lossless_f89.png"));
}

#[test]
fn extract_every_nth_frame_appends_to_the_frame_set() {
    let mut h = Harness::new();
    let id = open(&mut h, "clip.mp4");
    let out = h.frame(vec![]);
    h.click_at(text_pos(&out, "Extract Frames").unwrap() + egui::vec2(4.0, 4.0));
    h.frames(2);
    let dialog = h
        .app
        .extract_dialog_mut()
        .expect("dialog opened by the button");
    assert_eq!((dialog.from, dialog.to, dialog.n), (0, 89, 30));
    dialog.n = 40;
    assert!(h.app.start_extract());
    wait_until(&mut h, "extraction", |h| h.app.extract_dialog().is_none());
    let frames: Vec<u64> = h
        .app
        .state
        .frame_set
        .entries()
        .iter()
        .map(|e| e.frame)
        .collect();
    assert_eq!(frames, vec![0, 40, 80]);
    assert!(h.app.is_panel_open(Panel::FrameSet));
    assert!(h.app.status_message().unwrap().contains("Added 3 frames"));

    // A second batch appends.
    h.app.open_extract_dialog(id);
    let dialog = h.app.extract_dialog_mut().unwrap();
    dialog.from = 10;
    dialog.to = 20;
    dialog.n = 5;
    h.app.start_extract();
    wait_until(&mut h, "second extraction", |h| {
        h.app.extract_dialog().is_none()
    });
    let frames: Vec<u64> = h
        .app
        .state
        .frame_set
        .entries()
        .iter()
        .map(|e| e.frame)
        .collect();
    assert_eq!(frames, vec![0, 40, 80, 10, 15, 20]);
    let out = h.frame(vec![]);
    assert!(has_text(&out, "6 frames"));
    assert!(has_text(&out, "clip.mp4 #40"));
}

#[test]
fn scene_extraction_finds_the_cuts_and_cancel_adds_nothing() {
    let mut h = Harness::new();
    let id = open(&mut h, "scenes.mp4");
    h.app.open_extract_dialog(id);
    let dialog = h.app.extract_dialog_mut().unwrap();
    dialog.mode = ExtractChoice::SceneChanges;
    assert_eq!((dialog.threshold, dialog.min_scene), (30, 10));
    h.app.start_extract();
    // Cancel right away: nothing is added and the dialog closes.
    h.app.cancel_extract();
    h.frames(3);
    assert!(h.app.state.frame_set.is_empty());
    assert!(h.app.extract_dialog().is_none());

    h.app.open_extract_dialog(id);
    h.app.extract_dialog_mut().unwrap().mode = ExtractChoice::SceneChanges;
    h.app.start_extract();
    h.frame(vec![]);
    let out = h.frame(vec![]);
    assert!(h.app.extract_dialog().is_none() || has_text(&out, "Extract Frames"));
    wait_until(&mut h, "scene extraction", |h| {
        h.app.extract_dialog().is_none()
    });
    let frames: Vec<u64> = h
        .app
        .state
        .frame_set
        .entries()
        .iter()
        .map(|e| e.frame)
        .collect();
    assert_eq!(frames, vec![0, 45, 85, 115, 165, 200]);
}

#[test]
fn frame_set_panel_opens_removes_and_clears() {
    let mut h = Harness::new();
    let id = open(&mut h, "lossless.mkv");
    h.app.open_extract_dialog(id);
    h.app.extract_dialog_mut().unwrap().n = 45;
    h.app.start_extract();
    wait_until(&mut h, "extraction", |h| h.app.extract_dialog().is_none());
    assert_eq!(h.app.state.frame_set.len(), 2); // frames 0 and 45
    h.frames(2);
    // Double-click the "#45" cell to open it.
    let out = h.frame(vec![]);
    let label = text_pos(&out, "lossless.mkv #45").expect("frame label");
    let cell = label + egui::vec2(20.0, -30.0);
    h.click_at(cell);
    h.click();
    let doc = newest_document(&h);
    assert_eq!(
        h.app.state.document(doc).unwrap().document.name,
        "lossless frame 45"
    );
    assert!(layer_pixels(&h, doc) == reference("lossless_f45.png"));
    // Same pixels as Grab Frame of that frame.
    h.app.video_mut(id).unwrap().seek(45);
    wait_until(&mut h, "frame 45", |h| shows(h, id, 45));
    h.app.grab_frame(id);
    wait_until(&mut h, "grab", |h| h.app.document_tabs().len() == 2);
    assert!(layer_pixels(&h, newest_document(&h)) == layer_pixels(&h, doc));

    // The single click selected #45: Remove deletes it.
    let selected: Vec<u64> = h.app.frame_set_view().selected.iter().copied().collect();
    assert_eq!(selected.len(), 1);
    assert_eq!(h.app.state.frame_set.get(selected[0]).unwrap().frame, 45);
    h.app.frame_set_action(FrameSetAction::Remove(selected));
    let frames: Vec<u64> = h
        .app
        .state
        .frame_set
        .entries()
        .iter()
        .map(|e| e.frame)
        .collect();
    assert_eq!(frames, vec![0]);
    h.app.frame_set_action(FrameSetAction::Clear);
    assert!(h.app.state.frame_set.is_empty());
    let out = h.frame(vec![]);
    assert!(has_text(&out, "0 frames"));
}

#[test]
fn closing_a_video_tab_releases_it() {
    let mut h = Harness::new();
    let a = open(&mut h, "clip.mp4");
    let b = open(&mut h, "clip.webm");
    h.app.close_video(a);
    assert!(h.app.video(a).is_none());
    assert_eq!(h.app.video_tabs(), vec![b]);
    // File → Close (Cmd+W) closes the focused video tab.
    h.shortcut(egui::Modifiers::COMMAND, Key::W);
    assert!(h.app.video_tabs().is_empty());
    assert!(h.app.video(b).is_none());
}
